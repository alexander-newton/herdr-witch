//! Per-tab records (grimoire line, witch pane ids, closures, user sizing),
//! persisted as `tabs.json` in the state dir so they survive restarts.

use crate::herdr::TabInfo;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A tab's stable identity. herdr 0.9.5 keeps tab ids across
/// `server stop` + restore (probed in Task 9), so this is the tab id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TabKey(pub String);

impl From<&str> for TabKey {
    fn from(tab_id: &str) -> Self {
        TabKey(tab_id.to_string())
    }
}

pub fn tab_key(t: &TabInfo) -> TabKey {
    TabKey::from(t.tab_id.as_str())
}

/// The two pane kinds the plugin adds to a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sigil,
    Grimoire,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabRecord {
    /// Index into the grimoire pool's lines.
    pub line: usize,
    pub sigil_pane: Option<String>,
    pub grim_pane: Option<String>,
    pub sigil_closed: bool,
    pub grim_closed: bool,
    /// The user dragged the grimoire split; never resize it again.
    pub user_sized: bool,
}

impl TabRecord {
    pub fn pane(&self, k: Kind) -> Option<&str> {
        match k {
            Kind::Sigil => self.sigil_pane.as_deref(),
            Kind::Grimoire => self.grim_pane.as_deref(),
        }
    }

    pub fn closed(&self, k: Kind) -> bool {
        match k {
            Kind::Sigil => self.sigil_closed,
            Kind::Grimoire => self.grim_closed,
        }
    }

    /// Record (or forget) the pane showing kind `k`.
    pub fn set_pane(&mut self, k: Kind, id: Option<String>) {
        match k {
            Kind::Sigil => self.sigil_pane = id,
            Kind::Grimoire => self.grim_pane = id,
        }
    }

    /// The pane of kind `k` is gone for good: forget it and never reopen it.
    pub fn mark_closed(&mut self, k: Kind) {
        self.set_pane(k, None);
        match k {
            Kind::Sigil => self.sigil_closed = true,
            Kind::Grimoire => self.grim_closed = true,
        }
    }
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub tabs: HashMap<TabKey, TabRecord>,
    /// Set when `load` found an unusable file it couldn't move aside;
    /// `save` then refuses, so that file is never overwritten.
    #[serde(skip)]
    blocked: bool,
}

pub const STORE_FILE: &str = "tabs.json";

impl Store {
    /// Load `dir/tabs.json`. A missing file gives an empty store. Any other
    /// problem (unreadable, not UTF-8, not valid JSON) logs one stderr line
    /// and moves the file aside to `tabs.json.corrupt-<ms>`, then starts
    /// empty; if it can't be moved, the store is blocked from saving.
    pub fn load(dir: &Path) -> Store {
        let path = dir.join(STORE_FILE);
        let problem = match std::fs::read_to_string(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Store::default(),
            Err(e) => e.to_string(),
            Ok(text) => match serde_json::from_str(&text) {
                Ok(store) => return store,
                Err(e) => e.to_string(),
            },
        };
        let stamp = crate::ipc::now_ms();
        let aside = (0..)
            .map(|n| match n {
                0 => dir.join(format!("{STORE_FILE}.corrupt-{stamp}")),
                n => dir.join(format!("{STORE_FILE}.corrupt-{stamp}-{n}")),
            })
            .find(|p| !p.exists())
            .expect("unbounded");
        let blocked = std::fs::rename(&path, &aside).is_err();
        let fate = if blocked {
            "could not move it aside; not saving".to_string()
        } else {
            format!("moved to {}", aside.display())
        };
        eprintln!(
            "herdr-witch: unusable {} ({problem}); {fate}",
            path.display()
        );
        Store {
            blocked,
            ..Store::default()
        }
    }

    /// Write `dir/tabs.json` atomically (temp file, then rename).
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        if self.blocked {
            return Err(std::io::Error::other(
                "tabs.json is unusable and could not be moved aside",
            ));
        }
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{STORE_FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(&tmp, dir.join(STORE_FILE))
    }

    /// The grimoire lines recorded tabs already show.
    pub fn live_lines(&self) -> HashSet<usize> {
        self.tabs.values().map(|r| r.line).collect()
    }

