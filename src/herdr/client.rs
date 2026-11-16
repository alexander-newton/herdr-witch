//! `SocketClient`: the real `HerdrApi` over herdr's Unix socket.
//!
//! Wire protocol (herdr 0.9.5, protocol 22, probed against a live server):
//! - Newline-delimited JSON over the Unix socket in `HERDR_SOCKET_PATH`.
//! - **One request per connection.** The client writes one line,
//!   `{"id":"1","method":"tab.list","params":{}}`, the server answers with
//!   one line and closes the connection; a second write on the same socket
//!   gets `EPIPE`. So every call opens a fresh connection.
//! - Success: `{"id":"1","result":{"type":"tab_list","tabs":[...]}}`; the
//!   `result.type` tag names the payload (`pong`, `pane_list`, `pane_layout`,
//!   `plugin_pane_opened`, `layout_split_ratio_set`, `ok`, ...).
//! - Failure: `{"id":"1","error":{"code":"tab_not_found","message":"..."}}`.
//!   Unknown methods give `invalid_request`; unknown params fields are ignored.
//! - `events.subscribe` with `{"subscriptions":[{"type":"tab.created"},...]}`
//!   keeps its connection open: the first line is the ack
//!   `{"id":..,"result":{"type":"subscription_started"}}`, then each event
//!   arrives as `{"event":"tab_created","data":{"type":"tab_created",...}}`.
//!   Subscription names are dotted, event names underscored.
//! - `PaneInfo.focused` is true for exactly one pane server-wide.

use super::PLUGIN_ID;
use super::api::{HerdrApi, HerdrError, OpenPane, Result};
use super::types::{AgentInfo, Event, LayoutSnapshot, PaneInfo, TabInfo, field};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct SocketClient {
    path: PathBuf,
    next_id: u64,
}

/// Per-request read/write timeout, so a wedged server can't hang the daemon.
const TIMEOUT: Duration = Duration::from_secs(5);

impl SocketClient {
    /// Remember `path` and check the server answers a `ping`.
    pub fn connect(path: impl AsRef<Path>) -> Result<Self> {
        let mut c = SocketClient {
            path: path.as_ref().to_path_buf(),
            next_id: 0,
        };
        c.ping()?;
        Ok(c)
    }

    /// Ping the server; returns its version.
    pub fn ping(&mut self) -> Result<String> {
        let r = self.call("ping", json!({}))?;
        field(&r, "version")
    }

    /// Open a separate, long-lived connection streaming events of `kinds`
    /// (dotted names such as `tab.created`). The stream ends when the server
    /// closes the connection; reconnect with a [`Backoff`].
    pub fn subscribe(path: impl AsRef<Path>, kinds: &[&str]) -> Result<EventStream> {
        let subs: Vec<Value> = kinds.iter().map(|k| json!({ "type": k })).collect();
        let mut reader = open(path.as_ref())?;
        send(
            reader.get_mut(),
            "sub",
            "events.subscribe",
            json!({ "subscriptions": subs }),
        )?;
        read_result(&mut reader, "sub")?;
        // Events may be minutes apart: block indefinitely from here on.
        reader.get_ref().set_read_timeout(None)?;
        Ok(EventStream { reader })
    }

    /// One request on a fresh connection; returns the `result` object.
    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id.to_string();
        let mut reader = open(&self.path)?;
        send(reader.get_mut(), &id, method, params)?;
        read_result(&mut reader, &id)
    }

    fn all_panes(&mut self) -> Result<Vec<PaneInfo>> {
        field(&self.call("pane.list", json!({}))?, "panes")
    }
}

fn open(path: &Path) -> Result<BufReader<UnixStream>> {
    let stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    Ok(BufReader::new(stream))
}

fn send(stream: &mut UnixStream, id: &str, method: &str, params: Value) -> Result<()> {
    let mut line = serde_json::to_vec(&json!({ "id": id, "method": method, "params": params }))?;
    line.push(b'\n');
    stream.write_all(&line)?;
    Ok(())
}

/// Read one response line and unwrap its `result`, mapping `error`.
fn read_result(reader: &mut BufReader<UnixStream>, id: &str) -> Result<Value> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(HerdrError::Protocol(
            "connection closed before reply".into(),
        ));
    }
    let mut v: Value = serde_json::from_str(&line)?;
    if v.get("id").and_then(Value::as_str) != Some(id) {
        return Err(HerdrError::Protocol(format!("reply id mismatch: {line}")));
    }
    if let Some(e) = v.get("error") {
        return Err(HerdrError::Remote {
            code: field(e, "code").unwrap_or_default(),
            message: field(e, "message").unwrap_or_default(),
        });
    }
    match v.get_mut("result") {
        Some(r) => Ok(r.take()),
        None => Err(HerdrError::Protocol(format!("no result: {line}"))),
    }
}

