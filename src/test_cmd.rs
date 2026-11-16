//! `herdr-witch test`: a rendering check, opened in a popup by the "Test
//! rendering" action. It prints the palette, the same sigil drawn with
//! half-blocks and with octants, all 256 octant glyphs, a cursed line, and
//! how to fall back to half-blocks.
//!
//! The output flows like any command's (no cursor positioning), and each
//! line resets its colours. herdr closes a popup the moment its process
//! exits (probed on 0.9.5), so `run` waits for Enter.

use crate::grimoire::GRIMOIRE;
use crate::grimoire::curse::curse;
use crate::grimoire::pool::LinePool;
use crate::ipc::GlobalState;
use crate::palette::{self, Rgb};
use crate::render::encode::{OCTANT, encode_halfblock, encode_octant};
use crate::render::raster::Buf;
use crate::sigil::anim::SigilAnim;
use crate::sigil::fit::Mode;
use crate::sigil::sprites::load_assets;
use crate::sigil::{GeoCache, draw_frame};
use std::fmt::Write;

const SIGIL_COLS: usize = 24;
/// Tall enough for the sigil plus the witch in both modes.
const SIGIL_ROWS: usize = 19;
/// Two sigils with a 2-column gap.
const PAIR_COLS: usize = 2 * SIGIL_COLS + 2;
/// 16 glyphs a row, space-separated.
const TABLE_COLS: usize = 31;
const TABLE_GAP: usize = 4;
const RESET: &str = "\x1b[0m";

const PALETTE: [(&str, Rgb); 9] = [
    ("bg", palette::BG),
    ("deep", palette::DEEP),
    ("halo", palette::HALO),
    ("violet", palette::VIOLET),
    ("orchid", palette::ORCHID),
    ("gold", palette::GOLD),
    ("bright", palette::BRIGHT),
    ("cyan", palette::CYAN),
    ("white", palette::WHITE),
];

/// A truecolour SGR: `layer` 38 is the foreground, 48 the background.
fn sgr(layer: u8, c: Rgb) -> String {
    format!(
        "\x1b[{layer};2;{};{};{}m",
        c[0] as u8, c[1] as u8, c[2] as u8
    )
}

/// The sigil (with the witch, arms raised, two agents working) as
/// self-contained rows: each sets its own colours and ends with a reset.
fn sigil_rows(mode: Mode, seed: u64) -> Vec<String> {
    let assets = load_assets(None);
    let mut anim = SigilAnim::new(seed);
    let mut cache = GeoCache::default();
    let st = GlobalState {
        working: 2,
        typing: true,
        written_ms: 0,
        visible_tabs: Vec::new(),
    };
    let mut buf = Buf::new(1, 1);
    for _ in 0..12 {
        buf = draw_frame(
            SIGIL_COLS, SIGIL_ROWS, mode, &mut anim, &assets, &mut cache, &st,
        )
        .expect("the sigil fits 24x19");
    }
    (0..SIGIL_ROWS)
        .map(|r| {
            let row = match mode {
                Mode::HalfBlock => encode_halfblock(&buf, 0, 2 * r, SIGIL_COLS, 1),
                Mode::Octant => encode_octant(&buf, 0, 4 * r, SIGIL_COLS, 1),
            };
            // Each encoding starts by moving to its row 1; flowing output
            // doesn't need it.
            format!("{}{RESET}", row.trim_start_matches("\x1b[1;1H"))
        })
        .collect()
}

/// The 256 octants in 16 rows of 16, bright on deep violet.
fn table_rows() -> Vec<String> {
    OCTANT
        .chunks(16)
        .map(|row| {
            let glyphs: Vec<String> = row.iter().map(char::to_string).collect();
            format!(
                "{}{}{}{RESET}",
                sgr(38, palette::BRIGHT),
                sgr(48, palette::DEEP),
                glyphs.join(" ")
            )
        })
        .collect()
}

