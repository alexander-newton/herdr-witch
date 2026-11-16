use std::path::PathBuf;

fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// First 12 hex chars of the FNV-1a 64 hash of the herdr socket path.
pub fn session_key(socket: &str) -> String {
    format!("{:016x}", fnv1a64(socket))[..12].to_string()
}

type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

fn real_env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

fn key(env: Env) -> String {
    session_key(&env("HERDR_SOCKET_PATH").unwrap_or_default())
}

/// The per-session state dir: `$HERDR_PLUGIN_STATE_DIR` (herdr sets it),
/// else `$XDG_STATE_HOME/herdr-witch`, else `herdr-witch` in the XDG
/// default state dir under `$HOME`, or, with no `HOME`,
/// `$XDG_RUNTIME_DIR/herdr-witch/state`. `None` if none of those is set:
/// never a shared `/tmp` path.
pub fn state_dir_with(env: Env) -> Option<PathBuf> {
    let base = if let Some(d) = env("HERDR_PLUGIN_STATE_DIR") {
        PathBuf::from(d)
    } else if let Some(x) = env("XDG_STATE_HOME") {
        PathBuf::from(x).join("herdr-witch")
    } else if let Some(h) = env("HOME") {
        PathBuf::from(h)
            .join(".local")
            .join("state")
            .join("herdr-witch")
    } else {
        PathBuf::from(env("XDG_RUNTIME_DIR")?)
            .join("herdr-witch")
            .join("state")
    };
    Some(base.join(key(env)))
}

pub fn runtime_dir_with(env: Env) -> Option<PathBuf> {
    match env("XDG_RUNTIME_DIR") {
        Some(r) => Some(PathBuf::from(r).join("herdr-witch").join(key(env))),
        None => state_dir_with(env),
    }
}

pub fn config_path_with(env: Env) -> Option<PathBuf> {
    env("HERDR_PLUGIN_CONFIG_DIR").map(|d| PathBuf::from(d).join("config.toml"))
}

/// Exit 1 with a clear message: there is nowhere private to keep state.
fn no_dir() -> ! {
    eprintln!(
        "herdr-witch: no state directory: set HERDR_PLUGIN_STATE_DIR, XDG_STATE_HOME, HOME or XDG_RUNTIME_DIR"
    );
    std::process::exit(1);
}

/// `state_dir_with` for this process; exits 1 if there is none.
pub fn state_dir() -> PathBuf {
    state_dir_with(&real_env).unwrap_or_else(|| no_dir())
}

/// `runtime_dir_with` for this process; exits 1 if there is none.
pub fn runtime_dir() -> PathBuf {
    runtime_dir_with(&real_env).unwrap_or_else(|| no_dir())
}

pub fn config_path() -> Option<PathBuf> {
    config_path_with(&real_env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn state_paths_differ_per_socket() {
        let a = runtime_dir_with(&env(&[
            ("XDG_RUNTIME_DIR", "/run/u"),
            ("HERDR_SOCKET_PATH", "/s/a.sock"),
        ]));
        let b = runtime_dir_with(&env(&[
            ("XDG_RUNTIME_DIR", "/run/u"),
            ("HERDR_SOCKET_PATH", "/s/b.sock"),
        ]));
        assert_ne!(a, b);
        assert!(a.unwrap().starts_with("/run/u/herdr-witch"));
    }

    #[test]
    fn session_key_is_12_hex_and_stable() {
        let k = session_key("/s/a.sock");
        assert_eq!(k.len(), 12);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(k, session_key("/s/a.sock"));
        // FNV-1a 64 of "" is the offset basis cbf29ce484222325.
        assert_eq!(session_key(""), "cbf29ce48422");
    }

    #[test]
    fn state_dir_fallback_order() {
        let k = session_key("sock");
        let e = env(&[
            ("HERDR_PLUGIN_STATE_DIR", "/p"),
            ("XDG_STATE_HOME", "/x"),
            ("HOME", "/h"),
            ("HERDR_SOCKET_PATH", "sock"),
        ]);
        assert_eq!(state_dir_with(&e), Some(PathBuf::from(format!("/p/{k}"))));
        let e = env(&[
            ("XDG_STATE_HOME", "/x"),
            ("HOME", "/h"),
            ("HERDR_SOCKET_PATH", "sock"),
        ]);
        assert_eq!(
            state_dir_with(&e),
            Some(PathBuf::from(format!("/x/herdr-witch/{k}")))
        );
        let e = env(&[("HOME", "/h"), ("HERDR_SOCKET_PATH", "sock")]);
        assert_eq!(
            state_dir_with(&e),
            Some(
                PathBuf::from("/h")
                    .join(".local")
                    .join("state")
                    .join("herdr-witch")
                    .join(&k)
            )
        );
    }

    #[test]
    fn without_home_state_goes_to_runtime_dir_or_nowhere() {
        let k = session_key("sock");
        // Never a shared /tmp path.
        let e = env(&[("XDG_RUNTIME_DIR", "/run/u"), ("HERDR_SOCKET_PATH", "sock")]);
        assert_eq!(
            state_dir_with(&e),
            Some(PathBuf::from(format!("/run/u/herdr-witch/state/{k}")))
        );
        let e = env(&[("HERDR_SOCKET_PATH", "sock")]);
        assert_eq!(state_dir_with(&e), None);
        assert_eq!(runtime_dir_with(&e), None);
    }

    #[test]
    fn runtime_falls_back_to_state_dir() {
        let e = env(&[("HOME", "/h"), ("HERDR_SOCKET_PATH", "sock")]);
        assert_eq!(runtime_dir_with(&e), state_dir_with(&e));
    }

    #[test]
    fn config_path_needs_plugin_dir() {
        assert_eq!(config_path_with(&env(&[])), None);
        assert_eq!(
            config_path_with(&env(&[("HERDR_PLUGIN_CONFIG_DIR", "/c")])),
            Some(PathBuf::from("/c/config.toml"))
        );
    }
}