impl HerdrApi for SocketClient {
    fn tabs(&mut self) -> Result<Vec<TabInfo>> {
        field(&self.call("tab.list", json!({}))?, "tabs")
    }

    fn panes(&mut self, tab: &str) -> Result<Vec<PaneInfo>> {
        let mut ps = self.all_panes()?;
        ps.retain(|p| p.tab_id == tab);
        Ok(ps)
    }

    /// `pane.layout` addresses a tab through one of its panes.
    fn layout(&mut self, tab: &str) -> Result<LayoutSnapshot> {
        let pane = self
            .panes(tab)?
            .into_iter()
            .next()
            .ok_or_else(|| HerdrError::Remote {
                code: "tab_not_found".into(),
                message: format!("tab {tab} has no panes"),
            })?;
        field(
            &self.call("pane.layout", json!({ "pane_id": pane.pane_id }))?,
            "layout",
        )
    }

    fn agents(&mut self) -> Result<Vec<AgentInfo>> {
        field(&self.call("agent.list", json!({}))?, "agents")
    }

    fn open_plugin_pane(&mut self, req: OpenPane) -> Result<String> {
        let env: serde_json::Map<String, Value> = req
            .env
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
        let r = self.call(
            "plugin.pane.open",
            json!({
                "plugin_id": PLUGIN_ID,
                "entrypoint": req.entrypoint,
                "placement": "split",
                "target_pane_id": req.target_pane_id,
                "direction": req.direction,
                "focus": false,
                "env": env,
            }),
        )?;
        field(&r["plugin_pane"]["pane"], "pane_id")
    }

    fn set_split_ratio(&mut self, tab: &str, path: &[bool], ratio: f32) -> Result<()> {
        self.call(
            "layout.set_split_ratio",
            json!({ "tab_id": tab, "path": path, "ratio": ratio }),
        )?;
        Ok(())
    }

    fn focused_pane(&mut self) -> Result<Option<PaneInfo>> {
        Ok(self.all_panes()?.into_iter().find(|p| p.focused))
    }

    fn pane_text(&mut self, pane_id: &str) -> Result<String> {
        let r = self.call(
            "pane.read",
            json!({ "pane_id": pane_id, "source": "visible" }),
        )?;
        field(&r["read"], "text")
    }

    fn pane_argv(&mut self, pane_id: &str) -> Result<Vec<String>> {
        let r = self.call("pane.process_info", json!({ "pane_id": pane_id }))?;
        let first = &r["process_info"]["foreground_processes"][0];
        Ok(field(first, "argv").unwrap_or_default())
    }

    fn plugin_enabled(&mut self) -> Result<bool> {
        let r = self.call("plugin.list", json!({ "plugin_id": PLUGIN_ID }))?;
        let plugins: Vec<Value> = field(&r, "plugins")?;
        Ok(plugins.iter().any(|p| {
            p["plugin_id"].as_str() == Some(PLUGIN_ID) && p["enabled"].as_bool() == Some(true)
        }))
    }

    fn remove_pane(&mut self, pane_id: &str) -> Result<()> {
        self.call("pane.close", json!({ "pane_id": pane_id }))?;
        Ok(())
    }
}

/// The event stream from [`SocketClient::subscribe`].
pub struct EventStream {
    reader: BufReader<UnixStream>,
}

impl EventStream {
    /// A second handle on the stream's socket. `shutdown` on it ends the
    /// stream from another thread (a blocked `next` returns `None`).
    pub fn shutdown_handle(&self) -> std::io::Result<UnixStream> {
        self.reader.get_ref().try_clone()
    }
}

impl Iterator for EventStream {
    type Item = Result<Event>;

    /// `None` once the server closes the stream.
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Ok(0) => return None,
                Ok(_) if line.trim().is_empty() => continue,
                Ok(_) => return Some(Event::parse(&line)),
                Err(e) => return Some(Err(e.into())),
            }
        }
    }
}

const BACKOFF_MIN: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_secs(10);

/// Reconnect delays: 250 ms, doubling to a 10 s cap; `reset` after success.
pub struct Backoff {
    next: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

impl Backoff {
    pub fn new() -> Self {
        Backoff { next: BACKOFF_MIN }
    }

