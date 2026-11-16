//! Scenarios for the singleton lock, "Summon here", respawning, restored
//! panes and the runtime loop, against `FakeHerdr`.

use super::decorate::{DaemonState, GRIM_TITLE, MOVE_GRACE_MS, SIGIL_TITLE, on_event, reconcile};
use super::store::{Store, TabKey, temp_dir};
use super::summon::{request, summon, take_requests};
use super::tests::{cfg, closed, fake, info, opens, pool, state, user_tab, witch};
use super::{Runtime, try_lock};
use crate::herdr::fake::FakeHerdr;
use crate::herdr::fake::Node;
use crate::herdr::{AgentInfo, AgentStatus, Dir, OpenPane};
use crate::herdr::{Event, HerdrApi};

fn leaf(id: &str) -> Box<Node> {
    Box::new(Node::Pane(id.into()))
}

fn split(dir: Dir, ratio: f32, first: Box<Node>, second: Box<Node>) -> Box<Node> {
    Box::new(Node::Split {
        dir,
        ratio,
        first,
        second,
    })
}

/// Split `target` with a pane that stands in for one the user made.
fn user_split(h: &mut FakeHerdr, target: &str, direction: Dir) -> String {
    h.open_plugin_pane(OpenPane {
        entrypoint: "user-pane",
        target_pane_id: target.into(),
        direction,
        env: vec![],
    })
    .unwrap()
}

/// Calls that change something (no `panes` / `layout` / `pane_argv` reads).
fn mutations(h: &FakeHerdr) -> Vec<&str> {
    h.calls
        .iter()
        .filter(|c| {
            !["panes", "layout", "pane_argv"]
                .iter()
                .any(|r| c.starts_with(r))
        })
        .map(String::as_str)
        .collect()
}

#[test]
fn lock_is_exclusive() {
    let path = temp_dir("lock").join("daemon.lock");
    let first = try_lock(&path).unwrap();
    assert!(first.is_some(), "the first lock is taken");
    assert!(try_lock(&path).unwrap().is_none(), "a second lock fails");
    drop(first);
    assert!(
        try_lock(&path).unwrap().is_some(),
        "free again once dropped"
    );
}

/// Ruling S: no `layout.apply` (it recreates the tab and kills the user's
/// processes). The grimoire splits the content pane at the bottom-left
/// corner, the sigil the one at the top-right.
#[test]
fn summon_multi_pane_splits_corner_panes() {
    let mut h = fake();
    let mut st = state("summon-multi");
    let t = h.add_tab("ws", "a");
    let a = h.panes(&t).unwrap()[0].pane_id.clone();
    let b = user_split(&mut h, &a, Dir::Right);
    let c = user_split(&mut h, &b, Dir::Down);
    // a | b/c on 120x24: a touches bottom-left, b top-right.
    h.calls.clear();
    summon(&mut h, &mut st, &t, &cfg()).unwrap();

    let rec = st.store.tabs[&TabKey::from(t.as_str())].clone();
    let (s, g) = (
        rec.sigil_pane.clone().unwrap(),
        rec.grim_pane.clone().unwrap(),
    );
    assert_eq!(
        mutations(&h),
        [
            format!(
                "open_plugin_pane grimoire {a} down HERDR_WITCH_LINE={}",
                rec.line
            ),
            format!("open_plugin_pane sigil {b} right "),
            format!("set_split_ratio {t} [false] 0.875"),
            format!("set_split_ratio {t} [true, false] 0.85"),
        ]
    );
    // The user's panes keep their places: only their own leaves were split.
    let want = split(
        Dir::Right,
        0.5,
        split(Dir::Down, 0.875, leaf(&a), leaf(&g)),
        split(
            Dir::Down,
            0.5,
            split(Dir::Right, 0.85, leaf(&b), leaf(&s)),
            leaf(&c),
        ),
    );
    assert_eq!(h.tree(&t), Some(want.as_ref()));
    assert_eq!(h.focused.as_deref(), Some(a.as_str()), "focus unchanged");
    assert!(!rec.sigil_closed && !rec.grim_closed);
    assert!(st.ours.contains(&s) && st.ours.contains(&g));
    assert_eq!(Store::load(&st.dir), st.store);
}

