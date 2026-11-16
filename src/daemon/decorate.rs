//! Adding the sigil and grimoire panes to tabs, re-adopting them after
//! restarts, and reacting to herdr events.
//!
//! Probe findings (Task 9, herdr 0.9.5, isolated dev server):
//! - Tab ids survive `herdr server stop` + restart with restore, gaps
//!   included (`w1:t1 w1:t3 w1:t6 w1:t8` came back as-is; a closed `w1:t7`
//!   stayed retired). Pane ids survive too, and plugin panes are relaunched
//!   with their label. So `TabKey` is the tab id.
//! - `plugin.pane.open` panes report the manifest pane `title` as
//!   `PaneInfo.label` (title is None), also after a restore. Re-adoption
//!   matches on `SIGIL_TITLE` / `GRIM_TITLE`.
//! - `GRIM_OUTER_ROWS` = 3 (see `sizing`). Every split ratio is clamped to
//!   [0.1, 0.9]; the first child gets `round(rows * ratio)` rows.
//! - `layout.set_split_ratio` path `[]` is the root split, `[false]` its
//!   first child (Task 8, confirmed).
//! - Creating a workspace emits `tab_created` for its first tab (Task 8),
//!   so no `workspace.created` subscription is needed.
//! - `pane.updated` does not fire on output changes: see `typing`.
//! - `server stop` emits no `tab_closed` / `pane_closed`; the event stream
//!   just ends. So records are never wiped by a shutdown.
//! - `pane.move` to a new tab emits `tab_created` before `pane_moved`
//!   (Task 8), hence `pending` + `tick`.
//! - `layout.apply` does not rearrange a tab in place: it replaced `w1:t8`
//!   by a new tab `w1:t9` with new panes (the old processes were killed),
//!   and its ratios are clamped too.
//!
//! Task 10 probes (herdr 0.9.5, isolated dev server):
//! - A plugin pane whose process ends by itself (killed, crashed) gets
//!   `pane_exited` and disappears; there is no `pane_closed`. Closing it
//!   with `pane.close` or `plugin.pane.close` gives `pane_closed` and no
//!   `pane_exited`. So only `pane_closed` is a closure; `pane_exited`
//!   re-adds the pane (throttled, see `RESPAWN_COOLDOWN_MS`).
//! - `plugin disable` / `plugin unlink` emit nothing and leave the plugin's
//!   panes (and its startup process) running, so they never look like
//!   closures either.
//!
//! Final-review probes (herdr 0.9.5, isolated dev server):
//! - herdr's close keybinding (prefix+x) on a witch pane, in an attached
//!   client, sends `pane_closed` only, like `pane.close`. On a content pane
//!   too. So a `pane_exited` really is a process ending by itself.
//! - When the last content pane goes (its shell exits: `pane_exited`; it is
//!   closed: `pane_closed`; or moved away: `pane_moved`), herdr keeps a tab
//!   that holds only plugin panes. `close_orphans` closes it.
//! - After `server stop` + restart, herdr restores plugin panes with their
//!   id, label and saved `launch_argv`, but runs a plain shell in them, not
//!   the plugin's program. `reconcile` therefore replaces those shells
//!   (`restore::replace_shells`).
//!   (Task 9 saw the labels come back and took them for relaunches.)

use super::restore::replace_shells;
use super::sizing::{DragTracker, SizingAction, grimoire_ratio, sigil_ratio};
use super::store::{Kind, STORE_FILE, Store, TabKey, TabRecord, tab_key};
use super::summon::open_at_corners;
use super::typing::TypingDetector;
use crate::config::Config;
use crate::grimoire::pool::LinePool;
use crate::herdr::{Dir, Event, HerdrApi, HerdrError, OpenPane, PaneInfo, Result};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Manifest pane titles; herdr reports them as `PaneInfo.label`.
pub const SIGIL_TITLE: &str = "✶ sigil";
pub const GRIM_TITLE: &str = "✶ grimoire";