    /// The delay to wait before the next attempt.
    pub fn next_delay(&mut self) -> Duration {
        let d = self.next;
        self.next = (d * 2).min(BACKOFF_MAX);
        d
    }

    pub fn reset(&mut self) {
        self.next = BACKOFF_MIN;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::types::Dir;
    use std::os::unix::net::UnixListener;
    use std::thread::JoinHandle;

    /// A one-shot mock herdr: accepts `conns` connections in turn, reads one
    /// request line from each and writes `reply(request)` back. Returns the
    /// socket path and a handle yielding every request it saw.
    fn mock(
        name: &str,
        conns: usize,
        reply: impl Fn(&Value) -> String + Send + 'static,
    ) -> (PathBuf, JoinHandle<Vec<Value>>) {
        let dir = std::env::temp_dir().join(format!("hw-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let h = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..conns {
                let (mut s, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&s).read_line(&mut line).unwrap();
                let req: Value = serde_json::from_str(&line).unwrap();
                s.write_all(reply(&req).as_bytes()).unwrap();
                seen.push(req);
            }
            seen
        });
        (path, h)
    }

    fn ok(req: &Value, result: Value) -> String {
        format!(
            "{}\n",
            serde_json::json!({"id": req["id"], "result": result})
        )
    }

    fn pong(req: &Value) -> String {
        ok(
            req,
            serde_json::json!({"type":"pong","version":"0.9.5","protocol":22}),
        )
    }

    fn panes_json() -> Value {
        serde_json::json!({"type":"pane_list","panes":[
            {"pane_id":"w1:p1","tab_id":"w1:t1","workspace_id":"w1","focused":false,"agent_status":"idle","revision":3},
            {"pane_id":"w1:p2","tab_id":"w1:t2","workspace_id":"w1","focused":true,"agent_status":"working","revision":9},
            {"pane_id":"w1:p3","tab_id":"w1:t2","workspace_id":"w1","focused":false,"agent_status":"idle","revision":0}
        ]})
    }

    #[test]
    fn request_shape() {
        let (path, h) = mock("shape", 2, |req| match req["method"].as_str() {
            Some("ping") => pong(req),
            _ => ok(
                req,
                serde_json::json!({"type":"plugin_pane_opened","plugin_pane":{
                    "plugin_id":"herdr-witch","entrypoint":"sigil",
                    "pane":{"pane_id":"w1:p9","tab_id":"w1:t1","workspace_id":"w1","focused":false,"agent_status":"unknown","revision":0}}}),
            ),
        });
        let mut c = SocketClient::connect(&path).unwrap();
        let id = c
            .open_plugin_pane(OpenPane {
                entrypoint: "sigil",
                target_pane_id: "w1:p1".into(),
                direction: Dir::Right,
                env: vec![("HERDR_WITCH_LINE".into(), "3".into())],
            })
            .unwrap();
        assert_eq!(id, "w1:p9");
        let reqs = h.join().unwrap();
        assert_eq!(reqs[0]["method"], "ping");
        let r = &reqs[1];
        assert_eq!(r["method"], "plugin.pane.open");
        assert!(r["id"].is_string());
        assert_ne!(r["id"], reqs[0]["id"]);
        let p = &r["params"];
        assert_eq!(p["placement"], "split");
        assert_eq!(p["focus"], false);
        assert_eq!(p["plugin_id"], "herdr-witch");
        assert_eq!(p["entrypoint"], "sigil");
        assert_eq!(p["target_pane_id"], "w1:p1");
        assert_eq!(p["direction"], "right");
        assert_eq!(p["env"], serde_json::json!({"HERDR_WITCH_LINE": "3"}));
    }

    #[test]
    fn set_split_ratio_shape() {
        let (path, h) = mock("ratio", 2, |req| match req["method"].as_str() {
            Some("ping") => pong(req),
            _ => ok(
                req,
                serde_json::json!({"type":"layout_split_ratio_set","layout":{}}),
            ),
        });
        let mut c = SocketClient::connect(&path).unwrap();
        c.set_split_ratio("w1:t1", &[false], 0.85).unwrap();
        let r = &h.join().unwrap()[1];
        assert_eq!(r["method"], "layout.set_split_ratio");
        assert_eq!(r["params"]["tab_id"], "w1:t1");
        assert_eq!(r["params"]["path"], serde_json::json!([false]));
        assert!((r["params"]["ratio"].as_f64().unwrap() - 0.85).abs() < 1e-6);
    }

