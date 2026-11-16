//! The sigil and grimoire programs against stray keys (#4): Ctrl-C, Ctrl-\
//! and Ctrl-Z must not end or freeze them (only herdr's close does), and
//! typed keys must not echo into them.

use rustix::process::{Pid, Signal, kill_process};
use rustix::termios::{LocalModes, tcgetattr};
use std::fs::File;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const KINDS: [&str; 2] = ["sigil", "grimoire"];

fn spawn(kind: &str, stdin: Stdio) -> Child {
    let dir = std::env::temp_dir().join(format!("hw-tty-{kind}-{}", std::process::id()));
    Command::new(env!("CARGO_BIN_EXE_herdr-witch"))
        .arg(kind)
        .env_remove("HERDR_TAB_ID")
        .env("XDG_RUNTIME_DIR", &dir)
        .env("HERDR_SOCKET_PATH", dir.join("none.sock"))
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// Keep reading the child's frames so it never blocks on a full pipe.
fn drain(child: &mut Child) -> std::thread::JoinHandle<()> {
    let mut out = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        let mut buf = [0u8; 65536];
        while matches!(out.read(&mut buf), Ok(n) if n > 0) {}
    })
}

fn state(pid: u32) -> char {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    stat.rsplit(')')
        .next()
        .unwrap()
        .trim_start()
        .chars()
        .next()
        .unwrap()
}

#[test]
fn stray_signals_do_not_stop_a_witch_pane() {
    for kind in KINDS {
        let mut child = spawn(kind, Stdio::null());
        let reader = drain(&mut child);
        std::thread::sleep(Duration::from_millis(300));
        let pid = Pid::from_raw(child.id() as i32).unwrap();
        for sig in [Signal::INT, Signal::QUIT, Signal::TSTP] {
            kill_process(pid, sig).unwrap();
        }
        std::thread::sleep(Duration::from_millis(300));
        let alive = child.try_wait().unwrap().is_none();
        let st = if alive { state(child.id()) } else { 'X' };
        let _ = child.kill();
        let _ = child.wait();
        reader.join().unwrap();
        assert!(alive, "{kind} was ended by a stray signal");
        assert_ne!(st, 'T', "{kind} was stopped by SIGTSTP");
    }
}

/// A pty whose slave end becomes the pane's stdin.
fn pty() -> (rustix::fd::OwnedFd, File) {
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    let slave = File::options()
        .read(true)
        .write(true)
        .open(name.to_str().unwrap())
        .unwrap();
    (master, slave)
}

fn quiet(f: &File) -> bool {
    let m = tcgetattr(f).unwrap().local_modes;
    !m.contains(LocalModes::ECHO) && !m.contains(LocalModes::ICANON)
}

#[test]
fn witch_pane_quiets_its_tty_and_restores_it() {
    for kind in KINDS {
        let (_master, slave) = pty();
        let before = tcgetattr(&slave).unwrap().local_modes;
        assert!(before.contains(LocalModes::ECHO | LocalModes::ICANON));
        let mut child = spawn(kind, Stdio::from(slave.try_clone().unwrap()));
        let mut out = child.stdout.take().unwrap();
        let mut buf = [0u8; 65536];
        let t = Instant::now();
        while !quiet(&slave) && t.elapsed() < Duration::from_secs(3) {
            let _ = out.read(&mut buf);
        }
        let was_quiet = quiet(&slave);
        // The pane goes away: its next frame fails and it exits normally.
        drop(out);
        let t = Instant::now();
        while child.try_wait().unwrap().is_none() && t.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let exited = child.try_wait().unwrap().is_some();
        let _ = child.kill();
        let _ = child.wait();
        assert!(was_quiet, "{kind}: echo and canonical mode still on");
        assert!(exited, "{kind} did not exit when its output closed");
        assert_eq!(
            tcgetattr(&slave).unwrap().local_modes,
            before,
            "{kind}: tty modes restored on exit"
        );
    }
}
