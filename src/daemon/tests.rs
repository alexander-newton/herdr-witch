//! Daemon scenarios against `FakeHerdr`.

use super::decorate::{
    DaemonState, GRIM_TITLE, MOVE_GRACE_MS, SIGIL_TITLE, decorate_tab, on_event, reconcile, tick,
};
use super::store::{STORE_FILE, Store, TabKey, temp_dir};
use crate::config::Config;
use crate::grimoire::pool::LinePool;
use crate::herdr::fake::FakeHerdr;
use crate::herdr::{Event, HerdrApi, TabInfo};

pub(super) fn fake() -> FakeHerdr {
    let mut h = FakeHerdr::new();
    // Like the real server: plugin panes carry the manifest pane title.
    h.labels.insert("sigil", SIGIL_TITLE.into());
    h.labels.insert("grimoire", GRIM_TITLE.into());
    h.area = (120, 24);
    h
}

pub(super) fn state(name: &str) -> DaemonState {
    DaemonState::new(temp_dir(name), pool())
}

pub(super) fn pool() -> LinePool {
    LinePool::from_text("one\ntwo\nthree\nfour\nfive\n", 7)
}

pub(super) fn cfg() -> Config {
    Config::default()
}

pub(super) fn info(h: &FakeHerdr, tab: &str) -> TabInfo {
    h.tabs
        .iter()
        .find(|t| t.info.tab_id == tab)
        .unwrap()
        .info
        .clone()
}

/// A tab the user creates while the daemon runs: event, then the grace tick.
pub(super) fn user_tab(
    h: &mut FakeHerdr,
    st: &mut DaemonState,
    label: &str,
    now: u64,
) -> (String, String) {
    let t = h.add_tab("ws", label);
    let root = h.panes(&t).unwrap()[0].pane_id.clone();
    let ev = Event::TabCreated(info(h, &t));
    on_event(h, st, &cfg(), &ev, now).unwrap();
    tick(h, st, &cfg(), now + MOVE_GRACE_MS).unwrap();
    (t, root)
}

pub(super) fn opens(h: &FakeHerdr) -> Vec<&str> {
    h.calls
        .iter()
        .filter(|c| c.starts_with("open_plugin_pane"))
        .map(String::as_str)
        .collect()
}

pub(super) fn witch(h: &mut FakeHerdr, tab: &str, title: &str) -> Option<String> {
    h.panes(tab)
        .unwrap()
        .into_iter()
        .find(|p| p.label.as_deref() == Some(title))
        .map(|p| p.pane_id)
}

pub(super) fn closed(h: &mut FakeHerdr, st: &mut DaemonState, pane: &str) {
    h.close_pane(pane);
    let ev = Event::PaneClosed {
        pane_id: pane.into(),
        tab_id: None,
    };
    on_event(h, st, &cfg(), &ev, 0).unwrap();
}

fn layout_event(h: &mut FakeHerdr, st: &mut DaemonState, tab: &str) {
    let ev = Event::LayoutUpdated { tab_id: tab.into() };
    on_event(h, st, &cfg(), &ev, 0).unwrap();
}

#[test]
fn auto_open_false_skips() {
    let mut h = fake();
    let mut st = state("noauto");
    let t = h.add_tab("ws", "a");
    let off = Config {
        auto_open: false,
        ..Config::default()
    };
    let ev = Event::TabCreated(info(&h, &t));
    on_event(&mut h, &mut st, &off, &ev, 0).unwrap();
    tick(&mut h, &mut st, &off, 10_000).unwrap();
    assert!(opens(&h).is_empty(), "{:?}", h.calls);
    assert!(st.store.tabs.is_empty());
}

