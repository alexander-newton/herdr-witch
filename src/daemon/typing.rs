//! "The user is typing": the focused pane's output changes while its agent
//! isn't working. herdr exposes no keystrokes, so this is inferred.
//!
//! Probe result (Task 9, herdr 0.9.5): `pane.updated` does **not** fire on
//! output, not even for real keystrokes typed through an attached client,
//! and `PaneInfo.revision` / `pane.read`'s `revision` never advance (they
//! stayed at 1 and 0). The spec's fallback, `events.wait` on
//! `pane_output_changed`, is rejected with `unsupported_event_wait_match`
//! ("events.wait currently supports pane agent status matches"), and the
//! `pane.output_matched` subscription fires only once, on first match. So
//! `poll` reads the focused pane's visible text and turns each change into
//! a synthetic revision for `on_pane_updated`, the same rule either way.

use super::decorate::is_witch;
use crate::herdr::{AgentStatus, HerdrApi, HerdrError, PaneInfo, Result};
use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};

/// How long typing lasts after the last output change.
pub const TYPING_HOLD_MS: u64 = 900;

#[derive(Debug, Default)]
pub struct TypingDetector {
    /// The last revision seen, and whose: `poll` reads one pane at a time.
    pub last_rev: Option<(String, u64)>,
    pub until_ms: u64,
    /// The pane `poll` last read: (pane id, text hash, synthetic revision).
    polled: Option<(String, u64, u64)>,
}

impl TypingDetector {
    /// Count a revision increase on the focused, non-plugin pane whose
    /// agent isn't working as typing. A pane's first sighting only records
    /// its revision. Plugin panes are excluded by id (`ours`) and by label.
    /// Fed only by `poll`: herdr's own revisions don't track output, and
    /// mixing them into `last_rev` would fake or hide changes.
    fn on_pane_updated(&mut self, p: &PaneInfo, now_ms: u64, ours: &HashSet<String>) {
        let prev = self.last_rev.replace((p.pane_id.clone(), p.revision));
        let changed = prev.is_some_and(|(id, r)| id == p.pane_id && p.revision > r);
        if changed
            && p.focused
            && p.agent_status != AgentStatus::Working
            && !ours.contains(&p.pane_id)
            && !is_witch(p)
        {
            self.until_ms = now_ms + TYPING_HOLD_MS;
        }
    }

    pub fn typing(&self, now_ms: u64) -> bool {
        now_ms < self.until_ms
    }

