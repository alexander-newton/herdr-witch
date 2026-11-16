use crate::render::raster::Buf;
use std::fmt::Write;

pub use crate::render::octant_table::OCTANT;

type Px = [u8; 3];

fn px(buf: &Buf, x: usize, y: usize) -> Option<Px> {
    (x < buf.w && y < buf.h).then(|| buf.to_u8(x, y))
}

fn luma(c: Px) -> f32 {
    0.3 * c[0] as f32 + 0.59 * c[1] as f32 + 0.11 * c[2] as f32
}

fn dist2(a: Px, b: Px) -> i32 {
    (0..3).map(|i| (a[i] as i32 - b[i] as i32).pow(2)).sum()
}

/// Tracks the SGR colours last written so unchanged ones are skipped.
#[derive(Default)]
struct Sgr {
    fg: Option<Px>,
    bg: Option<Px>,
}

impl Sgr {
    fn fg(&mut self, out: &mut String, c: Px) {
        if self.fg != Some(c) {
            let _ = write!(out, "\x1b[38;2;{};{};{}m", c[0], c[1], c[2]);
            self.fg = Some(c);
        }
    }

    fn bg(&mut self, out: &mut String, c: Px) {
        if self.bg != Some(c) {
            let _ = write!(out, "\x1b[48;2;{};{};{}m", c[0], c[1], c[2]);
            self.bg = Some(c);
        }
    }
}

/// Half-block encoding: each cell is `▀`, fg = pixel (x, 2y), bg = pixel (x, 2y+1).
/// Pixels outside the buffer are black.
pub fn encode_halfblock(buf: &Buf, ox: usize, oy: usize, cols: usize, rows: usize) -> String {
    let mut out = String::new();
    encode_halfblock_into(&mut out, buf, ox, oy, cols, rows);
    out
}

/// `encode_halfblock` into `out` (cleared first), reusing its allocation.
pub fn encode_halfblock_into(
    out: &mut String,
    buf: &Buf,
    ox: usize,
    oy: usize,
    cols: usize,
    rows: usize,
) {
    out.clear();
    let mut sgr = Sgr::default();
    for r in 0..rows {
        let _ = write!(out, "\x1b[{};1H", r + 1);
        for c in 0..cols {
            let top = px(buf, ox + c, oy + 2 * r).unwrap_or([0; 3]);
            let bot = px(buf, ox + c, oy + 2 * r + 1).unwrap_or([0; 3]);
            sgr.fg(out, top);
            sgr.bg(out, bot);
            out.push('▀');
        }
    }
}

/// Octant encoding: each 2x4 pixel block becomes one character. The brightest
/// colour is the foreground and the darkest the background; each pixel nearer
/// the foreground sets its bit. Pixels outside the buffer take the background.
pub fn encode_octant(buf: &Buf, ox: usize, oy: usize, cols: usize, rows: usize) -> String {
    let mut out = String::new();
    encode_octant_into(&mut out, buf, ox, oy, cols, rows);
    out
}