/// A `tab_created` is acted on this long after it arrives, so a
/// `pane_moved` for the same tab can still mark it as move-created (herdr
/// sends `tab_created` first). With the runtime's 100 ms loop, a new tab is
/// therefore decorated 400-500 ms after it appears (documented in README).
pub const MOVE_GRACE_MS: u64 = 400;

/// A tab's witch panes are re-added after a `pane_exited` at most once in
/// this long, so a pane that dies at once can't loop. A throttled pane is
/// left missing (not closed) until the next reconnect or Summon.
pub const RESPAWN_COOLDOWN_MS: u64 = 10_000;

pub(super) const KINDS: [Kind; 2] = [Kind::Sigil, Kind::Grimoire];

impl Kind {
    pub fn title(self) -> &'static str {
        match self {
            Kind::Sigil => SIGIL_TITLE,
            Kind::Grimoire => GRIM_TITLE,
        }
    }

    /// The manifest `panes[].id`.
    pub fn entrypoint(self) -> &'static str {
        match self {
            Kind::Sigil => "sigil",
            Kind::Grimoire => "grimoire",
        }
    }
}

/// Is this pane one of the plugin's, by its label?
pub fn is_witch(p: &PaneInfo) -> bool {
    matches!(p.label.as_deref(), Some(SIGIL_TITLE | GRIM_TITLE))
}

/// Everything the daemon logic keeps between events, grouped so the entry
/// points take one `&mut DaemonState` instead of seven arguments.
pub struct DaemonState {
    /// Where `tabs.json` lives (`paths::state_dir()` in the daemon).
    pub dir: PathBuf,
    pub store: Store,
    pub pool: LinePool,
    /// Tabs seen in `tab_created`, by tab id, with the time it arrived.
    pub pending: HashMap<String, u64>,
    /// Tabs made by `pane.move`; they are never decorated.
    pub created_by_move: HashSet<String>,
    /// The tab ids that existed at the first `reconcile` (pre-install tabs).
    pub baseline: Option<HashSet<String>>,
    pub drag: DragTracker,
    pub typing: TypingDetector,
    /// Pane ids of witch panes, excluded from typing detection.
    pub ours: HashSet<String>,
    /// When each tab last had a witch pane re-added after `pane_exited`.
    pub respawned: HashMap<TabKey, u64>,
}

impl DaemonState {
    /// Fresh state with the store loaded from `dir`.
    pub fn new(dir: PathBuf, pool: LinePool) -> Self {
        DaemonState {
            store: Store::load(&dir),
            dir,
            pool,
            pending: HashMap::new(),
            created_by_move: HashSet::new(),
            baseline: None,
            drag: DragTracker::default(),
            typing: TypingDetector::default(),
            ours: HashSet::new(),
            respawned: HashMap::new(),
        }
    }

    /// Persist the store; a failure is logged, not fatal.
    pub fn save(&self) {
        if let Err(e) = self.store.save(&self.dir) {
            eprintln!(
                "herdr-witch: saving {}: {e}",
                self.dir.join(STORE_FILE).display()
            );
        }
    }
}

/// Decorate one tab, creating its record (and grimoire line) if needed.
/// Idempotent: witch panes already in the tab are re-adopted (by recorded
/// id, else by label), closed kinds are skipped, and only missing kinds are
/// opened, all with `focus: false`. Opening needs the tab's content to be
/// a single pane; a multi-pane tab is only re-adopted (Summon handles it),
/// and one with no witch panes gets no record at all.
pub fn decorate_tab(
    api: &mut dyn HerdrApi,
    st: &mut DaemonState,
    tab_id: &str,
    cfg: &Config,
) -> Result<()> {
    decorate(api, st, tab_id, cfg, false)
}