#[test]
fn summon_clears_closed_flags() {
    let mut h = fake();
    let mut st = state("summon-flags");
    let (t, root) = user_tab(&mut h, &mut st, "a", 0);
    let key = TabKey::from(t.as_str());
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    let g = witch(&mut h, &t, GRIM_TITLE).unwrap();
    closed(&mut h, &mut st, &s);
    closed(&mut h, &mut st, &g);
    assert!(st.store.tabs[&key].sigil_closed && st.store.tabs[&key].grim_closed);
    let line = st.store.tabs[&key].line;
    h.calls.clear();
    summon(&mut h, &mut st, &t, &cfg()).unwrap();

    let rec = &st.store.tabs[&key];
    assert!(!rec.sigil_closed && !rec.grim_closed);
    assert_eq!(rec.line, line, "the tab keeps its incantation");
    // A lone content pane gets the usual full layout.
    assert_eq!(
        mutations(&h),
        [
            format!("open_plugin_pane grimoire {root} down HERDR_WITCH_LINE={line}"),
            format!("open_plugin_pane sigil {root} right "),
            format!("set_split_ratio {t} [] 0.875"),
            format!("set_split_ratio {t} [false] 0.85"),
        ]
    );
    let saved = &Store::load(&st.dir).tabs[&key];
    assert!(!saved.sigil_closed && !saved.grim_closed);
    assert!(saved.sigil_pane.is_some() && saved.grim_pane.is_some());
}

#[test]
fn summon_grimoire_under_existing_sigil() {
    let mut h = fake();
    let mut st = state("summon-grim");
    let (t, root) = user_tab(&mut h, &mut st, "a", 0);
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    let g = witch(&mut h, &t, GRIM_TITLE).unwrap();
    closed(&mut h, &mut st, &g);
    h.calls.clear();
    summon(&mut h, &mut st, &t, &cfg()).unwrap();

    let g2 = st.store.tabs[&TabKey::from(t.as_str())]
        .grim_pane
        .clone()
        .unwrap();
    assert_eq!(
        mutations(&h),
        [
            format!(
                "open_plugin_pane grimoire {root} down HERDR_WITCH_LINE={}",
                st.store.tabs[&TabKey::from(t.as_str())].line
            ),
            format!("set_split_ratio {t} [false] 0.875"),
        ]
    );
    let want = split(
        Dir::Right,
        0.85,
        split(Dir::Down, 0.875, leaf(&root), leaf(&g2)),
        leaf(&s),
    );
    assert_eq!(h.tree(&t), Some(want.as_ref()));
}

#[test]
fn summon_preexisting_tab_gets_record() {
    let mut h = fake();
    let mut st = state("summon-pre");
    let t = h.add_tab("ws", "old");
    reconcile(&mut h, &mut st, &cfg()).unwrap(); // baseline: left alone
    assert!(opens(&h).is_empty());
    summon(&mut h, &mut st, &t, &cfg()).unwrap();
    assert_eq!(opens(&h).len(), 2, "{:?}", h.calls);
    assert!(st.store.tabs.contains_key(&TabKey::from(t.as_str())));
    // Unknown tab: nothing happens.
    summon(&mut h, &mut st, "w9:t9", &cfg()).unwrap();
    assert_eq!(opens(&h).len(), 2);
}