#[test]
fn new_tab_gets_both_panes() {
    let mut h = fake();
    let mut st = state("both");
    let t = h.add_tab("ws", "a");
    let root = h.panes(&t).unwrap()[0].pane_id.clone();
    let ev = Event::TabCreated(info(&h, &t));
    on_event(&mut h, &mut st, &cfg(), &ev, 1000).unwrap();
    tick(&mut h, &mut st, &cfg(), 1000 + MOVE_GRACE_MS - 1).unwrap();
    assert!(opens(&h).is_empty(), "waits for a possible pane_moved");
    h.calls.clear();
    tick(&mut h, &mut st, &cfg(), 1000 + MOVE_GRACE_MS).unwrap();

    let rec = st.store.tabs[&TabKey::from(t.as_str())].clone();
    assert!(rec.line < 5);
    let calls: Vec<&str> = h
        .calls
        .iter()
        .filter(|c| !c.starts_with("panes") && !c.starts_with("layout"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        calls,
        [
            format!(
                "open_plugin_pane grimoire {root} down HERDR_WITCH_LINE={}",
                rec.line
            ),
            format!("open_plugin_pane sigil {root} right "),
            format!("set_split_ratio {t} [] 0.875"),
            format!("set_split_ratio {t} [false] 0.85"),
        ]
    );
    // Focus never moved; the grimoire is 3 rows (1 content row), full width
    // at the bottom; the sigil is 15% of the width.
    assert_eq!(h.focused.as_deref(), Some(root.as_str()));
    let (s, g) = (rec.sigil_pane.unwrap(), rec.grim_pane.unwrap());
    let l = h.layout(&t).unwrap();
    let rect = |id: &str| l.panes.iter().find(|p| p.pane_id == id).unwrap().rect;
    assert_eq!((rect(&g).y, rect(&g).height, rect(&g).width), (21, 3, 120));
    assert_eq!((rect(&s).width, rect(&s).height), (18, 21));
    assert!(st.ours.contains(&s) && st.ours.contains(&g));
    // Persisted.
    assert!(st.dir.join(STORE_FILE).exists());
    assert_eq!(Store::load(&st.dir), st.store);
}

#[test]
fn tab_created_twice_decorates_once() {
    let mut h = fake();
    let mut st = state("twice");
    let t = h.add_tab("ws", "a");
    let ev = Event::TabCreated(info(&h, &t));
    on_event(&mut h, &mut st, &cfg(), &ev, 0).unwrap();
    on_event(&mut h, &mut st, &cfg(), &ev, 50).unwrap();
    tick(&mut h, &mut st, &cfg(), 500).unwrap();
    on_event(&mut h, &mut st, &cfg(), &ev, 600).unwrap();
    tick(&mut h, &mut st, &cfg(), 1100).unwrap();
    reconcile(&mut h, &mut st, &cfg()).unwrap();
    decorate_tab(&mut h, &mut st, &t, &cfg()).unwrap();
    assert_eq!(opens(&h).len(), 2, "{:?}", h.calls);
    assert_eq!(h.panes(&t).unwrap().len(), 3);
}

#[test]
fn closed_pane_never_reopened() {
    let mut h = fake();
    let mut st = state("closed");
    let (t, _) = user_tab(&mut h, &mut st, "a", 0);
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    closed(&mut h, &mut st, &s);
    let rec = &st.store.tabs[&TabKey::from(t.as_str())];
    assert!(rec.sigil_closed && rec.sigil_pane.is_none());
    assert!(!st.ours.contains(&s));
    assert!(Store::load(&st.dir).tabs[&TabKey::from(t.as_str())].sigil_closed);

    h.calls.clear();
    let ev = Event::TabCreated(info(&h, &t));
    on_event(&mut h, &mut st, &cfg(), &ev, 5000).unwrap();
    tick(&mut h, &mut st, &cfg(), 6000).unwrap();
    reconcile(&mut h, &mut st, &cfg()).unwrap();
    assert!(opens(&h).is_empty(), "{:?}", h.calls);
    assert_eq!(witch(&mut h, &t, SIGIL_TITLE), None);

    // The grimoire closes too; the record stays, with both closed.
    let g = witch(&mut h, &t, GRIM_TITLE).unwrap();
    closed(&mut h, &mut st, &g);
    reconcile(&mut h, &mut st, &cfg()).unwrap();
    assert!(opens(&h).is_empty());
    assert!(st.store.tabs[&TabKey::from(t.as_str())].grim_closed);
}

/// herdr restarts and restores the tab (same id) without the plugin panes.
#[test]
fn closure_survives_restart() {
    let dir = temp_dir("restart");
    let mut h = fake();
    let mut st = DaemonState::new(dir.clone(), pool());
    let (t, _) = user_tab(&mut h, &mut st, "a", 0);
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    closed(&mut h, &mut st, &s);
    drop(st);

    let mut h2 = fake();
    assert_eq!(h2.add_tab("ws", "a"), t);
    let root = h2.panes(&t).unwrap()[0].pane_id.clone();
    let mut st2 = DaemonState::new(dir, pool());
    reconcile(&mut h2, &mut st2, &cfg()).unwrap();
    assert_eq!(opens(&h2).len(), 1, "{:?}", h2.calls);
    assert!(opens(&h2)[0].starts_with(&format!("open_plugin_pane grimoire {root} down")));
    assert_eq!(witch(&mut h2, &t, SIGIL_TITLE), None);
    let rec = &st2.store.tabs[&TabKey::from(t.as_str())];
    assert!(rec.sigil_closed && rec.grim_pane.is_some());
}

#[test]
fn line_kept_across_restart() {
    let dir = temp_dir("line");
    let mut h = fake();
    let mut st = DaemonState::new(dir.clone(), pool());
    let (t, _) = user_tab(&mut h, &mut st, "a", 0);
    let (t2, _) = user_tab(&mut h, &mut st, "b", 1000);
    let line = st.store.tabs[&TabKey::from(t.as_str())].line;
    assert_ne!(line, st.store.tabs[&TabKey::from(t2.as_str())].line);
    drop(st);

    // Restored without plugin panes: the grimoire comes back with its line.
    let mut h2 = fake();
    h2.add_tab("ws", "a");
    let mut st2 = DaemonState::new(
        dir.clone(),
        LinePool::from_text("one\ntwo\nthree\nfour\nfive\n", 99),
    );
    reconcile(&mut h2, &mut st2, &cfg()).unwrap();
    let want = format!("HERDR_WITCH_LINE={line}");
    assert!(
        opens(&h2)
            .iter()
            .any(|c| c.contains("grimoire") && c.ends_with(&want)),
        "{:?}",
        h2.calls
    );

    // Restored with its plugin panes (what herdr 0.9.5 does): re-adopted.
    let mut h3 = fake();
    let t3 = h3.add_tab("ws", "a");
    let root = h3.panes(&t3).unwrap()[0].pane_id.clone();
    for (ep, d) in [
        ("grimoire", crate::herdr::Dir::Down),
        ("sigil", crate::herdr::Dir::Right),
    ] {
        h3.open_plugin_pane(crate::herdr::OpenPane {
            entrypoint: ep,
            target_pane_id: root.clone(),
            direction: d,
            env: vec![],
        })
        .unwrap();
    }
    h3.calls.clear();
    let mut st3 = DaemonState::new(dir, pool());
    reconcile(&mut h3, &mut st3, &cfg()).unwrap();
    assert!(opens(&h3).is_empty(), "{:?}", h3.calls);
    let rec = &st3.store.tabs[&TabKey::from(t3.as_str())];
    assert_eq!(rec.line, line);
    assert_eq!(rec.grim_pane, witch(&mut h3, &t3, GRIM_TITLE));
    assert_eq!(rec.sigil_pane, witch(&mut h3, &t3, SIGIL_TITLE));
    assert!(st3.ours.contains(rec.sigil_pane.as_ref().unwrap()));
}

#[test]
fn moved_pane_tab_not_decorated() {
    let mut h = fake();
    let mut st = state("moved");
    let (a, _) = user_tab(&mut h, &mut st, "a", 0);
    let s = witch(&mut h, &a, SIGIL_TITLE).unwrap();
    h.calls.clear();
    // The user moves tab a's sigil into a new tab: tab_created comes first.
    h.close_pane(&s);
    let b = h.add_tab("ws", "b");
    let moved = h.panes(&b).unwrap()[0].pane_id.clone();
    let ev = Event::TabCreated(info(&h, &b));
    on_event(&mut h, &mut st, &cfg(), &ev, 1000).unwrap();
    let ev = Event::PaneMoved {
        pane_id: moved.clone(),
        previous_pane_id: s.clone(),
        created_tab: Some(b.clone()),
    };
    on_event(&mut h, &mut st, &cfg(), &ev, 1010).unwrap();
    tick(&mut h, &mut st, &cfg(), 5000).unwrap();
    reconcile(&mut h, &mut st, &cfg()).unwrap();
    assert!(opens(&h).is_empty(), "{:?}", h.calls);
    assert!(!st.store.tabs.contains_key(&TabKey::from(b.as_str())));
    // Tab a lost its sigil to the move: it isn't reopened there.
    assert!(st.store.tabs[&TabKey::from(a.as_str())].sigil_closed);
    assert!(st.ours.contains(&moved) && !st.ours.contains(&s));
}

#[test]
fn preexisting_tabs_ignored_on_first_install() {
    let mut h = fake();
    let mut st = state("preexisting");
    h.add_tab("ws", "old1");
    h.add_tab("other", "old2");
    reconcile(&mut h, &mut st, &cfg()).unwrap();
    assert!(opens(&h).is_empty(), "{:?}", h.calls);
    assert!(st.store.tabs.is_empty());
    // A tab made while the daemon was disconnected is new, not pre-existing.
    let t = h.add_tab("ws", "new");
    reconcile(&mut h, &mut st, &cfg()).unwrap();
    assert_eq!(opens(&h).len(), 2);
    assert!(st.store.tabs.contains_key(&TabKey::from(t.as_str())));
}

#[test]
fn multi_pane_tab_left_for_summon() {
    let mut h = fake();
    let mut st = state("multi");
    let t = h.add_tab("ws", "a");
    let root = h.panes(&t).unwrap()[0].pane_id.clone();
    h.open_plugin_pane(crate::herdr::OpenPane {
        entrypoint: "user-pane",
        target_pane_id: root,
        direction: crate::herdr::Dir::Right,
        env: vec![],
    })
    .unwrap();
    h.calls.clear();
    decorate_tab(&mut h, &mut st, &t, &cfg()).unwrap();
    assert!(opens(&h).is_empty());
    assert!(!h.calls.iter().any(|c| c.starts_with("set_split_ratio")));
    // Left alone: no record, no grimoire line used up.
    assert!(st.store.tabs.is_empty());
    assert_eq!(st.pool.next, 0);
}

#[test]
fn tab_closed_forgets_record() {
    let mut h = fake();
    let mut st = state("tabclosed");
    let (t, _) = user_tab(&mut h, &mut st, "a", 0);
    on_event(
        &mut h,
        &mut st,
        &cfg(),
        &Event::TabClosed { tab_id: t.clone() },
        0,
    )
    .unwrap();
    assert!(st.store.tabs.is_empty());
    assert!(Store::load(&st.dir).tabs.is_empty());
}

#[test]
fn drag_marks_user_sized() {
    let mut h = fake();
    let mut st = state("drag");
    let (t, _) = user_tab(&mut h, &mut st, "a", 0);
    layout_event(&mut h, &mut st, &t);
    assert!(!st.store.tabs[&TabKey::from(t.as_str())].user_sized);
    // The user drags the grimoire split.
    h.set_split_ratio(&t, &[], 0.6).unwrap();
    layout_event(&mut h, &mut st, &t);
    assert!(st.store.tabs[&TabKey::from(t.as_str())].user_sized);
    assert!(Store::load(&st.dir).tabs[&TabKey::from(t.as_str())].user_sized);
    // From then on a window resize leaves the tab alone.
    h.calls.clear();
    h.area = (100, 20);
    layout_event(&mut h, &mut st, &t);
    assert!(
        !h.calls.iter().any(|c| c.starts_with("set_split_ratio")),
        "{:?}",
        h.calls
    );
}

#[test]
fn window_resize_reapplies() {
    let mut h = fake();
    let mut st = state("resize");
    let (t, _) = user_tab(&mut h, &mut st, "a", 0);
    let g = witch(&mut h, &t, GRIM_TITLE).unwrap();
    h.calls.clear();
    h.area = (100, 20);
    layout_event(&mut h, &mut st, &t);
    assert!(
        h.calls.contains(&format!("set_split_ratio {t} [] 0.85")),
        "{:?}",
        h.calls
    );
    let l = h.layout(&t).unwrap();
    assert_eq!(
        l.panes.iter().find(|p| p.pane_id == g).unwrap().rect.height,
        3
    );
    // The layout event our own resize triggers changes nothing.
    h.calls.clear();
    layout_event(&mut h, &mut st, &t);
    assert!(!h.calls.iter().any(|c| c.starts_with("set_split_ratio")));
    assert!(!st.store.tabs[&TabKey::from(t.as_str())].user_sized);
    // A tall window: herdr's 0.9 clamp gives 4 rows, which is not a drag.
    h.area = (100, 40);
    layout_event(&mut h, &mut st, &t);
    layout_event(&mut h, &mut st, &t);
    assert!(!st.store.tabs[&TabKey::from(t.as_str())].user_sized);
}

#[test]
fn pane_updated_does_not_change_typing() {
    // herdr 0.9.5 never bumps revisions on output; `TypingDetector::poll`
    // is the only typing source, so events must not feed it.
    let mut h = fake();
    let mut st = state("typing");
    user_tab(&mut h, &mut st, "a", 0);
    let p = h.focused_pane().unwrap().unwrap();
    for (rev, now) in [(1, 100), (2, 200), (3, 300)] {
        let line = format!(
            r#"{{"event":"pane_updated","data":{{"pane":{{"pane_id":"{}","focused":true,"revision":{rev}}}}}}}"#,
            p.pane_id
        );
        let ev = Event::parse(&line).unwrap();
        assert_eq!(ev, Event::Other);
        on_event(&mut h, &mut st, &cfg(), &ev, now).unwrap();
    }
    assert!(!st.typing.typing(300));
    assert!(st.typing.last_rev.is_none());
}

#[test]
fn rearranged_grimoire_left_alone() {
    use crate::herdr::Dir;
    use crate::herdr::fake::Node;
    let mut h = fake();
    let mut st = state("rearranged");
    let (t, _) = user_tab(&mut h, &mut st, "a", 0);
    let g = witch(&mut h, &t, GRIM_TITLE).unwrap();
    // The user put a full-width pane between the content and the grimoire:
    // the grimoire is still at the bottom but no longer the root's child.
    let tab = h.tabs.iter_mut().find(|x| x.info.tab_id == t).unwrap();
    let Node::Split { second, .. } = &mut tab.root else {
        panic!()
    };
    **second = Node::Split {
        dir: Dir::Down,
        ratio: 0.5,
        first: Box::new(Node::Pane("w1:p99".into())),
        second: Box::new(Node::Pane(g)),
    };
    h.calls.clear();
    h.area = (100, 20);
    layout_event(&mut h, &mut st, &t);
    assert!(
        !h.calls.iter().any(|c| c.starts_with("set_split_ratio")),
        "{:?}",
        h.calls
    );
}

#[test]
fn relabelled_witch_pane_still_adopted() {
    let mut h = fake();
    let mut st = state("relabel");
    let (t, _) = user_tab(&mut h, &mut st, "a", 0);
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    let g = witch(&mut h, &t, GRIM_TITLE).unwrap();
    // The user renamed both witch panes.
    for p in h
        .panes
        .iter_mut()
        .filter(|p| p.pane_id == s || p.pane_id == g)
    {
        p.label = Some("renamed".into());
    }
    h.calls.clear();
    reconcile(&mut h, &mut st, &cfg()).unwrap();
    decorate_tab(&mut h, &mut st, &t, &cfg()).unwrap();
    assert!(opens(&h).is_empty(), "{:?}", h.calls);
    let rec = &st.store.tabs[&TabKey::from(t.as_str())];
    assert_eq!(rec.sigil_pane.as_deref(), Some(s.as_str()));
    assert_eq!(rec.grim_pane.as_deref(), Some(g.as_str()));
    assert!(st.ours.contains(&s) && st.ours.contains(&g));
}

#[test]
fn vanished_pane_pruned_from_ours() {
    let mut h = fake();
    let mut st = state("prune");
    let (t, _) = user_tab(&mut h, &mut st, "a", 0);
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    // Gone without a pane_closed event (e.g. herdr restored the tab only).
    h.close_pane(&s);
    reconcile(&mut h, &mut st, &cfg()).unwrap();
    assert!(!st.ours.contains(&s));
    let s2 = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    assert_ne!(s2, s);
    assert!(st.ours.contains(&s2));
}

fn io_error() -> crate::herdr::HerdrError {
    crate::herdr::HerdrError::Io(std::io::ErrorKind::BrokenPipe.into())
}

fn remote_error() -> crate::herdr::HerdrError {
    crate::herdr::HerdrError::Remote {
        code: "pane_not_found".into(),
        message: "gone".into(),
    }
}

#[test]
fn on_event_tolerates_herdr_races() {
    let mut h = fake();
    let mut st = state("races");
    let (t, _) = user_tab(&mut h, &mut st, "a", 0);
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    // A layout event for a tab that closed before we asked: tab_not_found.
    let gone = h.add_tab("ws", "gone");
    decorate_tab(&mut h, &mut st, &gone, &cfg()).unwrap();
    let root = h.panes(&gone).unwrap()[0].pane_id.clone();
    for p in h.panes(&gone).unwrap() {
        if p.pane_id != root {
            h.close_pane(&p.pane_id);
        }
    }
    h.close_pane(&root);
    let ev = Event::LayoutUpdated {
        tab_id: gone.clone(),
    };
    on_event(&mut h, &mut st, &cfg(), &ev, 0).unwrap();
    // A pane move whose tab lookup races a close.
    h.fail.insert(format!("panes {t}"), remote_error);
    let ev = Event::PaneMoved {
        pane_id: "w1:p50".into(),
        previous_pane_id: s.clone(),
        created_tab: None,
    };
    on_event(&mut h, &mut st, &cfg(), &ev, 0).unwrap();
    // A socket error still propagates, so the runtime can reconnect.
    h.fail.insert(format!("layout {t}"), io_error);
    let ev = Event::LayoutUpdated { tab_id: t.clone() };
    assert!(matches!(
        on_event(&mut h, &mut st, &cfg(), &ev, 0),
        Err(crate::herdr::HerdrError::Io(_))
    ));
}

/// herdr closes a tab when its last pane goes, but not one left holding
/// only plugin panes (probed on 0.9.5). The daemon closes the witch panes
/// then, so herdr closes the tab, without recording closures or respawning.
fn assert_tab_closed_by_daemon(h: &FakeHerdr, st: &DaemonState, t: &str, witches: &[String]) {
    let removes: Vec<&str> = h
        .calls
        .iter()
        .filter(|c| c.starts_with("remove_pane"))
        .map(String::as_str)
        .collect();
    let want: Vec<String> = witches.iter().map(|w| format!("remove_pane {w}")).collect();
    assert_eq!(removes, want, "{:?}", h.calls);
    assert!(opens(h).is_empty(), "nothing re-added: {:?}", h.calls);
    assert!(h.tree(t).is_none(), "herdr closed the emptied tab");
    assert!(!st.store.tabs.contains_key(&TabKey::from(t)));
    assert!(!Store::load(&st.dir).tabs.contains_key(&TabKey::from(t)));
    assert!(witches.iter().all(|w| !st.ours.contains(w)));
}

#[test]
fn last_content_pane_exiting_closes_the_tab() {
    let mut h = fake();
    let mut st = state("orphan-exit");
    let (t, root) = user_tab(&mut h, &mut st, "a", 0);
    let (s, g) = (
        witch(&mut h, &t, SIGIL_TITLE).unwrap(),
        witch(&mut h, &t, GRIM_TITLE).unwrap(),
    );
    let (other, _) = user_tab(&mut h, &mut st, "b", 0);
    h.calls.clear();
    h.close_pane(&root); // the shell exits
    let ev = Event::PaneExited {
        pane_id: root.clone(),
    };
    on_event(&mut h, &mut st, &cfg(), &ev, 50_000).unwrap();
    assert_tab_closed_by_daemon(&h, &st, &t, &[s.clone(), g.clone()]);
    assert!(st.store.tabs.contains_key(&TabKey::from(other.as_str())));
    // herdr then reports the closes and the tab: nothing more happens.
    h.calls.clear();
    for p in [&s, &g] {
        let ev = Event::PaneClosed {
            pane_id: p.clone(),
            tab_id: None,
        };
        on_event(&mut h, &mut st, &cfg(), &ev, 50_100).unwrap();
    }
    let ev = Event::TabClosed { tab_id: t.clone() };
    on_event(&mut h, &mut st, &cfg(), &ev, 50_100).unwrap();
    assert!(opens(&h).is_empty() && !h.calls.iter().any(|c| c.starts_with("remove")));
}

#[test]
fn last_content_pane_closed_closes_the_tab() {
    let mut h = fake();
    let mut st = state("orphan-close");
    let (t, root) = user_tab(&mut h, &mut st, "a", 0);
    let (s, g) = (
        witch(&mut h, &t, SIGIL_TITLE).unwrap(),
        witch(&mut h, &t, GRIM_TITLE).unwrap(),
    );
    h.calls.clear();
    closed(&mut h, &mut st, &root);
    assert_tab_closed_by_daemon(&h, &st, &t, &[s, g]);
}

#[test]
fn last_content_pane_moved_out_closes_the_tab() {
    let mut h = fake();
    let mut st = state("orphan-move");
    let (t, root) = user_tab(&mut h, &mut st, "a", 0);
    let (s, g) = (
        witch(&mut h, &t, SIGIL_TITLE).unwrap(),
        witch(&mut h, &t, GRIM_TITLE).unwrap(),
    );
    h.calls.clear();
    h.close_pane(&root); // gone from this tab...
    let dest = h.add_tab("ws", "moved"); // ...into a tab of its own
    let ev = Event::PaneMoved {
        pane_id: root.clone(),
        previous_pane_id: root.clone(),
        created_tab: Some(dest),
    };
    on_event(&mut h, &mut st, &cfg(), &ev, 0).unwrap();
    assert_tab_closed_by_daemon(&h, &st, &t, &[s, g]);
}

#[test]
fn tab_with_content_left_is_kept() {
    let mut h = fake();
    let mut st = state("orphan-keep");
    let (t, root) = user_tab(&mut h, &mut st, "a", 0);
    let other = h
        .open_plugin_pane(crate::herdr::OpenPane {
            entrypoint: "user-pane",
            target_pane_id: root.clone(),
            direction: crate::herdr::Dir::Right,
            env: vec![],
        })
        .unwrap();
    h.calls.clear();
    closed(&mut h, &mut st, &root);
    assert!(!h.calls.iter().any(|c| c.starts_with("remove_pane")));
    assert!(h.tree(&t).is_some());
    assert!(st.store.tabs.contains_key(&TabKey::from(t.as_str())));
    let _ = other;
}

#[test]
fn stale_record_ids_do_not_make_user_panes_orphans() {
    use super::store::Kind;
    let mut h = fake();
    let mut st = state("orphan-stale");
    let (t, root) = user_tab(&mut h, &mut st, "a", 0);
    let other = h
        .open_plugin_pane(crate::herdr::OpenPane {
            entrypoint: "user-pane",
            target_pane_id: root.clone(),
            direction: crate::herdr::Dir::Right,
            env: vec![],
        })
        .unwrap();
    // The witch panes are gone; the record's ids went stale and now name
    // the user's panes, which are unlabelled and not ours.
    for kind in [SIGIL_TITLE, GRIM_TITLE] {
        let w = witch(&mut h, &t, kind).unwrap();
        h.close_pane(&w);
    }
    st.ours.clear();
    let rec = st.store.tabs.get_mut(&TabKey::from(t.as_str())).unwrap();
    rec.set_pane(Kind::Sigil, Some(root.clone()));
    rec.set_pane(Kind::Grimoire, Some(other.clone()));
    h.calls.clear();
    let ev = Event::PaneExited {
        pane_id: "w9:p9".into(),
    };
    on_event(&mut h, &mut st, &cfg(), &ev, 0).unwrap();
    assert!(!h.calls.iter().any(|c| c.starts_with("remove_pane")));
    assert!(h.tree(&t).is_some());
    assert!(st.store.tabs.contains_key(&TabKey::from(t.as_str())));
}

#[test]
fn orphan_with_one_witch_pane_left() {
    let mut h = fake();
    let mut st = state("orphan-one");
    let (t, root) = user_tab(&mut h, &mut st, "a", 0);
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    let g = witch(&mut h, &t, GRIM_TITLE).unwrap();
    closed(&mut h, &mut st, &s); // the user closed the sigil earlier
    h.calls.clear();
    closed(&mut h, &mut st, &root);
    assert_tab_closed_by_daemon(&h, &st, &t, &[g]);
}

#[test]
fn witch_crash_in_orphan_tab_is_not_respawned() {
    let mut h = fake();
    let mut st = state("orphan-crash");
    let (t, root) = user_tab(&mut h, &mut st, "a", 0);
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    let g = witch(&mut h, &t, GRIM_TITLE).unwrap();
    // The daemon missed the content pane going (it was down): a later
    // witch-pane crash still closes the tab instead of re-adding the pane.
    h.close_pane(&root);
    h.close_pane(&s);
    h.calls.clear();
    let ev = Event::PaneExited { pane_id: s };
    on_event(&mut h, &mut st, &cfg(), &ev, 50_000).unwrap();
    assert_tab_closed_by_daemon(&h, &st, &t, &[g]);
}