/// `decorate_tab`, and with `wrap` also the layouts it leaves alone: a tab
/// with several content panes, or a sigil without a grimoire, gets its
/// missing panes by splitting corner panes (`summon::open_at_corners`).
pub(super) fn decorate(
    api: &mut dyn HerdrApi,
    st: &mut DaemonState,
    tab_id: &str,
    cfg: &Config,
    wrap: bool,
) -> Result<()> {
    let panes = api.panes(tab_id)?;
    if panes.is_empty() {
        return Ok(()); // the tab is gone
    }
    let key = TabKey::from(tab_id);
    let known = st.store.tabs.get(&key);
    let is_new = known.is_none();
    let mut rec = known.cloned().unwrap_or_default();
    adopt(&mut rec, &panes, &mut st.ours);
    let content: Vec<String> = panes
        .iter()
        .filter(|p| !is_witch(p) && !KINDS.iter().any(|&k| rec.pane(k) == Some(&p.pane_id)))
        .map(|p| p.pane_id.clone())
        .collect();
    let orig = match &content[..] {
        [orig] => Some(orig.clone()),
        _ => None,
    };
    let has_witch = rec.sigil_pane.is_some() || rec.grim_pane.is_some();
    if is_new && orig.is_none() && !has_witch && !wrap {
        return Ok(()); // left alone: no record, no grimoire line used
    }
    if is_new {
        rec.line = st.pool.assign(&st.store.live_lines());
    }
    // The plain layout (grimoire under everything, sigil beside the content)
    // needs one content pane and no sigil yet.
    let plain = rec.sigil_pane.is_none();
    st.store.tabs.insert(key.clone(), rec);
    let result = match orig {
        Some(orig) if plain => open_missing(api, st, &key, &orig, cfg),
        _ if wrap => open_at_corners(api, st, &key, &content, cfg),
        _ => Ok(()),
    };
    st.save(); // also keeps whatever opened before an error
    result
}

/// Point `rec` at the witch panes in `panes`: a recorded id that is still
/// there wins (even if the user relabelled it), else a pane with the kind's
/// label. A recorded pane that is gone without a `pane_closed` (e.g. herdr
/// restored the tab but not its plugin panes) counts as missing, not
/// closed, and is dropped from `ours`.
fn adopt(rec: &mut TabRecord, panes: &[PaneInfo], ours: &mut HashSet<String>) {
    for kind in KINDS {
        let recorded = rec
            .pane(kind)
            .filter(|id| panes.iter().any(|p| p.pane_id == *id))
            .map(str::to_string);
        let id = recorded.or_else(|| {
            panes
                .iter()
                .find(|p| p.label.as_deref() == Some(kind.title()))
                .map(|p| p.pane_id.clone())
        });
        if let Some(old) = rec.pane(kind).filter(|old| Some(*old) != id.as_deref()) {
            ours.remove(old);
        }
        ours.extend(id.clone());
        rec.set_pane(kind, id);
    }
}

/// The kinds the tab is missing and the user hasn't closed: (grimoire,
/// sigil).
pub(super) fn wanted(rec: &TabRecord) -> (bool, bool) {
    (
        rec.grim_pane.is_none() && !rec.grim_closed,
        rec.sigil_pane.is_none() && !rec.sigil_closed,
    )
}

/// Split `target` with a witch pane of `kind` (`focus: false`, always) and
/// record it. The grimoire gets the tab's line in `HERDR_WITCH_LINE`.
pub(super) fn open_witch(
    api: &mut dyn HerdrApi,
    st: &mut DaemonState,
    key: &TabKey,
    kind: Kind,
    target: &str,
    direction: Dir,
) -> Result<()> {
    let rec = st
        .store
        .tabs
        .get_mut(key)
        .expect("record created by caller");
    let env = match kind {
        Kind::Grimoire => vec![("HERDR_WITCH_LINE".into(), rec.line.to_string())],
        Kind::Sigil => Vec::new(),
    };
    let id = api.open_plugin_pane(OpenPane {
        entrypoint: kind.entrypoint(),
        target_pane_id: target.to_string(),
        direction,
        env,
    })?;
    rec.set_pane(kind, Some(id.clone()));
    st.ours.insert(id);
    Ok(())
}