/// A witch pane whose process died (`pane_exited`, no `pane_closed`) is
/// re-added, not marked closed; a crash loop is throttled.
#[test]
fn exited_witch_pane_respawns() {
    let mut h = fake();
    let mut st = state("respawn");
    let (t, root) = user_tab(&mut h, &mut st, "a", 0);
    let key = TabKey::from(t.as_str());
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    let exited = |h: &mut FakeHerdr, st: &mut DaemonState, pane: &str, now| {
        h.close_pane(pane); // herdr removes the pane when its process ends
        let ev = Event::PaneExited {
            pane_id: pane.into(),
        };
        on_event(h, st, &cfg(), &ev, now).unwrap();
    };
    h.calls.clear();
    exited(&mut h, &mut st, &s, 10_000);
    let s2 = st.store.tabs[&key].sigil_pane.clone().expect("re-added");
    assert_ne!(s2, s);
    assert!(!st.store.tabs[&key].sigil_closed);
    assert!(!st.ours.contains(&s) && st.ours.contains(&s2));
    assert_eq!(opens(&h), [format!("open_plugin_pane sigil {root} right ")]);

    // Dies again within the cooldown: left missing (not closed) for now.
    h.calls.clear();
    exited(&mut h, &mut st, &s2, 12_000);
    assert!(opens(&h).is_empty(), "{:?}", h.calls);
    let rec = &st.store.tabs[&key];
    assert!(rec.sigil_pane.is_none() && !rec.sigil_closed);

    // After the cooldown both missing kinds come back.
    let g = witch(&mut h, &t, GRIM_TITLE).unwrap();
    exited(&mut h, &mut st, &g, 25_000);
    assert_eq!(opens(&h).len(), 2, "{:?}", h.calls);
    let rec = &st.store.tabs[&key];
    assert!(rec.sigil_pane.is_some() && rec.grim_pane.is_some());

    // A user close is still a closure.
    let s3 = rec.sigil_pane.clone().unwrap();
    closed(&mut h, &mut st, &s3);
    assert!(st.store.tabs[&key].sigil_closed);
}

#[test]
fn summon_requests_queue() {
    let dir = temp_dir("requests");
    request(&dir, "w1:t1", 1000).unwrap();
    request(&dir, "w1:t2", 1000).unwrap();
    request(&dir, "w1:t3", 1001).unwrap();
    assert_eq!(take_requests(&dir, 2000), ["w1:t1", "w1:t2", "w1:t3"]);
    assert!(take_requests(&dir, 2000).is_empty(), "taken once");
    // A daemon still starting up (backoff, a 2 s settle) serves it 25 s on.
    request(&dir, "w1:t5", 0).unwrap();
    assert_eq!(take_requests(&dir, 25_000), ["w1:t5"]);
    request(&dir, "w1:t4", 0).unwrap();
    assert!(
        take_requests(&dir, 30_001).is_empty(),
        "stale requests dropped after 30 s"
    );
    assert!(take_requests(&temp_dir("requests-none"), 0).is_empty());
}

#[test]
fn runtime_step_publishes_state_and_serves_summons() {
    let mut h = fake();
    let st = state("runtime");
    let run_dir = temp_dir("runtime-run");
    let mut rt = Runtime::new(st, cfg(), run_dir.clone());
    let old = h.add_tab("ws", "old");
    let root = h.panes(&old).unwrap()[0].pane_id.clone();
    reconcile(&mut h, &mut rt.st, &cfg()).unwrap();
    h.agents = [
        AgentStatus::Working,
        AgentStatus::Working,
        AgentStatus::Idle,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, s)| AgentInfo {
        pane_id: format!("w1:p{}", 90 + i),
        agent_status: s,
        ..AgentInfo::default()
    })
    .collect();
    h.texts.insert(root.clone(), "$ ".into());
    rt.step(&mut h, &[], 1_000).unwrap();
    let s = crate::ipc::read_state(&run_dir, 1_000);
    assert_eq!((s.working, s.typing), (2, false));

    // The user types into the focused idle shell: typing within 250 ms.
    h.texts.insert(root.clone(), "$ ls".into());
    rt.step(&mut h, &[], 1_250).unwrap();
    assert!(crate::ipc::read_state(&run_dir, 1_250).typing);

    // A queued "Summon here" is served by the daemon.
    assert!(opens(&h).is_empty());
    request(&rt.st.dir, &old, 1_300).unwrap();
    rt.step(&mut h, &[], 1_300).unwrap();
    assert_eq!(opens(&h).len(), 2, "{:?}", h.calls);

    // Events are handled: a new tab is decorated after the grace period.
    let t = h.add_tab("ws", "new");
    let ev = Event::TabCreated(info(&h, &t));
    rt.step(&mut h, &[ev], 2_000).unwrap();
    rt.step(&mut h, &[], 2_000 + MOVE_GRACE_MS).unwrap();
    assert!(rt.st.store.tabs.contains_key(&TabKey::from(t.as_str())));

    // State is rewritten every second even when nothing changes.
    let written = crate::ipc::read_state(&run_dir, 2_400).written_ms;
    rt.step(&mut h, &[], 3_500).unwrap();
    assert!(crate::ipc::read_state(&run_dir, 3_500).written_ms > written);
}