    #[test]
    fn pane_text_shape() {
        let (path, h) = mock("read", 2, |req| match req["method"].as_str() {
            Some("ping") => pong(req),
            _ => ok(
                req,
                serde_json::json!({"type":"pane_read","read":{
                    "pane_id":"w1:p1","workspace_id":"w1","tab_id":"w1:t1","source":"visible",
                    "format":"text","text":"$ ls\n","revision":0,"truncated":false}}),
            ),
        });
        let mut c = SocketClient::connect(&path).unwrap();
        assert_eq!(c.pane_text("w1:p1").unwrap(), "$ ls\n");
        let r = &h.join().unwrap()[1];
        assert_eq!(r["method"], "pane.read");
        assert_eq!(r["params"]["pane_id"], "w1:p1");
        assert_eq!(r["params"]["source"], "visible");
    }

    #[test]
    fn plugin_enabled_shape() {
        let entry = |enabled: bool| {
            serde_json::json!({"type":"plugin_list","plugins":[
                {"plugin_id":"herdr-witch","name":"herdr-witch","enabled":enabled,"plugin_root":"/p"}]})
        };
        for (plugins, want) in [
            (entry(true), true),
            (entry(false), false),
            (
                serde_json::json!({"type":"plugin_list","plugins":[]}),
                false,
            ),
        ] {
            let (path, h) = mock("plugins", 2, move |req| match req["method"].as_str() {
                Some("ping") => pong(req),
                _ => ok(req, plugins.clone()),
            });
            let mut c = SocketClient::connect(&path).unwrap();
            assert_eq!(c.plugin_enabled().unwrap(), want);
            let r = &h.join().unwrap()[1];
            assert_eq!(r["method"], "plugin.list");
            assert_eq!(r["params"]["plugin_id"], "herdr-witch");
        }
    }

    #[test]
    fn remote_error_is_mapped() {
        let (path, h) = mock("err", 2, |req| match req["method"].as_str() {
            Some("ping") => pong(req),
            _ => format!(
                "{}\n",
                serde_json::json!({"id": req["id"], "error": {"code":"split_not_found","message":"split path not found"}})
            ),
        });
        let mut c = SocketClient::connect(&path).unwrap();
        match c.set_split_ratio("w1:t1", &[true], 0.5) {
            Err(HerdrError::Remote { code, .. }) => assert_eq!(code, "split_not_found"),
            r => panic!("{r:?}"),
        }
        h.join().unwrap();
    }

    #[test]
    fn connect_fails_without_server() {
        let p = std::env::temp_dir().join(format!("hw-none-{}.sock", std::process::id()));
        assert!(matches!(SocketClient::connect(&p), Err(HerdrError::Io(_))));
    }

    #[test]
    fn panes_layout_and_focus() {
        let (path, h) = mock("panes", 6, |req| match req["method"].as_str() {
            Some("ping") => pong(req),
            Some("pane.list") => ok(req, panes_json()),
            Some("pane.layout") => ok(
                req,
                serde_json::json!({"type":"pane_layout","layout":{"tab_id":"w1:t2","area":{"x":0,"y":0,"width":120,"height":40},"panes":[],"splits":[]}}),
            ),
            m => panic!("unexpected {m:?}"),
        });
        let mut c = SocketClient::connect(&path).unwrap();
        assert_eq!(c.panes("w9:t9").unwrap(), []);
        let ps = c.panes("w1:t2").unwrap();
        assert_eq!(
            ps.iter().map(|p| p.pane_id.as_str()).collect::<Vec<_>>(),
            ["w1:p2", "w1:p3"]
        );
        let l = c.layout("w1:t2").unwrap();
        assert_eq!((l.tab_id.as_str(), l.area.height), ("w1:t2", 40));
        let f = c.focused_pane().unwrap().unwrap();
        assert_eq!(f.pane_id, "w1:p2");
        let reqs = h.join().unwrap();
        assert_eq!(reqs[4]["method"], "pane.layout");
        assert_eq!(reqs[4]["params"]["pane_id"], "w1:p2");
    }