fn open_missing(
    api: &mut dyn HerdrApi,
    st: &mut DaemonState,
    key: &TabKey,
    orig: &str,
    cfg: &Config,
) -> Result<()> {
    let rec = &st.store.tabs[key];
    let (mut want_grim, want_sigil) = wanted(rec);
    // A full-width grimoire under an existing sigil can't be had by a split.
    want_grim &= rec.sigil_pane.is_none();
    if !want_grim && !want_sigil {
        return Ok(());
    }
    let tab_id = key.0.as_str();
    let area = api.layout(tab_id)?.area;
    if want_grim {
        open_witch(api, st, key, Kind::Grimoire, orig, Dir::Down)?;
    }
    if want_sigil {
        open_witch(api, st, key, Kind::Sigil, orig, Dir::Right)?;
    }
    let has_grim = st.store.tabs[key].grim_pane.is_some();
    if want_grim {
        api.set_split_ratio(tab_id, &[], grimoire_ratio(area.height))?;
        st.drag.seed(key, (area.width, area.height));
    }
    if want_sigil {
        // Splitting `orig` right nests it under the grimoire split, if any.
        let path: &[bool] = if has_grim { &[false] } else { &[] };
        api.set_split_ratio(tab_id, path, sigil_ratio(cfg.sigil_width_pct))?;
    }
    Ok(())
}

/// A remote error (a tab or pane closed mid-call, say) is logged and
/// skipped; a socket error is returned so the runtime can reconnect.
pub(super) fn skip_remote(what: &str, e: HerdrError) -> Result<()> {
    match e {
        HerdrError::Remote { .. } => {
            eprintln!("herdr-witch: {what}: {e}");
            Ok(())
        }
        e => Err(e),
    }
}

/// On daemon start (and after a reconnect): decorate every tab with a
/// record, plus tabs that appeared since the first reconcile. Tabs present
/// at the first reconcile with no record predate install and are left
/// alone. A per-tab remote error is logged and skipped; a socket error
/// aborts.
pub fn reconcile(api: &mut dyn HerdrApi, st: &mut DaemonState, cfg: &Config) -> Result<()> {
    let tabs = api.tabs()?;
    let baseline = st
        .baseline
        .get_or_insert_with(|| tabs.iter().map(|t| t.tab_id.clone()).collect())
        .clone();
    for t in &tabs {
        let id = &t.tab_id;
        if st.pending.contains_key(id) || st.created_by_move.contains(id) {
            continue; // `tick` or nobody handles these
        }
        let new_since_start = cfg.auto_open && !baseline.contains(id);
        if !st.store.tabs.contains_key(&tab_key(t)) && !new_since_start {
            continue;
        }
        // A tab that lost restored shells gets them back whatever its layout.
        let redo = replace_shells(api, st, id).and_then(|wrap| decorate(api, st, id, cfg, wrap));
        if let Err(e) = redo {
            skip_remote(&format!("decorating {id}"), e)?;
        }
    }
    Ok(())
}

/// Handle one herdr event. `tab_created` only queues the tab; `tick`
/// decorates it once `MOVE_GRACE_MS` has passed. A remote error (the tab or
/// pane is already gone) is logged and skipped; only socket errors return.
/// `pane_updated` is ignored: herdr 0.9.5 never bumps revisions on output,
/// so typing comes only from `TypingDetector::poll`.
pub fn on_event(
    api: &mut dyn HerdrApi,
    st: &mut DaemonState,
    cfg: &Config,
    ev: &Event,
    now_ms: u64,
) -> Result<()> {
    handle(api, st, cfg, ev, now_ms).or_else(|e| skip_remote(&format!("handling {ev:?}"), e))
}

