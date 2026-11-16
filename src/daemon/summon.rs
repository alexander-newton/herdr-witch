//! "Summon here": add a tab's missing witch panes whatever its layout, and
//! the hand-off from the action's process to the daemon.
//!
//! The action runs in its own short-lived process, but the daemon owns the
//! tab records (it saves them on every event, and only it sees the
//! `pane_closed` that makes a closure). So the action only queues a request
//! file in the state dir, and the daemon serves it from its loop. herdr runs
//! `startup` hooks only when the server starts, not on `plugin link`, so if
//! no daemon holds the lock the action starts one.

use super::decorate::{DaemonState, decorate, open_witch, wanted};
use super::sizing::{grimoire_ratio, sigil_ratio};
use super::store::{Kind, TabKey};
use crate::config::Config;
use crate::herdr::types::Rect;
use crate::herdr::{Dir, HerdrApi, Result};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

/// Requests live in `<state dir>/summon/*.req`, one tab id each.
pub const SUMMON_DIR: &str = "summon";

/// A request older than this is dropped unserved (the daemon was down).
pub const REQUEST_TTL_MS: u64 = 30_000;

/// The "Summon here" action, run inside the daemon: forget the tab's
/// closures and add whatever witch pane is missing.
pub fn summon(
    api: &mut dyn HerdrApi,
    st: &mut DaemonState,
    tab_id: &str,
    cfg: &Config,
) -> Result<()> {
    if let Some(rec) = st.store.tabs.get_mut(&TabKey::from(tab_id)) {
        rec.sigil_closed = false;
        rec.grim_closed = false;
    }
    decorate(api, st, tab_id, cfg, true)
}

/// Add the missing witch panes to a tab whose layout the plain split can't
/// handle, without touching the user's panes beyond the two splits (no
/// `layout.apply`: it recreates the tab and kills its processes). The
/// grimoire splits `down` from the content pane at the tab's bottom-left
/// corner, the sigil splits `right` from the one at the top-right corner,
/// and each new split gets its ratio.
///
/// Known limitation: the grimoire is then only as wide as that corner pane,
/// not the full tab, and `resize` doesn't keep it at 1 row (it only tracks a
/// grimoire that is the root split's second child).
pub(super) fn open_at_corners(
    api: &mut dyn HerdrApi,
    st: &mut DaemonState,
    key: &TabKey,
    content: &[String],
    cfg: &Config,
) -> Result<()> {
    let (want_grim, want_sigil) = wanted(&st.store.tabs[key]);
    if !want_grim && !want_sigil {
        return Ok(());
    }
    let tab = key.0.as_str();
    let before = api.layout(tab)?;
    let a = before.area;
    let rects: Vec<(String, Rect)> = before
        .panes
        .into_iter()
        .filter(|p| content.contains(&p.pane_id))
        .map(|p| (p.pane_id, p.rect))
        .collect();
    // The pane nearest the corner: it touches it whenever a content pane does.
    let bottom_left = rects
        .iter()
        .min_by_key(|(_, r)| ((a.y + a.height).saturating_sub(r.y + r.height), r.x));
    let top_right = rects
        .iter()
        .min_by_key(|(_, r)| ((a.x + a.width).saturating_sub(r.x + r.width), r.y));
    let mut splits = Vec::new();
    if let Some((id, r)) = bottom_left.filter(|_| want_grim) {
        open_witch(api, st, key, Kind::Grimoire, id, Dir::Down)?;
        splits.push((*r, Dir::Down, grimoire_ratio(r.height)));
    }
    if let Some((id, r)) = top_right.filter(|_| want_sigil) {
        open_witch(api, st, key, Kind::Sigil, id, Dir::Right)?;
        splits.push((*r, Dir::Right, sigil_ratio(cfg.sigil_width_pct)));
    }
    if splits.is_empty() {
        return Ok(());
    }
    // Each new split took over its target's rect.
    let after = api.layout(tab)?;
    for (rect, dir, ratio) in splits {
        let path = after
            .splits
            .iter()
            .find(|s| s.rect == rect && s.direction == dir)
            .and_then(|s| split_path(&s.id));
        if let Some(path) = path {
            api.set_split_ratio(tab, &path, ratio)?;
        }
    }
    Ok(())
}