/// The whole report for a terminal `cols` wide. The octant table sits
/// beside the sigils when there is room, below them otherwise.
pub fn render(cols: usize, config_path: &str, seed: u64) -> String {
    let side_by_side = cols >= PAIR_COLS + TABLE_GAP + TABLE_COLS;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}✶ herdr-witch rendering test{RESET}",
        sgr(38, palette::GOLD)
    );
    // Palette swatches, named underneath.
    let w = if cols >= 9 * 7 { 6 } else { 5 };
    let swatches: Vec<String> = PALETTE
        .iter()
        .map(|&(_, c)| format!("{}{:w$}{RESET}", sgr(48, c), ""))
        .collect();
    let _ = writeln!(out, "{}", swatches.join(" "));
    let names: Vec<String> = PALETTE
        .iter()
        .map(|(n, _)| format!("{:w$}", &n[..n.len().min(w)]))
        .collect();
    let _ = writeln!(out, "{}", names.join(" ").trim_end());
    out.push('\n');

    let (hb, oct) = (
        sigil_rows(Mode::HalfBlock, seed),
        sigil_rows(Mode::Octant, seed),
    );
    let table = table_rows();
    let mut head = format!("{:<w$}octant", "half-block", w = SIGIL_COLS + 2);
    if side_by_side {
        let pad = PAIR_COLS + TABLE_GAP - head.len();
        let _ = write!(head, "{:pad$}octant table", "");
    }
    let _ = writeln!(out, "{head}");
    for r in 0..SIGIL_ROWS {
        let _ = write!(out, "{}  {}", hb[r], oct[r]);
        if let Some(t) = table.get(r).filter(|_| side_by_side) {
            let _ = write!(out, "{:pad$}{t}", "", pad = TABLE_GAP);
        }
        out.push('\n');
    }
    out.push('\n');
    if !side_by_side {
        out.push_str("octant table\n");
        for t in &table {
            let _ = writeln!(out, "{t}");
        }
        out.push('\n');
    }

    let label = "curse level 4: ";
    let pool = LinePool::from_text(GRIMOIRE, seed);
    let line = pool.fit_line(0, cols.saturating_sub(label.len()).max(1));
    let cursed = curse(&line, 4, &mut fastrand::Rng::with_seed(seed));
    let _ = writeln!(out, "{label}{}{cursed}{RESET}", sgr(38, palette::ORCHID));
    let _ = writeln!(
        out,
        "If the octant sigil looks broken, set render = \"halfblock\" in {config_path}"
    );
    out
}

/// Print the report for the current terminal and wait for Enter (only when
/// stdin is a terminal, so piping it still works).
pub fn run() {
    use std::io::{IsTerminal, Write as _};
    let cols = match crate::sigil::term_size() {
        (0, _) => 80,
        (c, _) => c,
    };
    let path = crate::paths::config_path().map_or_else(
        || "config.toml in `herdr plugin config-dir herdr-witch`".to_string(),
        |p| p.display().to_string(),
    );
    let seed = crate::ipc::now_ms();
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(render(cols, &path, seed).as_bytes());
    if std::io::stdin().is_terminal() {
        let _ = write!(stdout, "\nPress Enter to close.");
        let _ = stdout.flush();
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grimoire::curse::display_width;

    /// Text without escape sequences.
    fn plain(s: &str) -> String {
        let mut out = String::new();
        let mut it = s.chars();
        while let Some(c) = it.next() {
            if c == '\x1b' {
                for d in it.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn test_output_has_every_part() {
        for cols in [100, 60] {
            let out = render(cols, "/cfg/config.toml", 7);
            let text = plain(&out);
            for (i, ch) in OCTANT.iter().enumerate() {
                assert!(text.contains(*ch), "cols {cols}: octant {i} missing");
            }
            for c in [
                "13;6;20",
                "42;15;69",
                "58;20;96",
                "107;47;163",
                "170;90;220",
            ] {
                assert!(out.contains(&format!("48;2;{c}m")), "palette {c}");
            }
            for c in ["245;197;66", "255;232;150", "79;240;255", "235;250;255"] {
                assert!(out.contains(&format!("48;2;{c}m")), "palette {c}");
            }
            assert!(text.contains(
                "If the octant sigil looks broken, set render = \"halfblock\" in /cfg/config.toml"
            ));
            assert!(text.contains("half-block") && text.contains("octant"));
            // The curse line carries stacked combining marks.
            assert!(text.chars().any(|c| ('\u{300}'..='\u{36f}').contains(&c)));
            // Flowing output: no cursor positioning, and every coloured line
            // resets and fits the terminal (plain text may simply wrap).
            assert!(!out.contains(";1H"), "cursor moves");
            for line in out.lines().filter(|l| l.contains("\x1b[")) {
                assert!(line.ends_with("\x1b[0m"), "{line:?}");
                assert!(display_width(&plain(line)) <= cols, "cols {cols}: {line:?}");
            }
        }
    }

    #[test]
    fn test_output_fits_popup_height() {
        // Side by side, the report fits an 80% popup of a 45-row window.
        let wide = render(100, "/c", 1);
        assert!(wide.lines().count() <= 32, "{}", wide.lines().count());
        // Too narrow for that: the table goes below the sigils.
        assert!(render(60, "/c", 1).lines().count() > wide.lines().count());
    }
}