fn handle(
    api: &mut dyn HerdrApi,
    st: &mut DaemonState,
    cfg: &Config,
    ev: &Event,
    now_ms: u64,
) -> Result<()> {
    match ev {
        Event::TabCreated(t) => {
            if cfg.auto_open && !st.created_by_move.contains(&t.tab_id) {
                st.pending.entry(t.tab_id.clone()).or_insert(now_ms);
            }
        }
        Event::PaneMoved {
            pane_id,
            previous_pane_id,
            created_tab,
        } => {
            if let Some(t) = created_tab {
                st.pending.remove(t);
                st.created_by_move.insert(t.clone());
            }
            if let Some((key, kind)) = st.store.find_pane(previous_pane_id) {
                moved_witch(api, st, &key, kind, previous_pane_id, pane_id)?;
            }
            close_orphans(api, st)?;
        }
        // The bookkeeping comes before `close_orphans`, which needs the
        // socket: a closure must survive a socket error.
        Event::PaneClosed { pane_id, .. } => {
            st.ours.remove(pane_id);
            if let Some((key, kind)) = st.store.find_pane(pane_id) {
                st.store
                    .tabs
                    .get_mut(&key)
                    .expect("found")
                    .mark_closed(kind);
                st.save();
            }
            close_orphans(api, st)?;
        }
        Event::PaneExited { pane_id } => {
            st.ours.remove(pane_id);
            let found = st.store.find_pane(pane_id);
            if let Some((key, kind)) = &found {
                // Gone, but not closed by the user: it may come back.
                st.store
                    .tabs
                    .get_mut(key)
                    .expect("found")
                    .set_pane(*kind, None);
                st.save();
            }
            // Before respawning: a tab left with only witch panes is closed.
            close_orphans(api, st)?;
            if let Some((key, _)) = found.filter(|(k, _)| st.store.tabs.contains_key(k)) {
                respawn(api, st, &key, cfg, now_ms)?;
            }
        }
        Event::TabClosed { tab_id } => {
            let key = TabKey::from(tab_id.as_str());
            st.pending.remove(tab_id);
            st.created_by_move.remove(tab_id);
            st.respawned.remove(&key);
            st.drag.forget(&key);
            if let Some(rec) = st.store.tabs.remove(&key) {
                for id in [rec.sigil_pane, rec.grim_pane].into_iter().flatten() {
                    st.ours.remove(&id);
                }
                st.save();
            }
        }
        Event::LayoutUpdated { tab_id } => resize(api, st, tab_id)?,
        Event::TabFocused { .. } | Event::Other => {} // see `Runtime::step`
    }
    Ok(())
}

/// Close every recorded tab left holding only witch panes (its last content
/// pane exited, was closed or moved away; the events don't say which tab
/// it was in). herdr keeps such a tab, so the daemon removes the witch
/// panes, and herdr closes the tab with its last pane. The tab is forgotten
/// first, so those closes are neither closures nor respawns.
///
/// One `tab.list`; a tab is only listed pane by pane if it has at most two
/// panes (it can hold no more witch panes than that).
fn close_orphans(api: &mut dyn HerdrApi, st: &mut DaemonState) -> Result<()> {
    let tabs = api.tabs()?;
    let candidates: Vec<TabKey> = tabs
        .iter()
        .filter(|t| t.pane_count as usize <= KINDS.len())
        .map(tab_key)
        .filter(|k| {
            st.store
                .tabs
                .get(k)
                .is_some_and(|rec| KINDS.iter().any(|&kd| rec.pane(kd).is_some()))
        })
        .collect();
    for key in candidates {
        let panes = api.panes(&key.0)?;
        let rec = &st.store.tabs[&key];
        // A record can go stale and name a user's pane: an id alone counts
        // only if the daemon also opened that pane (`ours`).
        let ours = |p: &PaneInfo| {
            is_witch(p)
                || (st.ours.contains(&p.pane_id)
                    && KINDS.iter().any(|&k| rec.pane(k) == Some(&p.pane_id)))
        };
        if panes.is_empty() || !panes.iter().all(ours) {
            continue;
        }
        eprintln!("herdr-witch: {} holds only witch panes; closing it", key.0);
        st.store.tabs.remove(&key);
        st.drag.forget(&key);
        st.respawned.remove(&key);
        for p in &panes {
            st.ours.remove(&p.pane_id);
        }
        st.save();
        for p in panes {
            if let Err(e) = api.remove_pane(&p.pane_id) {
                skip_remote(&format!("closing {}", p.pane_id), e)?;
            }
        }
    }
    Ok(())
}