    /// The tab and kind whose recorded witch pane is `pane_id`.
    pub fn find_pane(&self, pane_id: &str) -> Option<(TabKey, Kind)> {
        self.tabs.iter().find_map(|(key, r)| {
            [Kind::Sigil, Kind::Grimoire]
                .into_iter()
                .find(|&k| r.pane(k) == Some(pane_id))
                .map(|k| (key.clone(), k))
        })
    }
}

#[cfg(test)]
pub(crate) fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("hw-store-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Store {
        let mut s = Store::default();
        s.tabs.insert(
            TabKey::from("w1:t1"),
            TabRecord {
                line: 4,
                sigil_pane: Some("w1:p2".into()),
                grim_pane: None,
                grim_closed: true,
                ..TabRecord::default()
            },
        );
        s.tabs.insert(
            TabKey::from("w2:t3"),
            TabRecord {
                line: 9,
                grim_pane: Some("w2:p7".into()),
                user_sized: true,
                ..TabRecord::default()
            },
        );
        s
    }

    #[test]
    fn store_round_trips() {
        let dir = temp_dir("rt");
        let s = sample();
        s.save(&dir).unwrap();
        assert_eq!(Store::load(&dir), s);
        assert!(!dir.join("tabs.json.tmp").exists());
        let text = std::fs::read_to_string(dir.join(STORE_FILE)).unwrap();
        assert!(text.contains("\"w1:t1\""), "{text}");
    }

    fn quarantined(dir: &Path) -> Vec<Vec<u8>> {
        let mut v: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("tabs.json.corrupt-")
            })
            .map(|p| std::fs::read(p).unwrap())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn store_missing_is_empty() {
        let dir = temp_dir("missing");
        assert_eq!(Store::load(&dir), Store::default());
        Store::default().save(&dir).unwrap();
    }

    /// A file we can't use is moved aside, never overwritten.
    #[test]
    fn store_corrupt_is_quarantined() {
        let dir = temp_dir("bad");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(STORE_FILE), "{not json").unwrap();
        let s = Store::load(&dir);
        assert!(s.tabs.is_empty());
        assert!(!dir.join(STORE_FILE).exists());
        sample().save(&dir).unwrap();
        assert_eq!(Store::load(&dir), sample());
        // Not valid UTF-8 either.
        std::fs::write(dir.join(STORE_FILE), [0xff, 0xfe, b'{']).unwrap();
        assert!(Store::load(&dir).tabs.is_empty());
        assert_eq!(
            quarantined(&dir),
            [b"{not json".to_vec(), vec![0xff, 0xfe, b'{']]
        );
    }

    /// If the bad file can't even be moved aside, saving is refused.
    #[test]
    fn store_unmovable_blocks_save() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("locked");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(STORE_FILE);
        std::fs::write(&file, "{\"tabs\":{\"w1:t1\":{\"line\":3}}}").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let s = Store::load(&dir);
        let saved = s.save(&dir);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(s.tabs.is_empty());
        assert!(saved.is_err());
        assert_eq!(Store::load(&dir).tabs[&TabKey::from("w1:t1")].line, 3);
    }

    #[test]
    fn store_lookups() {
        let s = sample();
        assert_eq!(s.live_lines(), HashSet::from([4, 9]));
        assert_eq!(
            s.find_pane("w2:p7"),
            Some((TabKey::from("w2:t3"), Kind::Grimoire))
        );
        assert_eq!(
            s.find_pane("w1:p2"),
            Some((TabKey::from("w1:t1"), Kind::Sigil))
        );
        assert_eq!(s.find_pane("w1:p1"), None);
    }

    #[test]
    fn record_kinds() {
        let mut r = TabRecord::default();
        r.set_pane(Kind::Sigil, Some("a".into()));
        r.set_pane(Kind::Grimoire, Some("b".into()));
        assert_eq!(
            (r.pane(Kind::Sigil), r.pane(Kind::Grimoire)),
            (Some("a"), Some("b"))
        );
        r.mark_closed(Kind::Sigil);
        assert_eq!(r.pane(Kind::Sigil), None);
        assert!(r.closed(Kind::Sigil) && !r.closed(Kind::Grimoire));
        assert_eq!(
            tab_key(&TabInfo {
                tab_id: "w1:t6".into(),
                ..TabInfo::default()
            }),
            TabKey::from("w1:t6")
        );
    }
}