#[test]
fn runtime_publishes_visible_tabs() {
    use super::SUBSCRIPTIONS;
    assert!(SUBSCRIPTIONS.contains(&"tab.focused"));
    let mut h = fake();
    let run_dir = temp_dir("runtime-visible");
    let mut rt = Runtime::new(state("runtime-visible"), cfg(), run_dir.clone());
    let a = h.add_tab("ws", "a"); // holds the focused pane
    let b = h.add_tab("ws", "b");
    rt.step(&mut h, &[], 1_000).unwrap();
    assert_eq!(
        crate::ipc::read_state(&run_dir, 1_000).visible_tabs,
        std::slice::from_ref(&a)
    );
    // A focus event is published at once, without waiting for the poll.
    let ev = Event::TabFocused { tab_id: b.clone() };
    rt.step(&mut h, &[ev], 1_100).unwrap();
    assert_eq!(crate::ipc::read_state(&run_dir, 1_100).visible_tabs, [b]);
    // The periodic poll (every second) corrects a missed event.
    rt.step(&mut h, &[], 2_100).unwrap();
    assert_eq!(crate::ipc::read_state(&run_dir, 2_100).visible_tabs, [a]);
}

/// herdr 0.9.5 restores plugin panes after a server restart as plain shells
/// that keep the pane id and label. Reconcile replaces them.
#[test]
fn restored_shells_are_replaced() {
    let dir = temp_dir("restored");
    let mut h = fake();
    let mut st = DaemonState::new(dir.clone(), pool());
    let (t, root) = user_tab(&mut h, &mut st, "a", 0);
    // A summoned multi-pane tab too.
    let m = h.add_tab("ws", "multi");
    let a = h.panes(&m).unwrap()[0].pane_id.clone();
    let b = user_split(&mut h, &a, Dir::Right);
    summon(&mut h, &mut st, &m, &cfg()).unwrap();
    let key = TabKey::from(t.as_str());
    let (s, g) = {
        let r = &st.store.tabs[&key];
        (r.sigil_pane.clone().unwrap(), r.grim_pane.clone().unwrap())
    };
    let mrec = st.store.tabs[&TabKey::from(m.as_str())].clone();
    drop(st);
    // The restart: same ids and labels, but shells.
    let shells = [
        &s,
        &g,
        mrec.sigil_pane.as_ref().unwrap(),
        mrec.grim_pane.as_ref().unwrap(),
    ];
    for id in shells {
        h.argv.insert(id.clone(), vec!["/bin/bash".into()]);
    }
    let mut st = DaemonState::new(dir, pool());
    h.calls.clear();
    reconcile(&mut h, &mut st, &cfg()).unwrap();

    for id in shells {
        assert!(
            h.calls.contains(&format!("remove_pane {id}")),
            "{id}: {:?}",
            h.calls
        );
    }
    let rec = st.store.tabs[&key].clone();
    let line = rec.line;
    assert_eq!(
        opens(&h)[..2],
        [
            format!("open_plugin_pane grimoire {root} down HERDR_WITCH_LINE={line}"),
            format!("open_plugin_pane sigil {root} right "),
        ]
    );
    // The multi-pane tab gets its corner panes back.
    assert!(opens(&h)[2].starts_with(&format!("open_plugin_pane grimoire {a} down")));
    assert_eq!(opens(&h)[3], format!("open_plugin_pane sigil {b} right "));
    assert_ne!(rec.sigil_pane.as_ref(), Some(&s));
    assert!(!rec.sigil_closed && !rec.grim_closed);
    // herdr's late `pane_closed` for the shells is not a user closure.
    for id in shells {
        closed(&mut h, &mut st, id);
    }
    assert!(!st.store.tabs[&key].sigil_closed && !st.store.tabs[&key].grim_closed);
    assert!(st.store.tabs[&key].sigil_pane.is_some());

    // Live witch panes are left alone.
    h.calls.clear();
    reconcile(&mut h, &mut st, &cfg()).unwrap();
    assert!(opens(&h).is_empty(), "{:?}", h.calls);
    assert!(!h.calls.iter().any(|c| c.starts_with("remove_pane")));
}

