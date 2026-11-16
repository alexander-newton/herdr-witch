//! Serde types for the parts of the herdr socket API (schema_version 1) that
//! the plugin uses. Every struct defaults missing fields and ignores unknown
//! ones, so schema drift doesn't break parsing.

use super::api::{HerdrError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    #[default]
    #[serde(other)]
    Unknown,
}

/// A split direction: `right` puts the new pane beside, `down` below.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    #[default]
    Right,
    Down,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct TabInfo {
    pub tab_id: String,
    pub workspace_id: String,
    pub number: u32,
    pub label: String,
    pub focused: bool,
    pub pane_count: u32,
    pub agent_status: AgentStatus,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct PaneInfo {
    pub pane_id: String,
    pub tab_id: String,
    pub workspace_id: String,
    /// True only for the single server-wide focused pane.
    pub focused: bool,
    pub agent_status: AgentStatus,
    pub agent: Option<String>,
    /// Output revision; increases as the pane's terminal changes.
    pub revision: u64,
    /// Plugin panes carry the manifest pane `title` here.
    pub label: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct AgentInfo {
    pub pane_id: String,
    pub tab_id: String,
    pub workspace_id: String,
    pub agent: Option<String>,
    pub agent_status: AgentStatus,
    pub focused: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LayoutPane {
    pub pane_id: String,
    /// Focused within its tab (not necessarily server-wide).
    pub focused: bool,
    pub rect: Rect,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LayoutSplit {
    /// `split_<depth>_<path>`, e.g. `split_0_root`, `split_1_0` (path `[false]`).
    pub id: String,
    pub direction: Dir,
    /// Share of the split's rect given to its first child.
    pub ratio: f32,
    pub rect: Rect,
}

/// herdr's `PaneLayoutSnapshot`: the tab's area and every pane's and split's rect.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LayoutSnapshot {
    pub workspace_id: String,
    pub tab_id: String,
    pub zoomed: bool,
    pub area: Rect,
    pub focused_pane_id: String,
    pub panes: Vec<LayoutPane>,
    pub splits: Vec<LayoutSplit>,
}

/// One event from an `events.subscribe` stream.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    TabCreated(TabInfo),
    TabClosed {
        tab_id: String,
    },
    /// herdr's `pane_closed` carries only `pane_id` and `workspace_id`, so
    /// `tab_id` is `None` unless a future server adds it.
    PaneClosed {
        pane_id: String,
        tab_id: Option<String>,
    },
    /// The pane's process ended on its own and herdr removed the pane. A
    /// user close sends `pane_closed` instead (probed on 0.9.5: killing a
    /// plugin pane's process gives only `pane_exited`; `pane.close` and
    /// `plugin.pane.close` give only `pane_closed`).
    PaneExited {
        pane_id: String,
    },
    PaneMoved {
        pane_id: String,
        previous_pane_id: String,
        /// The tab the move created (`pane.move` to a new tab), if any.
        created_tab: Option<String>,
    },
    LayoutUpdated {
        tab_id: String,
    },
    /// The server-wide focused tab changed (a tab or workspace switch).
    TabFocused {
        tab_id: String,
    },
    Other,
}

impl Event {
    /// Parse one `{"event": kind, "data": {...}}` envelope.
    pub fn parse(line: &str) -> Result<Event> {
        let v: Value = serde_json::from_str(line)?;
        let kind = v
            .get("event")
            .and_then(Value::as_str)
            .ok_or_else(|| HerdrError::Protocol(format!("not an event: {line}")))?;
        let d = &v["data"];
        Ok(match kind {
            "tab_created" => Event::TabCreated(field(d, "tab")?),
            "tab_closed" => Event::TabClosed {
                tab_id: field(d, "tab_id")?,
            },
            "pane_closed" => Event::PaneClosed {
                pane_id: field(d, "pane_id")?,
                tab_id: field(d, "tab_id").ok(),
            },
            "pane_exited" => Event::PaneExited {
                pane_id: field(d, "pane_id")?,
            },
            "pane_moved" => Event::PaneMoved {
                pane_id: field(&d["pane"], "pane_id")?,
                previous_pane_id: field(d, "previous_pane_id")?,
                created_tab: field(&d["created_tab"], "tab_id").ok(),
            },
            "tab_focused" => Event::TabFocused {
                tab_id: field(d, "tab_id")?,
            },
            "layout_updated" => Event::LayoutUpdated {
                tab_id: field(&d["layout"], "tab_id")?,
            },
            _ => Event::Other,
        })
    }
}

/// Deserialize `v[key]`, failing if it is missing or null.
pub(crate) fn field<T: serde::de::DeserializeOwned>(v: &Value, key: &str) -> Result<T> {
    match v.get(key) {
        None | Some(Value::Null) => Err(HerdrError::Protocol(format!("missing `{key}`"))),
        Some(x) => Ok(T::deserialize(x)?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Event {
        let path = format!(
            "{}/src/herdr/fixtures/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).unwrap();
        Event::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    #[test]
    fn parse_fixtures() {
        match fixture("tab_created") {
            Event::TabCreated(t) => {
                assert_eq!(t.tab_id, "w1:t1");
                assert_eq!(t.workspace_id, "w1");
                assert_eq!(t.pane_count, 1);
            }
            e => panic!("tab_created: {e:?}"),
        }
        assert_eq!(
            fixture("tab_closed"),
            Event::TabClosed {
                tab_id: "w1:t2".into()
            }
        );
        assert_eq!(
            fixture("pane_closed"),
            Event::PaneClosed {
                pane_id: "w1:p4".into(),
                tab_id: None
            }
        );
        assert_eq!(
            fixture("pane_exited"),
            Event::PaneExited {
                pane_id: "w1:p4".into()
            }
        );
        // Not subscribed (herdr 0.9.5 never sends it for output): no parse path.
        assert_eq!(fixture("pane_updated"), Event::Other);
        assert_eq!(
            fixture("pane_moved"),
            Event::PaneMoved {
                pane_id: "w1:p3".into(),
                previous_pane_id: "w1:p3".into(),
                created_tab: Some("w1:t3".into())
            }
        );
        assert_eq!(
            fixture("layout_updated"),
            Event::LayoutUpdated {
                tab_id: "w1:t2".into()
            }
        );
        assert_eq!(fixture("pane_focused"), Event::Other);
        assert_eq!(
            fixture("tab_focused"),
            Event::TabFocused {
                tab_id: "w1:t2".into()
            }
        );
    }

    #[test]
    fn parse_tolerates_drift() {
        let line = r#"{"event":"tab_created","data":{"type":"tab_created","tab":{"tab_id":"w1:t9","agent_status":"sleepy","new_field":[1]}}}"#;
        match Event::parse(line).unwrap() {
            Event::TabCreated(t) => {
                assert_eq!(t.tab_id, "w1:t9");
                assert_eq!(t.agent_status, AgentStatus::Unknown);
            }
            e => panic!("{e:?}"),
        }
        let moved = r#"{"event":"pane_moved","data":{"pane":{"pane_id":"w2:p1"},"previous_pane_id":"w1:p1","created_tab":null}}"#;
        assert_eq!(
            Event::parse(moved).unwrap(),
            Event::PaneMoved {
                pane_id: "w2:p1".into(),
                previous_pane_id: "w1:p1".into(),
                created_tab: None
            }
        );
        assert_eq!(
            Event::parse(r#"{"event":"brand_new","data":{}}"#).unwrap(),
            Event::Other
        );
        assert!(Event::parse("not json").is_err());
        assert!(Event::parse(r#"{"event":"tab_closed","data":{}}"#).is_err());
    }

    #[test]
    fn layout_snapshot_parses() {
        let text = std::fs::read_to_string(format!(
            "{}/src/herdr/fixtures/layout_updated.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        let l: LayoutSnapshot = serde_json::from_value(v["data"]["layout"].clone()).unwrap();
        assert_eq!(l.area.height, 40);
        assert_eq!(l.panes.len(), 2);
        assert_eq!(l.splits[0].id, "split_0_root");
        assert_eq!(l.splits[0].direction, Dir::Down);
    }
}
