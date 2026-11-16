use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const STALE_MS: u64 = 3000;

/// What the daemon publishes for the sigil and grimoire panes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalState {
    pub working: u32,
    pub typing: bool,
    pub written_ms: u64,
    /// The tabs someone can see: herdr's focused tab (`tab.list` marks one
    /// tab focused server-wide). Empty when unknown, and missing in a file
    /// from an older daemon; panes then animate at full rate.
    #[serde(default)]
    pub visible_tabs: Vec<String>,
}

/// Milliseconds since the UNIX epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Atomically write `state.json` in `dir`, creating `dir` if needed.
pub fn write_state(dir: &Path, s: &GlobalState) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join("state.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(s).map_err(io::Error::other)?)?;
    std::fs::rename(&tmp, dir.join("state.json"))
}

/// Is a pane in tab `own` hidden? Only when both its tab and the visible
/// tabs are known and its tab isn't among them; any doubt means visible.
pub fn tab_hidden(own: Option<&str>, st: &GlobalState) -> bool {
    match own {
        Some(t) if !t.is_empty() && !st.visible_tabs.is_empty() => {
            !st.visible_tabs.iter().any(|v| v == t)
        }
        _ => false,
    }
}

/// The tab a witch pane runs in: herdr sets `HERDR_TAB_ID` for plugin panes
/// (probed on 0.9.5). It isn't updated if the pane is later moved to
/// another tab; the pane then follows its old tab's visibility.
pub fn own_tab() -> Option<String> {
    std::env::var("HERDR_TAB_ID").ok().filter(|t| !t.is_empty())
}

/// Read the state; missing, unparsable or older than 3000 ms reads as idle
/// (and with no visible tabs, so nothing is paused on stale information).
pub fn read_state(dir: &Path, now_ms: u64) -> GlobalState {
    let idle = GlobalState {
        written_ms: now_ms,
        ..GlobalState::default()
    };
    let Ok(bytes) = std::fs::read(dir.join("state.json")) else {
        return idle;
    };
    match serde_json::from_slice::<GlobalState>(&bytes) {
        Ok(s) if now_ms.saturating_sub(s.written_ms) <= STALE_MS => s,
        _ => idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("herdr-witch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn state_roundtrip() {
        let d = tmp("rt").join("nested");
        let s = GlobalState {
            working: 3,
            typing: true,
            written_ms: 10_000,
            ..GlobalState::default()
        };
        write_state(&d, &s).unwrap();
        assert!(!d.join("state.json.tmp").exists());
        assert_eq!(read_state(&d, 10_500), s);
    }

    #[test]
    fn stale_state_reads_idle() {
        let d = tmp("stale");
        let now = 50_000;
        write_state(
            &d,
            &GlobalState {
                working: 2,
                typing: true,
                written_ms: now - 3001,
                ..GlobalState::default()
            },
        )
        .unwrap();
        let r = read_state(&d, now);
        assert_eq!((r.working, r.typing), (0, false));
        write_state(
            &d,
            &GlobalState {
                working: 2,
                typing: true,
                written_ms: now - 3000,
                ..GlobalState::default()
            },
        )
        .unwrap();
        assert_eq!(read_state(&d, now).working, 2);
    }

    #[test]
    fn state_without_visible_tabs_still_parses() {
        // A daemon from before `visible_tabs` existed.
        let d = tmp("old-format");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("state.json"),
            r#"{"working":2,"typing":true,"written_ms":1000}"#,
        )
        .unwrap();
        let s = read_state(&d, 1000);
        assert_eq!((s.working, s.typing), (2, true));
        assert!(s.visible_tabs.is_empty());
    }

    #[test]
    fn visible_tabs_roundtrip() {
        let d = tmp("visible");
        let s = GlobalState {
            visible_tabs: vec!["w1:t2".into()],
            written_ms: 5,
            ..GlobalState::default()
        };
        write_state(&d, &s).unwrap();
        assert_eq!(read_state(&d, 5), s);
    }

    #[test]
    fn hidden_only_when_known_and_not_visible() {
        let st = |tabs: &[&str]| GlobalState {
            visible_tabs: tabs.iter().map(|t| t.to_string()).collect(),
            ..GlobalState::default()
        };
        assert!(tab_hidden(Some("w1:t1"), &st(&["w1:t2"])));
        assert!(!tab_hidden(Some("w1:t2"), &st(&["w1:t2"])));
        assert!(!tab_hidden(Some("w1:t2"), &st(&["w1:t1", "w1:t2"])));
        // Missing or stale info (read_state gives no tabs): full rate.
        assert!(!tab_hidden(Some("w1:t1"), &st(&[])));
        assert!(!tab_hidden(None, &st(&["w1:t2"])));
        assert!(!tab_hidden(Some(""), &st(&["w1:t2"])));
    }

    #[test]
    fn missing_state_reads_idle() {
        let r = read_state(&tmp("missing"), 1);
        assert_eq!((r.working, r.typing), (0, false));
    }

    #[test]
    fn garbage_state_reads_idle() {
        let d = tmp("garbage");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("state.json"), "{nope").unwrap();
        assert_eq!(read_state(&d, 1).working, 0);
    }
}
