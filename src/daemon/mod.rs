//! The daemon: decorating tabs, persisting per-tab records, sizing the
//! grimoire, inferring typing, and the runtime loop that drives it all.
//!
//! `run` is a singleton per herdr session (an `flock` in the session's
//! state dir). A reader thread owns the blocking event stream and forwards
//! events over a channel; the main loop wakes at least every 100 ms to
//! handle them, serve "Summon here" requests, decorate queued tabs, poll
//! for typing (250 ms) and agents (1 s), and publish the global state for
//! the sigil and grimoire panes (on change, and every second). Any socket
//! error or the end of the stream drops the connection; the daemon backs
//! off, reconnects, resubscribes and reconciles (after checking it is still
//! the same server, `restore::check_server`).
//!
//! Lifetime: herdr kills startup-hook daemons on `server stop`, but one
//! started by "Summon here" is detached and outlives it. So any daemon
//! exits 0 once it has been unable to connect for `GIVE_UP_MS`, and it
//! re-executes itself when its binary is replaced (checked every 10 s), so
//! a rebuild takes effect without waiting for the old daemon to die.
//! herdr sends nothing when the plugin is disabled or unlinked (and keeps
//! this process running), so every `INSTALL_CHECK_MS` the daemon asks
//! `plugin.list` whether it is still installed and enabled, and exits 0 if
//! not, as it does when its binary has been missing for `EXE_GONE_MS`.

pub mod decorate;
pub mod restore;
pub mod sizing;
pub mod store;
pub mod summon;
pub mod typing;

#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod tests;

use crate::config::Config;
use crate::grimoire::GRIMOIRE;
use crate::grimoire::pool::LinePool;
use crate::herdr::{
    AgentStatus, Backoff, Event, EventStream, HerdrApi, HerdrError, Result, SocketClient,
};
use crate::ipc::{GlobalState, now_ms, write_state};
use decorate::{DaemonState, on_event, reconcile, skip_remote, tick};
use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

/// The singleton lock, in the per-session state dir.
pub const LOCK_FILE: &str = "daemon.lock";

/// `pane.updated` is left out: herdr 0.9.5 never sends it for output, and
/// typing comes from `TypingDetector::poll`. `pane.exited` tells a witch
/// pane that died from one the user closed (`pane.closed`). `tab.focused`
/// (also sent on a workspace switch) updates the visible tab at once.
pub const SUBSCRIPTIONS: [&str; 7] = [
    "tab.created",
    "tab.closed",
    "tab.focused",
    "pane.closed",
    "pane.exited",
    "pane.moved",
    "layout.updated",
];

const LOOP_MS: u64 = 100;
const TYPING_POLL_MS: u64 = 250;
const AGENTS_POLL_MS: u64 = 1000;
/// The visible tab is re-read this often, in case a `tab_focused` is missed.
const VISIBLE_POLL_MS: u64 = 1000;
/// The state is written on every change; this is only the heartbeat that
/// keeps it from going stale for the panes (`ipc::STALE_MS`, 3 s).
const WRITE_MS: u64 = 1000;
/// A daemon that can't reach its server for this long exits.
const GIVE_UP_MS: u64 = 120_000;
/// How often the daemon looks at its own executable.
const EXE_CHECK_MS: u64 = 10_000;
/// A daemon whose executable has been missing this long exits.
const EXE_GONE_MS: u64 = 30_000;
/// How often the daemon checks that the plugin is still installed and enabled.
const INSTALL_CHECK_MS: u64 = 10_000;
/// The wait before re-checking a server that looks new (it may still be
/// restoring: herdr answers ~0.2 s before its restore is done).
const SETTLE: Duration = Duration::from_secs(2);

