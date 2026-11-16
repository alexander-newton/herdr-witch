//! After a (re)connect: telling a restored herdr server from a new one, and
//! replacing the shells herdr 0.9.5 restores in place of plugin panes.
//!
//! Records are keyed by tab id and hold pane ids. A restored server keeps
//! both (and the witch panes' labels), but a fresh server on the same
//! socket path (a crash, a lost session file) numbers its tabs and panes
//! from 1 again, so an old record can name a user's pane. Hence:
//! - `check_server` forgets every record when the server looks new;
//! - `replace_shells` only ever closes a pane that is in the recorded tab,
//!   carries the kind's label and runs a plain interactive shell.
//!
//! herdr exposes no server instance id (`ping` gives only the version), so
//! "new" is inferred from the records, see `looks_new`.

use super::decorate::{DaemonState, KINDS};
use super::sizing::DragTracker;
use super::store::{Kind, TabKey};
use super::typing::TypingDetector;
use crate::herdr::{HerdrApi, HerdrError, Result};
use std::collections::HashSet;

/// Does the server look different from the one the records were made on?
/// When some witch pane is recorded: yes iff none of them is in its
/// recorded tab with its kind's label (herdr restores ids and labels).
/// Otherwise: yes iff none of the recorded tabs exists. No records: no.
///
/// A server still restoring looks new too, so callers check twice
/// (`check_server`).
pub fn looks_new(api: &mut dyn HerdrApi, st: &DaemonState) -> Result<bool> {
    if st.store.tabs.is_empty() {
        return Ok(false);
    }
    let live: HashSet<String> = api.tabs()?.into_iter().map(|t| t.tab_id).collect();
    let mut any_pane = false;
    for (key, rec) in &st.store.tabs {
        let recorded: Vec<(Kind, &str)> = KINDS
            .iter()
            .filter_map(|&k| rec.pane(k).map(|id| (k, id)))
            .collect();
        any_pane |= !recorded.is_empty();
        if recorded.is_empty() || !live.contains(&key.0) {
            continue;
        }
        let panes = api.panes(&key.0)?;
        let back = recorded.iter().any(|&(kind, id)| {
            panes
                .iter()
                .any(|p| p.pane_id == id && p.label.as_deref() == Some(kind.title()))
        });
        if back {
            return Ok(false);
        }
    }
    if any_pane {
        return Ok(true);
    }
    Ok(!st.store.tabs.keys().any(|k| live.contains(&k.0)))
}

/// If the server looks new, wait (`settle`; the daemon sleeps 2 s, since
/// herdr answers on its socket ~0.2 s before its restore is done) and check
/// again. If it still looks new, forget every record, and take the server's
/// current tabs as the baseline, as on a first install. Returns whether it
/// forgot.
pub fn check_server(
    api: &mut dyn HerdrApi,
    st: &mut DaemonState,
    settle: &mut dyn FnMut(),
) -> Result<bool> {
    if !looks_new(api, st)? {
        return Ok(false);
    }
    settle();
    if !looks_new(api, st)? {
        return Ok(false);
    }
    eprintln!(
        "herdr-witch: this herdr server is not the one the records were made on; forgetting them"
    );
    let tabs = api.tabs()?;
    st.store.tabs.clear();
    st.ours.clear();
    st.pending.clear();
    st.created_by_move.clear();
    st.respawned.clear();
    st.drag = DragTracker::default();
    st.typing = TypingDetector::default();
    st.baseline = Some(tabs.into_iter().map(|t| t.tab_id).collect());
    st.save();
    Ok(true)
}