/// `encode_octant` into `out` (cleared first), reusing its allocation. No
/// per-cell allocation: the block's pixels sit in a fixed array.
pub fn encode_octant_into(
    out: &mut String,
    buf: &Buf,
    ox: usize,
    oy: usize,
    cols: usize,
    rows: usize,
) {
    out.clear();
    let mut sgr = Sgr::default();
    for r in 0..rows {
        let _ = write!(out, "\x1b[{};1H", r + 1);
        for c in 0..cols {
            let cells: [Option<Px>; 8] =
                std::array::from_fn(|i| px(buf, ox + 2 * c + i % 2, oy + 4 * r + i / 2));
            let mut inside = cells.iter().flatten().copied();
            let first = inside.next().unwrap_or([0; 3]);
            let (mut fg, mut bg) = (first, first);
            for p in inside {
                if luma(p) > luma(fg) {
                    fg = p;
                }
                if luma(p) < luma(bg) {
                    bg = p;
                }
            }
            if fg == bg {
                sgr.bg(out, bg);
                out.push(' ');
                continue;
            }
            let mut bits = 0usize;
            for (i, p) in cells.iter().enumerate() {
                if let Some(p) = p
                    && dist2(*p, fg) < dist2(*p, bg)
                {
                    bits |= 1 << i;
                }
            }
            sgr.fg(out, fg);
            sgr.bg(out, bg);
            out.push(OCTANT[bits]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::raster::Buf;

    fn gradient(w: usize, h: usize) -> Buf {
        let mut b = Buf::new(w, h);
        for y in 0..h {
            for x in 0..w {
                b.set(x as i32, y as i32, [(x * 40) as f32, (y * 30) as f32, 90.0]);
            }
        }
        b
    }

    #[test]
    fn octant_into_matches_and_reuses_string() {
        let b = gradient(6, 8);
        let want = encode_octant(&b, 0, 0, 4, 3);
        let mut out = String::with_capacity(4096);
        out.push_str("stale");
        let ptr = out.as_ptr();
        encode_octant_into(&mut out, &b, 0, 0, 4, 3);
        assert_eq!(out, want);
        assert_eq!(out.as_ptr(), ptr, "no reallocation within capacity");
    }

    #[test]
    fn halfblock_into_matches_and_reuses_string() {
        let b = gradient(4, 6);
        let want = encode_halfblock(&b, 0, 0, 4, 3);
        let mut out = String::with_capacity(4096);
        out.push_str("stale");
        let ptr = out.as_ptr();
        encode_halfblock_into(&mut out, &b, 0, 0, 4, 3);
        assert_eq!(out, want);
        assert_eq!(out.as_ptr(), ptr);
    }

    #[test]
    fn octant_table_complete() {
        let mut s: Vec<char> = OCTANT.to_vec();
        s.sort();
        s.dedup();
        assert_eq!(s.len(), 256);
        assert_eq!(OCTANT[0], ' ');
        assert_eq!(OCTANT[255], '█');
    }

    #[test]
    fn octant_known_chars() {
        assert_eq!(OCTANT[0b0000_0101], '▘');
        assert_eq!(OCTANT[0b0101_0101], '▌');
        assert_eq!(OCTANT[0b0000_1111], '▀');
    }

    #[test]
    fn halfblock_one_cell() {
        let mut b = Buf::new(1, 2);
        b.set(0, 0, [255.0, 0.0, 0.0]);
        b.set(0, 1, [0.0, 0.0, 255.0]);
        assert_eq!(
            encode_halfblock(&b, 0, 0, 1, 1),
            "\x1b[1;1H\x1b[38;2;255;0;0m\x1b[48;2;0;0;255m▀"
        );
    }

    #[test]
    fn halfblock_sgr_dedup() {
        let mut b = Buf::new(3, 2);
        b.clear([10.0, 20.0, 30.0]);
        let s = encode_halfblock(&b, 0, 0, 3, 1);
        assert_eq!(s.matches("38;2;").count(), 1);
        assert_eq!(s.matches("48;2;").count(), 1);
    }

    #[test]
    fn octant_uniform_block_is_space() {
        let mut b = Buf::new(2, 4);
        b.clear([5.0, 6.0, 7.0]);
        let s = encode_octant(&b, 0, 0, 1, 1);
        assert_eq!(s, "\x1b[1;1H\x1b[48;2;5;6;7m ");
    }

    #[test]
    fn octant_pattern_bits() {
        // upper-left quadrant bright (cells 1 and 3)
        let mut b = Buf::new(2, 4);
        b.set(0, 0, [255.0; 3]);
        b.set(0, 1, [255.0; 3]);
        let s = encode_octant(&b, 0, 0, 1, 1);
        assert!(s.ends_with('▘'), "{s:?}");
        assert!(s.contains("38;2;255;255;255"));
    }
}