// ---- Fix round 1 ----

use super::restore::{check_server, looks_new};
use super::{Batch, ExeStatus, ExeWatch, Liveness, drain};

/// A recorded id can end up on a pane that isn't a restored witch shell
/// (herdr renumbered, or the user's own program runs in it): never close it.
#[test]
fn replace_shells_spares_other_panes() {
    let mut h = fake();
    let mut st = state("spare");
    let (t, root) = user_tab(&mut h, &mut st, "a", 0);
    let key = TabKey::from(t.as_str());
    // (a) The recorded sigil id now names the user's unlabelled shell.
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    h.close_pane(&s);
    st.store.tabs.get_mut(&key).unwrap().sigil_pane = Some(root.clone());
    // (b) A labelled grimoire running something that isn't a plain shell.
    let g = witch(&mut h, &t, GRIM_TITLE).unwrap();
    h.argv
        .insert(g.clone(), vec!["vim".into(), "notes.txt".into()]);
    // (c) A labelled sigil running a shell script in another tab.
    let (t2, _) = user_tab(&mut h, &mut st, "b", 1000);
    let s2 = witch(&mut h, &t2, SIGIL_TITLE).unwrap();
    h.argv
        .insert(s2.clone(), vec!["/bin/bash".into(), "build.sh".into()]);
    // (d) A labelled grimoire in a login shell is a restored shell.
    let g2 = witch(&mut h, &t2, GRIM_TITLE).unwrap();
    h.argv.insert(g2.clone(), vec!["-zsh".into()]);
    h.calls.clear();
    reconcile(&mut h, &mut st, &cfg()).unwrap();

    let removed: Vec<&String> = h
        .calls
        .iter()
        .filter(|c| c.starts_with("remove_pane"))
        .collect();
    assert_eq!(removed, [&format!("remove_pane {g2}")]);
    let alive: Vec<String> = h
        .panes(&t)
        .unwrap()
        .into_iter()
        .map(|p| p.pane_id)
        .collect();
    assert!(alive.contains(&root) && alive.contains(&g));
    assert!(h.panes(&t2).unwrap().iter().any(|p| p.pane_id == s2));
    // The stale id is no longer taken for the sigil.
    assert_ne!(st.store.tabs[&key].sigil_pane.as_ref(), Some(&root));
    assert!(!st.ours.contains(&root));
}

