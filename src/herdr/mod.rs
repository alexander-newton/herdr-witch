//! herdr socket API: serde types, the `HerdrApi` trait, the real socket
//! client and (for tests) an in-memory fake.

pub mod api;
pub mod client;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
pub mod types;

pub use api::{HerdrApi, HerdrError, OpenPane, Result};
pub use client::{Backoff, EventStream, SocketClient};
pub use types::{AgentInfo, AgentStatus, Dir, Event, LayoutSnapshot, PaneInfo, TabInfo};

/// The manifest `id` herdr knows this plugin by (`plugin.pane.open` needs it).
pub const PLUGIN_ID: &str = "herdr-witch";
