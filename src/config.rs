use crate::paths;
use crate::sigil::fit::Mode;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub render: Mode,
    pub sigil_width_pct: u8,
    pub fps: u8,
    pub curse_level: u8,
    pub auto_open: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            render: Mode::Octant,
            sigil_width_pct: 15,
            fps: 12,
            curse_level: 4,
            auto_open: true,
        }
    }
}

impl Config {
    /// Clamp numeric fields to sane ranges.
    pub fn sanitized(self) -> Config {
        Config {
            fps: self.fps.clamp(1, 60),
            sigil_width_pct: self.sigil_width_pct.clamp(5, 50),
            curse_level: self.curse_level.min(4),
            ..self
        }
    }
}

/// Parse config text; on error return defaults plus the error message.
fn parse(text: &str) -> (Config, Option<String>) {
    match toml::from_str::<Config>(text) {
        Ok(c) => (c, None),
        Err(e) => (Config::default(), Some(e.to_string())),
    }
}

/// Load the config file; missing file or env gives defaults, a bad file logs one line.
pub fn load() -> Config {
    let Some(path) = paths::config_path() else {
        return Config::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Config::default();
    };
    let (cfg, err) = parse(&text);
    if let Some(e) = err {
        eprintln!(
            "herdr-witch: invalid config, using defaults: {}",
            e.replace('\n', " ")
        );
    }
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c, Config::default());
        assert_eq!(c.render, Mode::Octant);
        assert_eq!(
            (c.sigil_width_pct, c.fps, c.curse_level, c.auto_open),
            (15, 12, 4, true)
        );
    }

    #[test]
    fn config_partial() {
        let c: Config = toml::from_str("fps = 8").unwrap();
        assert_eq!(c.fps, 8);
        assert_eq!(Config { fps: 12, ..c }, Config::default());
    }

    #[test]
    fn render_modes_and_unknown_keys() {
        let c: Config = toml::from_str("render = \"halfblock\"\nbogus = 1").unwrap();
        assert_eq!(c.render, Mode::HalfBlock);
    }

    #[test]
    fn sanitized_clamps() {
        let c = Config {
            fps: 0,
            sigil_width_pct: 99,
            curse_level: 9,
            ..Config::default()
        }
        .sanitized();
        assert_eq!((c.fps, c.sigil_width_pct, c.curse_level), (1, 50, 4));
        let c = Config {
            fps: 200,
            sigil_width_pct: 0,
            ..Config::default()
        }
        .sanitized();
        assert_eq!((c.fps, c.sigil_width_pct), (60, 5));
        assert_eq!(Config::default().sanitized(), Config::default());
    }

    #[test]
    fn invalid_text_falls_back() {
        assert_eq!(parse("fps = [").0, Config::default());
        assert!(parse("fps = [").1.is_some());
    }
}
