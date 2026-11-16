//! The `HerdrApi` trait the daemon logic is written against, its request
//! types and error type.

use super::types::{AgentInfo, Dir, LayoutSnapshot, PaneInfo, TabInfo};
use std::fmt;

#[derive(Debug)]
pub enum HerdrError {
    /// The socket couldn't be reached, or the connection broke.
    Io(std::io::Error),
    /// The server sent something we couldn't understand.
    Protocol(String),
    /// The server answered with `{"error": {"code", "message"}}`.
    Remote { code: String, message: String },
}

pub type Result<T> = std::result::Result<T, HerdrError>;

impl fmt::Display for HerdrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HerdrError::Io(e) => write!(f, "herdr socket: {e}"),
            HerdrError::Protocol(m) => write!(f, "herdr protocol: {m}"),
            HerdrError::Remote { code, message } => write!(f, "herdr error {code}: {message}"),
        }
    }
}

impl std::error::Error for HerdrError {}

impl From<std::io::Error> for HerdrError {
    fn from(e: std::io::Error) -> Self {
        HerdrError::Io(e)
    }
}

impl From<serde_json::Error> for HerdrError {
    fn from(e: serde_json::Error) -> Self {
        HerdrError::Protocol(e.to_string())
    }
}

/// A `plugin.pane.open` request. It is always sent with `placement: "split"`
/// and `focus: false`: the plugin never steals focus.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenPane {
    /// The manifest `panes[].id` to open.
    pub entrypoint: &'static str,
    /// The pane to split. It keeps the first half; the new pane is second.
    pub target_pane_id: String,
    pub direction: Dir,
    pub env: Vec<(String, String)>,
}

pub trait HerdrApi {
    /// Every tab on the server.
    fn tabs(&mut self) -> Result<Vec<TabInfo>>;
    /// The panes in one tab, in layout order. An unknown or closed tab has
    /// no panes: `Ok(vec![])`, not an error.
    fn panes(&mut self, tab: &str) -> Result<Vec<PaneInfo>>;
    fn layout(&mut self, tab: &str) -> Result<LayoutSnapshot>;
    fn agents(&mut self) -> Result<Vec<AgentInfo>>;
    /// Split `req.target_pane_id` with a plugin pane; returns the new pane id.
    fn open_plugin_pane(&mut self, req: OpenPane) -> Result<String>;
    /// Set the ratio of the split at `path` (`[]` is the root split; `false`
    /// steps into the first child, `true` into the second).
    fn set_split_ratio(&mut self, tab: &str, path: &[bool], ratio: f32) -> Result<()>;
    /// The server-wide focused pane, if any.
    fn focused_pane(&mut self) -> Result<Option<PaneInfo>>;
    /// The pane's visible screen as plain text (`pane.read`, source
    /// `visible`). herdr 0.9.5 keeps no output revision, so typing is
    /// detected by polling this.
    fn pane_text(&mut self, pane_id: &str) -> Result<String>;
    /// The argv of the pane's (first) foreground process (`pane.process_info`),
    /// empty if it has none.
    fn pane_argv(&mut self, pane_id: &str) -> Result<Vec<String>>;
    /// Is this plugin (`PLUGIN_ID`) installed and enabled (`plugin.list`)?
    /// herdr sends no event when a plugin is disabled or unlinked.
    fn plugin_enabled(&mut self) -> Result<bool>;
    /// Close a pane (`pane.close`). The plugin only closes its own panes.
    fn remove_pane(&mut self, pane_id: &str) -> Result<()>;
}