/// Close the tab's recorded witch panes that herdr restored as shells,
/// forgetting them first so the `pane_closed` that follows isn't taken for
/// a user closure. A pane is closed only if it is in this tab, carries the
/// kind's label and runs a plain interactive shell (`is_plain_shell`).
/// A recorded pane running its witch program is kept (even if relabelled);
/// a labelled one with no foreground process is kept too. Any other
/// recorded id is stale: it is dropped from the record and its pane left
/// alone. Returns whether a pane was closed.
pub(super) fn replace_shells(
    api: &mut dyn HerdrApi,
    st: &mut DaemonState,
    tab_id: &str,
) -> Result<bool> {
    let key = TabKey::from(tab_id);
    let Some(rec) = st.store.tabs.get(&key) else {
        return Ok(false);
    };
    let recorded: Vec<(Kind, String)> = KINDS
        .iter()
        .filter_map(|&k| rec.pane(k).map(|id| (k, id.to_string())))
        .collect();
    if recorded.is_empty() {
        return Ok(false);
    }
    let panes = api.panes(tab_id)?;
    let mut changed = false;
    let mut replaced = false;
    for (kind, id) in recorded {
        let Some(pane) = panes.iter().find(|p| p.pane_id == id) else {
            forget(st, &key, kind, &id); // not in this tab (any more)
            changed = true;
            continue;
        };
        let labelled = pane.label.as_deref() == Some(kind.title());
        let argv = match api.pane_argv(&id) {
            Ok(argv) => argv,
            Err(HerdrError::Remote { .. }) => {
                forget(st, &key, kind, &id); // gone meanwhile
                changed = true;
                continue;
            }
            Err(e) => return Err(e),
        };
        if runs(&argv, kind) || (labelled && argv.is_empty()) {
            continue; // ours, or no evidence either way
        }
        forget(st, &key, kind, &id);
        changed = true;
        if !labelled || !is_plain_shell(&argv) {
            continue; // someone else's pane now: leave it alone
        }
        st.save(); // forgotten before herdr reports the close
        match api.remove_pane(&id) {
            Ok(()) | Err(HerdrError::Remote { .. }) => replaced = true,
            Err(e) => return Err(e),
        }
    }
    if changed {
        st.save();
    }
    Ok(replaced)
}

fn forget(st: &mut DaemonState, key: &TabKey, kind: Kind, id: &str) {
    if let Some(rec) = st.store.tabs.get_mut(key) {
        rec.set_pane(kind, None);
    }
    st.ours.remove(id);
}

/// Is `argv` `.../herdr-witch <kind's entrypoint> ...`?
pub(super) fn runs(argv: &[String], kind: Kind) -> bool {
    argv.first().map(|a| basename(a)) == Some("herdr-witch")
        && argv.get(1).map(String::as_str) == Some(kind.entrypoint())
}

/// An interactive shell with options at most (`/bin/bash`, `-zsh`,
/// `fish -l`): sh, bash, zsh, fish, dash or `$SHELL`'s name. `bash
/// script.sh` is not one.
fn is_plain_shell(argv: &[String]) -> bool {
    let Some(first) = argv.first() else {
        return false;
    };
    let name = basename(first).trim_start_matches('-');
    let login = std::env::var("SHELL").ok();
    let known = ["sh", "bash", "zsh", "fish", "dash"].contains(&name)
        || login.as_deref().map(basename) == Some(name);
    known && argv[1..].iter().all(|a| a.starts_with('-'))
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn shells_and_programs() {
        for shell in [
            &["/bin/bash"][..],
            &["-zsh"],
            &["fish", "-l"],
            &["/usr/bin/dash"],
        ] {
            assert!(is_plain_shell(&v(shell)), "{shell:?}");
        }
        for other in [&["vim", "x"][..], &["bash", "build.sh"], &[], &["python3"]] {
            assert!(!is_plain_shell(&v(other)), "{other:?}");
        }
        assert!(runs(
            &v(&["/p/target/release/herdr-witch", "sigil"]),
            Kind::Sigil
        ));
        assert!(!runs(&v(&["/p/herdr-witch", "sigil"]), Kind::Grimoire));
        assert!(!runs(&v(&["herdr-witchy", "sigil"]), Kind::Sigil));
    }
}