/// Take an exclusive, non-blocking `flock` on `path` (created, with its
/// directory, if needed). `Ok(None)` means another process holds it; the
/// lock lasts as long as the returned file is open.
pub fn try_lock(path: &Path) -> std::io::Result<Option<File>> {
    use rustix::fs::{FlockOperation, flock};
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    match flock(&file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(Some(file)),
        Err(rustix::io::Errno::WOULDBLOCK) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// `herdr-witch daemon`. Exits 0 at once if another daemon holds this
/// session's lock; otherwise runs until killed.
pub fn run(cfg: Config) -> ! {
    let dir = crate::paths::state_dir();
    let _lock = match try_lock(&dir.join(LOCK_FILE)) {
        Ok(Some(lock)) => lock,
        Ok(None) => std::process::exit(0), // this session already has a daemon
        Err(e) => {
            eprintln!("herdr-witch daemon: locking in {}: {e}", dir.display());
            std::process::exit(1);
        }
    };
    let Some(sock) = std::env::var_os("HERDR_SOCKET_PATH") else {
        eprintln!("herdr-witch daemon: HERDR_SOCKET_PATH is not set");
        std::process::exit(2);
    };
    let seed = now_ms() ^ u64::from(std::process::id());
    let st = DaemonState::new(dir, LinePool::from_text(GRIMOIRE, seed));
    let mut rt = Runtime::new(st, cfg, crate::paths::runtime_dir());
    rt.exe = std::env::current_exe().ok().map(ExeWatch::new);
    rt.plugin_root = std::env::var_os("HERDR_PLUGIN_ROOT").map(PathBuf::from);
    let mut backoff = Backoff::new();
    let mut live = Liveness::default();
    loop {
        rt.connected = false;
        match rt.session(Path::new(&sock), &mut backoff) {
            Ok(()) => eprintln!("herdr-witch daemon: event stream ended; reconnecting"),
            Err(e) => eprintln!("herdr-witch daemon: {e}; reconnecting"),
        }
        rt.exit_if_stopped();
        if !live.after_session(rt.connected, now_ms()) {
            eprintln!("herdr-witch daemon: herdr unreachable for 120 s; exiting");
            std::process::exit(0);
        }
        std::thread::sleep(backoff.next_delay());
        rt.check_exe(now_ms());
        rt.exit_if_stopped();
    }
}

/// Counts how long the daemon has gone without connecting.
#[derive(Debug, Default)]
struct Liveness {
    down_since: Option<u64>,
}

impl Liveness {
    /// After a session ends (`connected`: it got as far as a ping): `false`
    /// once no connection succeeded for `GIVE_UP_MS`.
    fn after_session(&mut self, connected: bool, now: u64) -> bool {
        let since = match self.down_since {
            Some(t) if !connected => t,
            _ => now,
        };
        self.down_since = Some(since);
        now.saturating_sub(since) < GIVE_UP_MS
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExeStatus {
    Same,
    /// Replaced by a new build: re-execute it.
    Rebuilt,
    /// Missing for `EXE_GONE_MS` (the plugin was removed): exit.
    Gone,
}

/// Notices the daemon's executable being replaced (`cargo build` writes a
/// new file: new inode or mtime) or removed. A missing file is not a change
/// until it has been missing for `EXE_GONE_MS`.
struct ExeWatch {
    path: PathBuf,
    stamp: Option<(i64, u64)>,
    next_check: u64,
    missing_since: Option<u64>,
}

impl ExeWatch {
    fn new(path: PathBuf) -> Self {
        ExeWatch {
            stamp: Self::stamp(&path),
            path,
            next_check: 0,
            missing_since: None,
        }
    }

    fn stamp(path: &Path) -> Option<(i64, u64)> {
        std::fs::metadata(path).ok().map(|m| (m.mtime(), m.ino()))
    }

    /// Checks at most every `EXE_CHECK_MS`.
    fn check(&mut self, now: u64) -> ExeStatus {
        if now < self.next_check {
            return ExeStatus::Same;
        }
        self.next_check = now + EXE_CHECK_MS;
        let Some(stamp) = Self::stamp(&self.path) else {
            let since = *self.missing_since.get_or_insert(now);
            return if now.saturating_sub(since) >= EXE_GONE_MS {
                ExeStatus::Gone
            } else {
                ExeStatus::Same
            };
        };
        self.missing_since = None;
        match self.stamp {
            None => {
                self.stamp = Some(stamp);
                ExeStatus::Same
            }
            Some(old) if old != stamp => ExeStatus::Rebuilt,
            Some(_) => ExeStatus::Same,
        }
    }
}

/// The daemon's state across socket sessions, and its periodic duties.
pub struct Runtime {
    pub st: DaemonState,
    pub cfg: Config,
    runtime_dir: PathBuf,
    working: u32,
    /// The focused tab(s), published so panes in other tabs can pause.
    visible: Vec<String>,
    /// The (working, typing, visible) last written, to write again on change.
    published: Option<(u32, bool, Vec<String>)>,
    next_typing: u64,
    next_agents: u64,
    next_visible: u64,
    next_write: u64,
    write_failed: bool,
    /// The last session got as far as a ping.
    connected: bool,
    exe: Option<ExeWatch>,
    /// `HERDR_PLUGIN_ROOT`: if `plugin.list` can't be used, the plugin
    /// counts as installed while this directory exists.
    pub plugin_root: Option<PathBuf>,
    next_install: u64,
    /// Why the daemon should exit (0) now, if it should.
    pub stop: Option<String>,
}

impl Runtime {
    pub fn new(st: DaemonState, cfg: Config, runtime_dir: PathBuf) -> Self {
        Runtime {
            st,
            cfg: cfg.sanitized(),
            runtime_dir,
            working: 0,
            visible: Vec::new(),
            published: None,
            next_typing: 0,
            next_agents: 0,
            next_visible: 0,
            next_write: 0,
            write_failed: false,
            connected: false,
            exe: None,
            plugin_root: None,
            next_install: 0,
            stop: None,
        }
    }

    /// Exit 0 if something said the daemon should stop.
    fn exit_if_stopped(&self) {
        if let Some(why) = &self.stop {
            eprintln!("herdr-witch daemon: {why}; exiting");
            std::process::exit(0);
        }
    }

    /// Is the plugin still installed and enabled? A server that can't
    /// answer `plugin.list` (a remote error) is asked nothing more: the
    /// plugin's root directory existing is the answer then.
    fn installed(root: Option<&PathBuf>, api: &mut dyn HerdrApi) -> Result<bool> {
        match api.plugin_enabled() {
            Ok(enabled) => Ok(enabled),
            Err(HerdrError::Remote { .. }) => Ok(root.is_none_or(|r| r.is_dir())),
            Err(e) => Err(e),
        }
    }

    /// Replace this process with the rebuilt binary, same arguments. The
    /// lock's file is close-on-exec, so the new image takes it again. If
    /// the exec fails (a half-written file), keep running and retry later.
    /// A binary missing for `EXE_GONE_MS` sets `stop`.
    fn check_exe(&mut self, now: u64) {
        use std::os::unix::process::CommandExt;
        let Some(w) = self.exe.as_mut() else {
            return;
        };
        match w.check(now) {
            ExeStatus::Same => return,
            ExeStatus::Gone => {
                self.stop = Some(format!("{} is gone", w.path.display()));
                return;
            }
            ExeStatus::Rebuilt => {}
        }
        eprintln!("herdr-witch daemon: binary rebuilt; restarting");
        let e = std::process::Command::new(&w.path)
            .args(std::env::args_os().skip(1))
            .exec();
        eprintln!("herdr-witch daemon: restarting failed: {e}");
    }

    /// One connection: subscribe, reconcile, then loop until the stream
    /// ends (`Ok`) or a socket error (`Err`).
    fn session(&mut self, sock: &Path, backoff: &mut Backoff) -> Result<()> {
        let mut api = SocketClient::connect(sock)?;
        self.connected = true;
        // Subscribe before reconciling so nothing in between is missed.
        let stream = SocketClient::subscribe(sock, &SUBSCRIPTIONS)?;
        let _stop = StopOnDrop(stream.shutdown_handle()?);
        let rx = spawn_reader(stream);
        restore::check_server(&mut api, &mut self.st, &mut || std::thread::sleep(SETTLE))?;
        reconcile(&mut api, &mut self.st, &self.cfg)?;
        backoff.reset();
        loop {
            let batch = drain(&rx, Duration::from_millis(LOOP_MS));
            // Handle what came before a stream error, then report it.
            let stepped = self.step(&mut api, &batch.events, now_ms());
            if let Some(e) = batch.error {
                return Err(e);
            }
            stepped?;
            self.exit_if_stopped();
            if batch.ended {
                return Ok(());
            }
            self.check_exe(now_ms());
            self.exit_if_stopped();
        }
    }

    /// One turn of the loop: handle `events`, serve summon requests,
    /// decorate due tabs, poll typing and agents when due, and publish the
    /// global state. Only socket errors are returned.
    pub fn step(&mut self, api: &mut dyn HerdrApi, events: &[Event], now: u64) -> Result<()> {
        if self.stop.is_some() {
            return Ok(()); // touch nothing more
        }
        let (st, cfg) = (&mut self.st, &self.cfg);
        // Every event is handled even after a socket error: a closure needs
        // no socket, and must not be lost to an earlier event's failure.
        let mut failed = None;
        for ev in events {
            if let Event::TabFocused { tab_id } = ev {
                self.visible = vec![tab_id.clone()];
            }
            if let Err(e) = on_event(api, st, cfg, ev, now) {
                failed.get_or_insert(e);
            }
        }
        // After the events: a failed check must not cost the batch.
        if now >= self.next_install {
            self.next_install = now + INSTALL_CHECK_MS;
            match Self::installed(self.plugin_root.as_ref(), api) {
                Ok(true) => {}
                Ok(false) => self.stop = Some("the plugin is disabled or uninstalled".into()),
                Err(e) => {
                    failed.get_or_insert(e);
                }
            }
        }
        if let Some(e) = failed {
            return Err(e);
        }
        if self.stop.is_some() {
            return Ok(()); // touch nothing more
        }
        for tab in summon::take_requests(&st.dir, now) {
            if let Err(e) = summon::summon(api, st, &tab, cfg) {
                skip_remote(&format!("summoning {tab}"), e)?;
            }
        }
        tick(api, st, cfg, now)?;
        if now >= self.next_typing {
            st.typing.poll(api, &st.ours, now)?;
            self.next_typing = now + TYPING_POLL_MS;
        }
        if now >= self.next_agents {
            match api.agents() {
                Ok(agents) => {
                    let working = agents
                        .iter()
                        .filter(|a| a.agent_status == AgentStatus::Working)
                        .count();
                    self.working = u32::try_from(working).unwrap_or(u32::MAX);
                }
                Err(e) => skip_remote("listing agents", e)?,
            }
            self.next_agents = now + AGENTS_POLL_MS;
        }
        if now >= self.next_visible {
            match api.tabs() {
                Ok(tabs) => {
                    self.visible = tabs
                        .into_iter()
                        .filter(|t| t.focused)
                        .map(|t| t.tab_id)
                        .collect();
                }
                Err(e) => skip_remote("listing tabs", e)?,
            }
            self.next_visible = now + VISIBLE_POLL_MS;
        }
        let state = (self.working, st.typing.typing(now), self.visible.clone());
        if self.published.as_ref() != Some(&state) || now >= self.next_write {
            self.publish(state, now);
        }
        Ok(())
    }

    fn publish(&mut self, (working, typing, visible): (u32, bool, Vec<String>), now: u64) {
        let s = GlobalState {
            working,
            typing,
            written_ms: now,
            visible_tabs: visible.clone(),
        };
        match write_state(&self.runtime_dir, &s) {
            Ok(()) => self.write_failed = false,
            Err(e) if !self.write_failed => {
                self.write_failed = true; // logged once per failure streak
                eprintln!(
                    "herdr-witch daemon: writing state in {}: {e}",
                    self.runtime_dir.display()
                );
            }
            Err(_) => {}
        }
        self.published = Some((working, typing, visible));
        self.next_write = now + WRITE_MS;
    }
}

/// Forward the stream's events to the main loop. Unparsable lines are
/// skipped; a socket error is forwarded and ends the thread, as does the
/// end of the stream (the receiver then sees `Disconnected`).
fn spawn_reader(stream: EventStream) -> Receiver<Result<Event>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for item in stream {
            match item {
                Err(HerdrError::Protocol(m)) => {
                    eprintln!("herdr-witch daemon: skipping an event: {m}");
                }
                item => {
                    let last = item.is_err();
                    if tx.send(item).is_err() || last {
                        return;
                    }
                }
            }
        }
    });
    rx
}

/// Ends the event stream when a session is dropped, so its reader thread
/// doesn't outlive it.
struct StopOnDrop(UnixStream);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}

/// What one turn of the loop took from the reader thread.
#[derive(Debug, Default)]
struct Batch {
    events: Vec<Event>,
    /// The stream's socket error, if one came; nothing after it is taken.
    error: Option<HerdrError>,
    /// The stream ended (the reader thread is gone).
    ended: bool,
}

/// Wait up to `wait` for the first item, then take what is queued, stopping
/// at the first error.
fn drain(rx: &Receiver<Result<Event>>, wait: Duration) -> Batch {
    let mut b = Batch::default();
    let take = |item: Result<Event>, b: &mut Batch| match item {
        Ok(ev) => b.events.push(ev),
        Err(e) => b.error = Some(e),
    };
    match rx.recv_timeout(wait) {
        Ok(item) => take(item, &mut b),
        Err(RecvTimeoutError::Timeout) => return b,
        Err(RecvTimeoutError::Disconnected) => {
            b.ended = true;
            return b;
        }
    }
    while b.error.is_none() {
        match rx.try_recv() {
            Ok(item) => take(item, &mut b),
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                b.ended = true;
                break;
            }
        }
    }
    b
}
