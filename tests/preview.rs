//! `herdr-witch scene ...`, the hidden preview of the familiars. It ignores
//! Ctrl-C like any witch pane, so it must always end by itself.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn witch(args: &[&str]) -> Command {
    let dir = std::env::temp_dir().join(format!("hw-preview-{}", std::process::id()));
    let mut c = Command::new(env!("CARGO_BIN_EXE_herdr-witch"));
    c.args(args)
        .env_remove("HERDR_TAB_ID")
        .env("XDG_RUNTIME_DIR", &dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}

#[test]
fn bad_scene_name_prints_usage() {
    for args in [&["scene", "dog"][..], &["scene"], &["scene", "cat", "wide"]] {
        let out = witch(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("usage: herdr-witch scene"), "{args:?}: {err}");
        assert!(out.stdout.is_empty(), "{args:?}: nothing drawn");
    }
}

#[test]
fn usage_keeps_the_scene_command_secret() {
    let out = witch(&[]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("scene"));
}

#[test]
fn preview_with_no_room_to_draw_ends_by_itself() {
    // Output to a pipe has no size at all: nothing can be drawn, so no
    // scene can ever play. The preview must say so and stop, not wait.
    let mut child = witch(&["scene", "all"]).spawn().unwrap();
    let t = Instant::now();
    while child.try_wait().unwrap().is_none() && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let exited = child.try_wait().unwrap().is_some();
    let _ = child.kill();
    let out = child.wait_with_output().unwrap();
    assert!(exited, "still running after 5 s");
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("too small"), "{err}");
}