/// A fresh herdr on the same socket (crash, lost session) reuses tab and
/// pane numbers: the old records must not reopen or close anything there.
#[test]
fn new_server_forgets_old_records() {
    let dir = temp_dir("newserver");
    let mut old = fake();
    let mut st = DaemonState::new(dir.clone(), pool());
    let (t, _) = user_tab(&mut old, &mut st, "a", 0);
    let s = witch(&mut old, &t, SIGIL_TITLE).unwrap();
    closed(&mut old, &mut st, &s);
    drop(st);

    let mut h = fake(); // same ids again, but the user's panes
    let t1 = h.add_tab("ws", "fresh");
    assert_eq!(t1, t);
    let r = h.panes(&t1).unwrap()[0].pane_id.clone();
    user_split(&mut h, &r, Dir::Down);
    user_split(&mut h, &r, Dir::Right);
    let mut st = DaemonState::new(dir.clone(), pool());
    assert!(looks_new(&mut h, &st).unwrap());
    let mut settled = 0;
    assert!(check_server(&mut h, &mut st, &mut || settled += 1).unwrap());
    assert_eq!(settled, 1, "re-checked once after settling");
    assert!(st.store.tabs.is_empty() && st.ours.is_empty());
    assert!(Store::load(&dir).tabs.is_empty(), "persisted");
    h.calls.clear();
    reconcile(&mut h, &mut st, &cfg()).unwrap();
    assert!(opens(&h).is_empty(), "its tabs predate us: {:?}", h.calls);
    assert!(!h.calls.iter().any(|c| c.starts_with("remove_pane")));
}

#[test]
fn restored_server_keeps_records() {
    let mut h = fake();
    let mut st = state("restoredsrv");
    let (t, _) = user_tab(&mut h, &mut st, "a", 0);
    // Restored: same ids and labels (shells or not).
    let mut settled = 0;
    assert!(!looks_new(&mut h, &st).unwrap());
    assert!(!check_server(&mut h, &mut st, &mut || settled += 1).unwrap());
    assert_eq!(settled, 0);
    assert_eq!(st.store.tabs.len(), 1);
    // Mid-restore (the tab is back, its plugin panes not yet): looks new,
    // which is why `check_server` settles and checks again.
    for title in [SIGIL_TITLE, GRIM_TITLE] {
        let id = witch(&mut h, &t, title).unwrap();
        h.close_pane(&id);
    }
    assert!(looks_new(&mut h, &st).unwrap());
    // Nothing recorded: nothing to protect.
    let empty = state("restoredsrv-empty");
    assert!(!looks_new(&mut h, &empty).unwrap());
}

#[test]
fn drain_stops_at_first_error() {
    let (tx, rx) = std::sync::mpsc::channel();
    let ev = |t: &str| Event::LayoutUpdated { tab_id: t.into() };
    let gone = Event::PaneClosed {
        pane_id: "w1:p3".into(),
        tab_id: None,
    };
    tx.send(Ok(ev("w1:t1"))).unwrap();
    tx.send(Ok(gone.clone())).unwrap();
    tx.send(Err(crate::herdr::HerdrError::Io(
        std::io::ErrorKind::BrokenPipe.into(),
    )))
    .unwrap();
    tx.send(Ok(ev("w1:t9"))).unwrap();
    let b: Batch = drain(&rx, std::time::Duration::from_millis(10));
    assert_eq!(b.events, [ev("w1:t1"), gone]);
    assert!(matches!(b.error, Some(crate::herdr::HerdrError::Io(_))));
    assert!(!b.ended);
    drop(tx);
    let b = drain(&rx, std::time::Duration::from_millis(10));
    assert_eq!(b.events, [ev("w1:t9")]);
    assert!(b.error.is_none() && b.ended);
}

/// A socket error on one event must not cost the closures after it.
#[test]
fn step_records_closures_despite_socket_error() {
    let mut h = fake();
    let st = state("stepio");
    let mut rt = Runtime::new(st, cfg(), temp_dir("stepio-run"));
    let (t, _) = user_tab(&mut h, &mut rt.st, "a", 0);
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    h.close_pane(&s);
    h.fail.insert(format!("layout {t}"), || {
        crate::herdr::HerdrError::Io(std::io::ErrorKind::BrokenPipe.into())
    });
    let events = [
        Event::LayoutUpdated { tab_id: t.clone() },
        Event::PaneClosed {
            pane_id: s.clone(),
            tab_id: None,
        },
    ];
    assert!(matches!(
        rt.step(&mut h, &events, 10_000),
        Err(crate::herdr::HerdrError::Io(_))
    ));
    assert!(rt.st.store.tabs[&TabKey::from(t.as_str())].sigil_closed);
    assert!(Store::load(&rt.st.dir).tabs[&TabKey::from(t.as_str())].sigil_closed);
}