/// Re-add the tab's missing witch panes after one's process died, unless
/// that already happened within `RESPAWN_COOLDOWN_MS`.
fn respawn(
    api: &mut dyn HerdrApi,
    st: &mut DaemonState,
    key: &TabKey,
    cfg: &Config,
    now_ms: u64,
) -> Result<()> {
    let recent = st.respawned.get(key);
    if recent.is_some_and(|&at| now_ms.saturating_sub(at) < RESPAWN_COOLDOWN_MS) {
        eprintln!(
            "herdr-witch: a witch pane in {} keeps exiting; not re-adding it for now",
            key.0
        );
        return Ok(());
    }
    st.respawned.insert(key.clone(), now_ms);
    decorate(api, st, &key.0, cfg, true)
}

/// A recorded witch pane was moved. If its tab still holds a pane of that
/// kind (a move within the tab) follow it; otherwise the user took it
/// away, so the tab won't get it back.
fn moved_witch(
    api: &mut dyn HerdrApi,
    st: &mut DaemonState,
    key: &TabKey,
    kind: Kind,
    old_id: &str,
    new_id: &str,
) -> Result<()> {
    st.ours.remove(old_id);
    st.ours.insert(new_id.to_string());
    let here = api
        .panes(&key.0)?
        .into_iter()
        .find(|p| p.label.as_deref() == Some(kind.title()));
    let rec = st.store.tabs.get_mut(key).expect("found");
    match here {
        Some(p) => rec.set_pane(kind, Some(p.pane_id)),
        None => rec.mark_closed(kind),
    }
    st.save();
    Ok(())
}

/// Keep the grimoire at its target height when the window resizes, and
/// stop for good once the user drags its split.
fn resize(api: &mut dyn HerdrApi, st: &mut DaemonState, tab_id: &str) -> Result<()> {
    let key = TabKey::from(tab_id);
    let Some(rec) = st.store.tabs.get(&key) else {
        return Ok(());
    };
    let Some(grim) = rec.grim_pane.clone().filter(|_| !rec.user_sized) else {
        return Ok(());
    };
    let l = api.layout(tab_id)?;
    let (a, g) = match l.panes.iter().find(|p| p.pane_id == grim) {
        Some(g) if !l.zoomed => (l.area, g.rect),
        _ => return Ok(()),
    };
    // Only touch the root split, and only while the grimoire is exactly its
    // second child (the user may have rearranged the tab).
    let at_root = l.splits.iter().any(|s| {
        let first = (f32::from(a.height) * s.ratio).round() as u16;
        s.id == "split_0_root" && s.direction == Dir::Down && g.y == first
    });
    if !at_root || g.width != a.width || g.y + g.height != a.height {
        return Ok(());
    }
    match st.drag.on_layout(&key, (a.width, a.height), g.height) {
        SizingAction::Reapply => api.set_split_ratio(tab_id, &[], grimoire_ratio(a.height))?,
        SizingAction::MarkUserSized => {
            st.store.tabs.get_mut(&key).expect("found").user_sized = true;
            st.save();
        }
        SizingAction::Nothing => {}
    }
    Ok(())
}

/// Decorate queued tabs older than `MOVE_GRACE_MS` that weren't made by a
/// pane move. Call it at least every ~100 ms.
pub fn tick(api: &mut dyn HerdrApi, st: &mut DaemonState, cfg: &Config, now_ms: u64) -> Result<()> {
    let mut due: Vec<String> = st
        .pending
        .iter()
        .filter(|&(_, &at)| now_ms.saturating_sub(at) >= MOVE_GRACE_MS)
        .map(|(id, _)| id.clone())
        .collect();
    due.sort();
    for id in due {
        st.pending.remove(&id);
        if st.created_by_move.contains(&id) {
            continue;
        }
        if let Err(e) = decorate_tab(api, st, &id, cfg) {
            skip_remote(&format!("decorating {id}"), e)?;
        }
    }
    Ok(())
}