    #[test]
    fn subscribe_streams_events() {
        let (path, h) = mock("sub", 1, |req| {
            let ack = ok(req, serde_json::json!({"type":"subscription_started"}));
            format!(
                "{ack}{}\n{}\n",
                r#"{"event":"tab_closed","data":{"type":"tab_closed","tab_id":"w1:t2","workspace_id":"w1"}}"#,
                r#"{"event":"pane_focused","data":{"type":"pane_focused","pane_id":"w1:p1","workspace_id":"w1"}}"#
            )
        });
        let evs: Vec<Event> = SocketClient::subscribe(&path, &["tab.closed", "pane.focused"])
            .unwrap()
            .map(|e| e.unwrap())
            .collect();
        assert_eq!(
            evs,
            [
                Event::TabClosed {
                    tab_id: "w1:t2".into()
                },
                Event::Other
            ]
        );
        let r = &h.join().unwrap()[0];
        assert_eq!(r["method"], "events.subscribe");
        assert_eq!(
            r["params"]["subscriptions"],
            serde_json::json!([{"type":"tab.closed"},{"type":"pane.focused"}])
        );
    }

    #[test]
    fn process_info_and_close_shape() {
        let (path, h) = mock("procinfo", 4, |req| match req["method"].as_str() {
            Some("ping") => pong(req),
            Some("pane.process_info") if req["params"]["pane_id"] == "w1:p9" => ok(
                req,
                serde_json::json!({"type":"pane_process_info","process_info":{"pane_id":"w1:p9",
                    "foreground_processes":[{"pid":5,"name":"herdr-witch",
                        "argv":["/x/target/release/herdr-witch","sigil"]}]}}),
            ),
            Some("pane.process_info") => ok(
                req,
                serde_json::json!({"type":"pane_process_info","process_info":{"pane_id":"w1:p8","foreground_processes":[]}}),
            ),
            Some("pane.close") => ok(req, serde_json::json!({"type":"ok"})),
            m => panic!("unexpected {m:?}"),
        });
        let mut c = SocketClient::connect(&path).unwrap();
        assert_eq!(
            c.pane_argv("w1:p9").unwrap(),
            ["/x/target/release/herdr-witch", "sigil"]
        );
        assert!(c.pane_argv("w1:p8").unwrap().is_empty());
        c.remove_pane("w1:p9").unwrap();
        let reqs = h.join().unwrap();
        assert_eq!(reqs[3]["method"], "pane.close");
        assert_eq!(reqs[3]["params"], serde_json::json!({"pane_id":"w1:p9"}));
    }

    #[test]
    fn stream_shutdown_ends_next() {
        let dir = std::env::temp_dir().join(format!("hw-shut-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&s).read_line(&mut line).unwrap();
            let req: Value = serde_json::from_str(&line).unwrap();
            let ack = ok(&req, serde_json::json!({"type":"subscription_started"}));
            s.write_all(ack.as_bytes()).unwrap();
            std::thread::sleep(Duration::from_secs(3)); // keeps the stream open
        });
        let mut stream = SocketClient::subscribe(&path, &["tab.closed"]).unwrap();
        let handle = stream.shutdown_handle().unwrap();
        let reader = std::thread::spawn(move || stream.next().is_none());
        std::thread::sleep(Duration::from_millis(100));
        let t0 = std::time::Instant::now();
        handle.shutdown(std::net::Shutdown::Both).unwrap();
        assert!(reader.join().unwrap(), "a shut-down stream ends");
        assert!(t0.elapsed() < Duration::from_secs(1));
        server.join().unwrap();
    }

    #[test]
    fn subscribe_error_is_reported() {
        let (path, h) = mock("suberr", 1, |req| {
            format!(
                "{}\n",
                serde_json::json!({"id": req["id"], "error": {"code":"invalid_request","message":"bad"}})
            )
        });
        assert!(matches!(
            SocketClient::subscribe(&path, &["nope"]),
            Err(HerdrError::Remote { .. })
        ));
        h.join().unwrap();
    }

    #[test]
    fn client_reconnect_backoff() {
        let mut b = Backoff::new();
        let ms: Vec<u128> = (0..8).map(|_| b.next_delay().as_millis()).collect();
        assert_eq!(ms, [250, 500, 1000, 2000, 4000, 8000, 10000, 10000]);
        b.reset();
        assert_eq!(b.next_delay().as_millis(), 250);
    }

    /// Pings the live herdr in `$HERDR_SOCKET_PATH`. Sends nothing but `ping`.
    #[test]
    #[ignore]
    fn live_ping() {
        let path = std::env::var("HERDR_SOCKET_PATH").expect("HERDR_SOCKET_PATH");
        let mut c = SocketClient::connect(&path).unwrap();
        assert!(!c.ping().unwrap().is_empty());
    }
}
