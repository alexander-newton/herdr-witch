use crate::palette::Rgb;
use std::path::Path;

/// Lit-pixel offsets within a 3x3 rune.
pub type Glyph = Vec<(i32, i32)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WitchSize {
    Big,
    Mini,
}

#[derive(Clone, Debug)]
pub struct Sprite {
    pub w: usize,
    pub h: usize,
    pub px: Vec<Option<Rgb>>,
    pub shoulders: [(i32, i32); 2],
    pub arm_len: f32,
    pub arm_thick: u8,
    /// Height of the canvas the sprite is placed on (arms may extend past `h`).
    pub canvas_h: usize,
}

#[derive(Clone, Debug)]
pub struct Assets {
    pub big: Sprite,
    pub mini: Sprite,
    pub runes: Vec<Glyph>,
}

const BIG_TXT: &str = include_str!("../../assets/witch_big.txt");
const MINI_TXT: &str = include_str!("../../assets/witch_mini.txt");
const RUNES_TXT: &str = include_str!("../../assets/runes.txt");

fn colour(c: char) -> Option<Rgb> {
    match c {
        'H' => Some([31.0, 11.0, 51.0]),
        'h' => Some([107.0, 47.0, 163.0]),
        'B' => Some([201.0, 154.0, 46.0]),
        'K' => Some([74.0, 24.0, 64.0]),
        'R' => Some([42.0, 15.0, 69.0]),
        'r' => Some([69.0, 32.0, 122.0]),
        'G' => Some([245.0, 197.0, 66.0]),
        _ => None,
    }
}

fn parse_sprite(
    txt: &str,
    shoulders: [(i32, i32); 2],
    arm_len: f32,
    arm_thick: u8,
    canvas_h: usize,
) -> Sprite {
    let rows: Vec<&str> = txt.lines().filter(|l| !l.is_empty()).collect();
    let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
    let mut px = vec![None; w * rows.len()];
    for (y, r) in rows.iter().enumerate() {
        for (x, c) in r.chars().enumerate() {
            px[y * w + x] = colour(c);
        }
    }
    Sprite {
        w,
        h: rows.len(),
        px,
        shoulders,
        arm_len,
        arm_thick,
        canvas_h,
    }
}

fn parse_runes(txt: &str) -> Vec<Glyph> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let flush = |cur: &mut Vec<&str>, out: &mut Vec<Glyph>| {
        if cur.is_empty() {
            return;
        }
        let mut g = Glyph::new();
        for (y, r) in cur.iter().enumerate() {
            for (x, c) in r.chars().enumerate() {
                if c == 'X' {
                    g.push((x as i32, y as i32));
                }
            }
        }
        out.push(g);
        cur.clear();
    };
    for line in txt.lines() {
        if line.trim().is_empty() {
            flush(&mut cur, &mut out);
        } else {
            cur.push(line.trim());
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn read(root: Option<&Path>, name: &str, fallback: &'static str) -> String {
    root.and_then(|r| std::fs::read_to_string(r.join("assets").join(name)).ok())
        .unwrap_or_else(|| fallback.to_string())
}

/// Load sprites and runes from `<root>/assets/*.txt`, falling back to the
/// copies embedded in the binary.
pub fn load_assets(root: Option<&Path>) -> Assets {
    Assets {
        big: parse_sprite(
            &read(root, "witch_big.txt", BIG_TXT),
            [(3, 11), (13, 11)],
            8.0,
            2,
            24,
        ),
        mini: parse_sprite(
            &read(root, "witch_mini.txt", MINI_TXT),
            [(1, 6), (7, 6)],
            4.0,
            1,
            13,
        ),
        runes: parse_runes(&read(root, "runes.txt", RUNES_TXT)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_assets_parse() {
        let a = load_assets(None);
        assert_eq!((a.big.w, a.big.h, a.big.canvas_h), (17, 22, 24));
        assert_eq!((a.mini.w, a.mini.h, a.mini.canvas_h), (9, 11, 13));
        assert_eq!(a.runes.len(), 10);
        assert_eq!(a.runes[0], vec![(0, 0), (2, 0), (1, 1), (0, 2), (2, 2)]);
        assert_eq!(a.big.px[8], Some([31.0, 11.0, 51.0]));
        assert_eq!(a.big.px[0], None);
        assert_eq!(a.big.shoulders, [(3, 11), (13, 11)]);
        assert_eq!(a.mini.arm_thick, 1);
    }

    #[test]
    fn missing_root_falls_back() {
        let a = load_assets(Some(Path::new("/nonexistent-herdr-witch-root")));
        assert_eq!(a.runes.len(), 10);
    }
}
