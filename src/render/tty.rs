//! A witch pane's terminal while it runs: Ctrl-C, Ctrl-\ and Ctrl-Z are
//! ignored (SIGINT, SIGQUIT and SIGTSTP), and echo and canonical mode are
//! off, so stray keys neither end, freeze nor scribble on the pane. Only
//! herdr closing the pane ends it, which keeps `pane_exited` meaning a
//! crash. The tty's modes are restored when the guard drops.

use rustix::process::Signal;
use rustix::termios::{LocalModes, OptionalActions, Termios, tcgetattr, tcsetattr};
use std::os::raw::c_int;

// rustix has no signal-disposition API and `libc` is not a dependency;
// std links the C library, so declare its `signal` (POSIX) directly.
unsafe extern "C" {
    fn signal(signum: c_int, handler: usize) -> usize;
}

/// The C library's `SIG_IGN` handler value.
const SIG_IGN: usize = 1;

/// Ignore the keyboard's job-control and interrupt signals in this process.
pub fn ignore_keyboard_signals() {
    for sig in [Signal::INT, Signal::QUIT, Signal::TSTP] {
        // SAFETY: installing SIG_IGN runs no code in signal context.
        unsafe {
            signal(sig.as_raw(), SIG_IGN);
        }
    }
}

/// Holds stdin's tty quiet (no ECHO, no ICANON) until dropped.
pub struct TtyGuard {
    saved: Option<Termios>,
}

impl TtyGuard {
    /// Ignore the keyboard signals and quiet stdin's tty, if it is one.
    pub fn new() -> Self {
        ignore_keyboard_signals();
        let stdin = std::io::stdin();
        let saved = tcgetattr(&stdin).ok();
        if let Some(t) = &saved {
            let mut quiet = t.clone();
            quiet
                .local_modes
                .remove(LocalModes::ECHO | LocalModes::ICANON);
            let _ = tcsetattr(&stdin, OptionalActions::Now, &quiet);
        }
        TtyGuard { saved }
    }
}

impl Default for TtyGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TtyGuard {
    fn drop(&mut self) {
        if let Some(t) = &self.saved {
            let _ = tcsetattr(std::io::stdin(), OptionalActions::Now, t);
        }
    }
}