/// A failed install check must not cost the batch: the closure is recorded
/// and saved, and the error still comes back.
#[test]
fn step_records_closures_despite_failed_install_check() {
    let mut h = fake();
    let st = state("stepinstall");
    let mut rt = Runtime::new(st, cfg(), temp_dir("stepinstall-run"));
    let (t, _) = user_tab(&mut h, &mut rt.st, "a", 0);
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    h.close_pane(&s);
    h.fail.insert("plugin_enabled".into(), || {
        crate::herdr::HerdrError::Io(std::io::ErrorKind::BrokenPipe.into())
    });
    let events = [Event::PaneClosed {
        pane_id: s.clone(),
        tab_id: None,
    }];
    assert!(matches!(
        rt.step(&mut h, &events, 10_000),
        Err(crate::herdr::HerdrError::Io(_))
    ));
    assert!(rt.st.store.tabs[&TabKey::from(t.as_str())].sigil_closed);
    assert!(Store::load(&rt.st.dir).tabs[&TabKey::from(t.as_str())].sigil_closed);
}

#[test]
fn closure_recorded_before_the_orphan_check() {
    // `close_orphans` needs the socket; its failure must not lose a closure.
    let mut h = fake();
    let mut st = state("orphan-io");
    let (t, _) = user_tab(&mut h, &mut st, "a", 0);
    let s = witch(&mut h, &t, SIGIL_TITLE).unwrap();
    h.close_pane(&s);
    h.fail.insert("tabs".into(), || {
        crate::herdr::HerdrError::Io(std::io::ErrorKind::BrokenPipe.into())
    });
    let ev = Event::PaneClosed {
        pane_id: s.clone(),
        tab_id: None,
    };
    assert!(on_event(&mut h, &mut st, &cfg(), &ev, 0).is_err());
    assert!(st.store.tabs[&TabKey::from(t.as_str())].sigil_closed);
    assert!(Store::load(&st.dir).tabs[&TabKey::from(t.as_str())].sigil_closed);
    // An exited witch pane is likewise forgotten before the check fails.
    let g = witch(&mut h, &t, GRIM_TITLE).unwrap();
    h.close_pane(&g);
    let ev = Event::PaneExited { pane_id: g.clone() };
    assert!(on_event(&mut h, &mut st, &cfg(), &ev, 0).is_err());
    let rec = &st.store.tabs[&TabKey::from(t.as_str())];
    assert!(rec.grim_pane.is_none() && !rec.grim_closed);
    assert!(!st.ours.contains(&g));
}

#[test]
fn daemon_stops_once_disabled_or_unlinked() {
    for (plugin, why) in [(Some(false), "disabled"), (None, "unlinked")] {
        let mut h = fake();
        let mut rt = Runtime::new(state("installed"), cfg(), temp_dir("installed-run"));
        let (t, _) = user_tab(&mut h, &mut rt.st, "a", 0);
        rt.step(&mut h, &[], 1_000).unwrap();
        assert_eq!(rt.stop, None);
        h.plugin = plugin;
        rt.step(&mut h, &[], 5_000).unwrap();
        assert_eq!(rt.stop, None, "checked every 10 s");
        // Nothing more is done once it is gone: the summon goes unserved.
        request(&rt.st.dir, &t, 11_000).unwrap();
        h.calls.clear();
        rt.step(&mut h, &[], 11_000).unwrap();
        assert!(rt.stop.is_some(), "{why}");
        assert_eq!(h.calls, ["plugin_enabled"], "{why}");
    }
}