/// herdr's split id `split_<depth>_<path>` as a `set_split_ratio` path:
/// `split_0_root` is `[]`, `split_2_10` is `[true, false]`.
fn split_path(id: &str) -> Option<Vec<bool>> {
    let (_, bits) = id.strip_prefix("split_")?.split_once('_')?;
    if bits == "root" {
        return Some(Vec::new());
    }
    bits.chars()
        .map(|c| match c {
            '0' => Some(false),
            '1' => Some(true),
            _ => None,
        })
        .collect()
}

static SEQ: AtomicU32 = AtomicU32::new(0);

/// Queue a summon of `tab_id` for the daemon (written atomically).
pub fn request(dir: &Path, tab_id: &str, now_ms: u64) -> std::io::Result<()> {
    let d = dir.join(SUMMON_DIR);
    std::fs::create_dir_all(&d)?;
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let name = format!("{now_ms:016}-{}-{seq:06}", std::process::id());
    let tmp = d.join(format!("{name}.tmp"));
    std::fs::write(&tmp, tab_id)?;
    std::fs::rename(&tmp, d.join(format!("{name}.req")))
}

/// Take every queued summon, oldest first; requests older than
/// `REQUEST_TTL_MS` are removed unserved.
pub fn take_requests(dir: &Path, now_ms: u64) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir.join(SUMMON_DIR)) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "req"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| {
            let tab = std::fs::read_to_string(&p).ok();
            let _ = std::fs::remove_file(&p);
            let name = p.file_name()?.to_str()?;
            let at: u64 = name.split('-').next()?.parse().ok()?;
            let tab = tab?.trim().to_string();
            (now_ms.saturating_sub(at) <= REQUEST_TTL_MS && !tab.is_empty()).then_some(tab)
        })
        .collect()
}

/// The tab an action was invoked on: `HERDR_PLUGIN_CONTEXT_JSON.tab_id`,
/// else `HERDR_TAB_ID`.
fn context_tab() -> Option<String> {
    let from_json = std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .ok()
        .and_then(|j| serde_json::from_str::<serde_json::Value>(&j).ok())
        .and_then(|v| v.get("tab_id")?.as_str().map(String::from));
    from_json
        .or_else(|| std::env::var("HERDR_TAB_ID").ok())
        .filter(|t| !t.is_empty())
}

/// `herdr-witch summon`, the action's process: queue the request, and start
/// the daemon if none is running. Returns the exit code.
pub fn action() -> i32 {
    let Some(tab) = context_tab() else {
        eprintln!("herdr-witch summon: no tab_id in HERDR_PLUGIN_CONTEXT_JSON");
        return 2;
    };
    let dir = crate::paths::state_dir();
    if let Err(e) = request(&dir, &tab, crate::ipc::now_ms()) {
        eprintln!("herdr-witch summon: queueing in {}: {e}", dir.display());
        return 1;
    }
    match super::try_lock(&dir.join(super::LOCK_FILE)) {
        Ok(None) => 0, // the daemon serves it within ~100 ms
        Ok(Some(lock)) => {
            drop(lock);
            spawn_daemon()
        }
        Err(e) => {
            eprintln!("herdr-witch summon: lock: {e}");
            1
        }
    }
}

/// Start `herdr-witch daemon` detached (own process group, no stdio); it
/// serves the queued request once connected.
fn spawn_daemon() -> i32 {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let spawned = std::env::current_exe().and_then(|exe| {
        Command::new(exe)
            .arg("daemon")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
    });
    match spawned {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("herdr-witch summon: starting the daemon: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split_path;

    #[test]
    fn split_ids_to_paths() {
        assert_eq!(split_path("split_0_root"), Some(vec![]));
        assert_eq!(split_path("split_1_1"), Some(vec![true]));
        assert_eq!(split_path("split_2_10"), Some(vec![true, false]));
        assert_eq!(split_path("split_2_1x"), None);
        assert_eq!(split_path("pane_1"), None);
    }
}
