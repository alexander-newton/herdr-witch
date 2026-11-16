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

/// A text sprite kept as characters; the scene that draws it maps the
/// letters to colours. `.` and space are empty.
#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    pub w: usize,
    pub h: usize,
    pub px: Vec<char>,
}

impl Pose {
    /// The letter at (`x`, `y`); `None` if empty or outside.
    pub fn at(&self, x: usize, y: usize) -> Option<char> {
        if x >= self.w || y >= self.h {
            return None;
        }
        Some(self.px[y * self.w + x]).filter(|c| !matches!(c, '.' | ' '))
    }
}

#[derive(Clone, Debug)]
pub struct Assets {
    pub big: Sprite,
    pub mini: Sprite,
    pub runes: Vec<Glyph>,
    /// The familiars' poses (`sigil::familiars`): the cat's four, the
    /// toad's two, the bat's two, and the top of the witch's head.
    pub cat: Vec<Pose>,
    pub toad: Vec<Pose>,
    pub bat: Vec<Pose>,
    pub bare: Pose,
}

const BIG_TXT: &str = include_str!("../../assets/witch_big.txt");
const MINI_TXT: &str = include_str!("../../assets/witch_mini.txt");
const RUNES_TXT: &str = include_str!("../../assets/runes.txt");
const CAT_TXT: &str = include_str!("../../assets/cat.txt");
const TOAD_TXT: &str = include_str!("../../assets/toad.txt");
const BAT_TXT: &str = include_str!("../../assets/bat.txt");
const BARE_TXT: &str = include_str!("../../assets/witch_bare.txt");

pub(crate) fn colour(c: char) -> Option<Rgb> {
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

/// Poses separated by blank lines, each padded to its widest row.
pub fn parse_poses(txt: &str) -> Vec<Pose> {
    let mut out = Vec::new();
    let mut rows: Vec<&str> = Vec::new();
    for line in txt.lines().map(str::trim_end).chain([""]) {
        if !line.is_empty() {
            rows.push(line);
        } else if !rows.is_empty() {
            let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
            let mut px = vec!['.'; w * rows.len()];
            for (y, r) in rows.iter().enumerate() {
                for (x, c) in r.chars().enumerate() {
                    px[y * w + x] = c;
                }
            }
            out.push(Pose {
                w,
                h: rows.len(),
                px,
            });
            rows.clear();
        }
    }
    out
}

/// The `count` poses in `<root>/assets/<name>`; the embedded ones if the
/// file is missing or doesn't hold exactly that many.
fn read_poses(root: Option<&Path>, name: &str, fallback: &'static str, count: usize) -> Vec<Pose> {
    let poses = parse_poses(&read(root, name, fallback));
    if poses.len() == count {
        poses
    } else {
        parse_poses(fallback)
    }
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
        cat: read_poses(root, "cat.txt", CAT_TXT, 4),
        toad: read_poses(root, "toad.txt", TOAD_TXT, 2),
        bat: read_poses(root, "bat.txt", BAT_TXT, 2),
        bare: read_poses(root, "witch_bare.txt", BARE_TXT, 1).remove(0),
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
    fn embedded_poses_parse() {
        let a = load_assets(None);
        assert_eq!(a.cat.len(), 4);
        assert!(a.cat.iter().all(|p| (p.w, p.h) == (12, 7)));
        assert_eq!(a.toad.len(), 2);
        assert!(a.toad.iter().all(|p| (p.w, p.h) == (9, 6)));
        assert_eq!(a.bat.len(), 2);
        assert_eq!((a.bare.w, a.bare.h), (9, 4));
        assert_eq!(a.cat[3].at(2, 2), Some('o'));
        assert_eq!(a.cat[3].at(0, 0), None);
        assert_eq!(a.cat[3].at(99, 0), None);
    }

    #[test]
    fn poses_pad_ragged_rows() {
        let p = parse_poses("AB\nA\n\n\nC\n");
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].w, p[0].h), (2, 2));
        assert_eq!(p[0].at(1, 1), None);
        assert_eq!(p[1].at(0, 0), Some('C'));
    }

    #[test]
    fn bad_pose_files_fall_back() {
        let root = std::env::temp_dir().join(format!("hw-poses-{}", std::process::id()));
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("assets/cat.txt"), "C\n").unwrap(); // one pose, not four
        std::fs::write(root.join("assets/toad.txt"), "").unwrap();
        let a = load_assets(Some(&root));
        assert_eq!(a.cat, load_assets(None).cat);
        assert_eq!(a.toad, load_assets(None).toad);
    }

    #[test]
    fn missing_root_falls_back() {
        let a = load_assets(Some(Path::new("/nonexistent-herdr-witch-root")));
        assert_eq!(a.runes.len(), 10);
    }
}