#[test]
fn install_check_falls_back_to_plugin_root() {
    let mut h = fake();
    h.fail.insert("plugin_enabled".into(), || {
        crate::herdr::HerdrError::Remote {
            code: "invalid_request".into(),
            message: "unknown method".into(),
        }
    });
    let root = temp_dir("plugin-root");
    std::fs::create_dir_all(&root).unwrap();
    let mut rt = Runtime::new(state("root"), cfg(), temp_dir("root-run"));
    rt.plugin_root = Some(root.clone());
    rt.step(&mut h, &[], 0).unwrap();
    assert_eq!(rt.stop, None, "root still there");
    std::fs::remove_dir_all(&root).unwrap();
    rt.step(&mut h, &[], 10_000).unwrap();
    assert!(rt.stop.is_some(), "root gone");
}

#[test]
fn exe_watch_gives_up_on_a_missing_binary() {
    let dir = temp_dir("exegone");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("herdr-witch");
    std::fs::write(&exe, "v1").unwrap();
    let mut w = ExeWatch::new(exe.clone());
    assert_eq!(w.check(0), ExeStatus::Same);
    std::fs::remove_file(&exe).unwrap();
    assert_eq!(w.check(10_000), ExeStatus::Same, "missing: not yet");
    assert_eq!(w.check(30_000), ExeStatus::Same);
    assert_eq!(w.check(40_000), ExeStatus::Gone, "missing for 30 s");
    // Back in time (a slow rebuild): the clock resets.
    let mut w = ExeWatch::new(exe.clone());
    assert_eq!(w.check(0), ExeStatus::Same);
    assert_eq!(w.check(20_000), ExeStatus::Same);
    std::fs::write(&exe, "v2").unwrap();
    assert_eq!(
        w.check(30_000),
        ExeStatus::Same,
        "first sighting is the baseline"
    );
    std::fs::remove_file(&exe).unwrap();
    assert_eq!(w.check(40_000), ExeStatus::Same);
    assert_eq!(w.check(60_000), ExeStatus::Same);
    assert_eq!(w.check(70_000), ExeStatus::Gone);
}

#[test]
fn daemon_gives_up_after_120s_unreachable() {
    let mut l = Liveness::default();
    assert!(
        l.after_session(false, 1_000),
        "first failure starts the clock"
    );
    assert!(l.after_session(false, 120_999));
    assert!(
        !l.after_session(false, 121_000),
        "120 s without a connection"
    );
    let mut l = Liveness::default();
    assert!(l.after_session(false, 0));
    assert!(
        l.after_session(true, 100_000),
        "a connection resets the clock"
    );
    assert!(l.after_session(false, 219_999));
    assert!(!l.after_session(false, 220_000));
}

#[test]
fn liveness_survives_a_clock_step_back() {
    let mut l = Liveness::default();
    assert!(l.after_session(false, 10_000));
    // The wall clock steps back (NTP): no underflow, not a give-up.
    assert!(l.after_session(false, 5_000));
    assert!(l.after_session(false, 129_999));
    assert!(!l.after_session(false, 130_000));
}

#[test]
fn exe_watch_sees_replaced_binary() {
    let dir = temp_dir("exewatch");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("herdr-witch");
    std::fs::write(&exe, "v1").unwrap();
    let mut w = ExeWatch::new(exe.clone());
    assert_eq!(w.check(0), ExeStatus::Same);
    // cargo replaces the file: new inode.
    std::fs::write(dir.join("new"), "v2").unwrap();
    std::fs::rename(dir.join("new"), &exe).unwrap();
    assert_eq!(
        w.check(5_000),
        ExeStatus::Same,
        "checked at most every 10 s"
    );
    assert_eq!(w.check(10_000), ExeStatus::Rebuilt);
    // Missing mid-build: not a change yet, and no exec of a missing file.
    let mut w = ExeWatch::new(exe.clone());
    std::fs::remove_file(&exe).unwrap();
    assert_eq!(w.check(0), ExeStatus::Same);
    let mut w = ExeWatch::new(exe);
    assert_eq!(w.check(0), ExeStatus::Same);
    assert_eq!(w.check(10_000), ExeStatus::Same);
}