    /// The polling fallback: one `focused_pane` call, plus a `pane_text`
    /// call unless that pane is ours or its agent is working. Task 10 calls
    /// this every ~200 ms. A newly focused pane only sets a baseline. A
    /// remote error (pane gone) is skipped; only socket errors return.
    pub fn poll(
        &mut self,
        api: &mut dyn HerdrApi,
        ours: &HashSet<String>,
        now_ms: u64,
    ) -> Result<()> {
        let Some(mut p) = api.focused_pane()? else {
            self.polled = None;
            return Ok(());
        };
        if ours.contains(&p.pane_id) || is_witch(&p) || p.agent_status == AgentStatus::Working {
            // Not read; its next read starts a fresh baseline.
            self.polled = None;
            return Ok(());
        }
        let text = match api.pane_text(&p.pane_id) {
            Ok(text) => text,
            // The pane closed between the two calls: skip, re-baseline.
            Err(HerdrError::Remote { .. }) => {
                self.polled = None;
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let hash = hasher.finish();
        let rev = match &self.polled {
            Some((id, old, rev)) if *id == p.pane_id => rev + u64::from(*old != hash),
            _ => {
                self.last_rev = None;
                0
            }
        };
        self.polled = Some((p.pane_id.clone(), hash, rev));
        p.revision = rev;
        self.on_pane_updated(&p, now_ms, ours);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::fake::FakeHerdr;

    fn pane(id: &str, rev: u64, focused: bool, status: AgentStatus) -> PaneInfo {
        PaneInfo {
            pane_id: id.into(),
            revision: rev,
            focused,
            agent_status: status,
            ..PaneInfo::default()
        }
    }

    #[test]
    fn typing_from_focused_idle_pane() {
        let none = HashSet::new();
        let mut t = TypingDetector::default();
        t.on_pane_updated(&pane("w1:p1", 4, true, AgentStatus::Idle), 0, &none);
        assert!(!t.typing(0), "first sighting is a baseline");
        t.on_pane_updated(&pane("w1:p1", 5, true, AgentStatus::Idle), 100, &none);
        assert!(t.typing(100));
        let mut t = TypingDetector::default();
        t.on_pane_updated(&pane("w1:p2", 1, false, AgentStatus::Idle), 0, &none);
        t.on_pane_updated(&pane("w1:p2", 2, false, AgentStatus::Idle), 10, &none);
        assert!(!t.typing(10), "unfocused panes don't count");
        t.on_pane_updated(&pane("w1:p2", 2, true, AgentStatus::Unknown), 20, &none);
        assert!(!t.typing(20), "same revision isn't a change");
        t.on_pane_updated(&pane("w1:p2", 3, true, AgentStatus::Unknown), 30, &none);
        assert!(t.typing(30), "a plain shell (no agent) counts");
    }

    #[test]
    fn last_rev_tracks_one_pane() {
        let none = HashSet::new();
        let mut t = TypingDetector::default();
        t.on_pane_updated(&pane("w1:p1", 4, true, AgentStatus::Idle), 0, &none);
        t.on_pane_updated(&pane("w1:p1", 5, true, AgentStatus::Idle), 10, &none);
        assert_eq!(t.last_rev, Some(("w1:p1".to_string(), 5)));
        t.on_pane_updated(&pane("w1:p2", 9, true, AgentStatus::Idle), 20, &none);
        assert_eq!(t.last_rev, Some(("w1:p2".to_string(), 9)));
        assert!(
            !t.typing(1000),
            "another pane's first sighting is a baseline"
        );
    }

    #[test]
    fn typing_ignores_working_agent() {
        let none = HashSet::new();
        let mut t = TypingDetector::default();
        t.on_pane_updated(&pane("w1:p1", 1, true, AgentStatus::Working), 0, &none);
        t.on_pane_updated(&pane("w1:p1", 2, true, AgentStatus::Working), 50, &none);
        assert!(!t.typing(50));
    }

    #[test]
    fn typing_ignores_own_panes() {
        let ours = HashSet::from(["w1:p9".to_string()]);
        let mut t = TypingDetector::default();
        t.on_pane_updated(&pane("w1:p9", 1, true, AgentStatus::Unknown), 0, &ours);
        t.on_pane_updated(&pane("w1:p9", 2, true, AgentStatus::Unknown), 50, &ours);
        assert!(!t.typing(50));
    }

    #[test]
    fn typing_expires_after_900ms() {
        let none = HashSet::new();
        let mut t = TypingDetector::default();
        t.on_pane_updated(&pane("w1:p1", 1, true, AgentStatus::Idle), 0, &none);
        t.on_pane_updated(&pane("w1:p1", 2, true, AgentStatus::Idle), 1000, &none);
        assert!(t.typing(1000));
        assert!(t.typing(1899));
        assert!(!t.typing(1900));
    }

    #[test]
    fn typing_poll_detects_text_change() {
        let mut h = FakeHerdr::new();
        let a = h.add_tab("ws", "a");
        let pa = h.panes(&a).unwrap()[0].pane_id.clone();
        let b = h.add_tab("ws", "b");
        let pb = h.panes(&b).unwrap()[0].pane_id.clone();
        let none = HashSet::new();
        let mut t = TypingDetector::default();
        h.texts.insert(pa.clone(), "$ ".into());
        t.poll(&mut h, &none, 0).unwrap();
        t.poll(&mut h, &none, 200).unwrap();
        assert!(!t.typing(200), "unchanged text");
        h.texts.insert(pa.clone(), "$ l".into());
        t.poll(&mut h, &none, 400).unwrap();
        assert!(t.typing(400));
        assert!(!t.typing(1300));
        // Focus moves to a pane whose text changed while unfocused: baseline only.
        h.texts.insert(pb.clone(), "new output".into());
        h.focused = Some(pb.clone());
        t.poll(&mut h, &none, 2000).unwrap();
        assert!(!t.typing(2000));
        // A working agent's output is never read.
        h.panes
            .iter_mut()
            .find(|p| p.pane_id == pb)
            .unwrap()
            .agent_status = AgentStatus::Working;
        h.calls.clear();
        h.texts.insert(pb.clone(), "agent output".into());
        t.poll(&mut h, &none, 2200).unwrap();
        assert!(!t.typing(2200));
        assert_eq!(h.calls, ["focused_pane"]);
        // Nor is our own pane's.
        let ours = HashSet::from([pb.clone()]);
        h.calls.clear();
        t.poll(&mut h, &ours, 2400).unwrap();
        assert_eq!(h.calls, ["focused_pane"]);
        // Back to idle: the first read is a fresh baseline, not typing.
        h.panes
            .iter_mut()
            .find(|p| p.pane_id == pb)
            .unwrap()
            .agent_status = AgentStatus::Idle;
        t.poll(&mut h, &none, 2600).unwrap();
        assert!(!t.typing(2600));
        h.texts.insert(pb.clone(), "agent output!".into());
        t.poll(&mut h, &none, 2800).unwrap();
        assert!(t.typing(2800));
    }

    #[test]
    fn typing_poll_tolerates_closed_pane() {
        use crate::herdr::HerdrError;
        let mut h = FakeHerdr::new();
        let a = h.add_tab("ws", "a");
        let pa = h.panes(&a).unwrap()[0].pane_id.clone();
        let none = HashSet::new();
        let mut t = TypingDetector::default();
        h.texts.insert(pa.clone(), "$ ".into());
        t.poll(&mut h, &none, 0).unwrap();
        // The focused pane closes between focused_pane and pane_text.
        h.fail
            .insert(format!("pane_text {pa}"), || HerdrError::Remote {
                code: "pane_not_found".into(),
                message: "gone".into(),
            });
        h.texts.insert(pa.clone(), "$ x".into());
        t.poll(&mut h, &none, 200).unwrap();
        assert!(!t.typing(200));
        // The next successful read is only a baseline, not a change.
        h.fail.clear();
        t.poll(&mut h, &none, 400).unwrap();
        assert!(!t.typing(400));
        h.texts.insert(pa.clone(), "$ xy".into());
        t.poll(&mut h, &none, 600).unwrap();
        assert!(t.typing(600));
        // A socket error propagates.
        h.fail.insert(format!("pane_text {pa}"), || {
            HerdrError::Io(std::io::ErrorKind::BrokenPipe.into())
        });
        assert!(matches!(t.poll(&mut h, &none, 800), Err(HerdrError::Io(_))));
    }
}
