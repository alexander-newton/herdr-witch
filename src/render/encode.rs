use crate::render::raster::Buf;

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

/// Append `n` in decimal. The encoders write thousands of numbers a frame,
/// and this is several times cheaper than `write!`.
fn push_num(out: &mut String, n: usize) {
    if n >= 10 {
        push_num(out, n / 10);
    }
    out.push((b'0' + (n % 10) as u8) as char);
}

/// Move the cursor to the 0-based cell (`row`, `col`).
fn cup(out: &mut String, row: usize, col: usize) {
    out.push_str("\x1b[");
    push_num(out, row + 1);
    out.push(';');
    push_num(out, col + 1);
    out.push('H');
}

/// Tracks the SGR colours last written so unchanged ones are skipped.
#[derive(Default)]
struct Sgr {
    fg: Option<Px>,
    bg: Option<Px>,
}

impl Sgr {
    fn colour(out: &mut String, lead: &str, c: Px) {
        out.push_str(lead);
        push_num(out, c[0] as usize);
        out.push(';');
        push_num(out, c[1] as usize);
        out.push(';');
        push_num(out, c[2] as usize);
        out.push('m');
    }

    fn fg(&mut self, out: &mut String, c: Px) {
        if self.fg != Some(c) {
            Self::colour(out, "\x1b[38;2;", c);
            self.fg = Some(c);
        }
    }

    fn bg(&mut self, out: &mut String, c: Px) {
        if self.bg != Some(c) {
            Self::colour(out, "\x1b[48;2;", c);
            self.bg = Some(c);
        }
    }
}

/// Whether the `bw`x`bh` pixel block at (`x0`, `y0`) is the same in both
/// buffers (which must be the same size). Pixels outside them never change.
fn same_block(a: &Buf, b: &Buf, x0: usize, y0: usize, bw: usize, bh: usize) -> bool {
    let (x0, x1) = (x0.min(a.w), (x0 + bw).min(a.w));
    (y0..(y0 + bh).min(a.h))
        .all(|y| a.d[y * a.w + x0..y * a.w + x1] == b.d[y * b.w + x0..y * b.w + x1])
}

/// Walks a frame's cells, skipping those whose pixel block is unchanged
/// since `prev` and placing the cursor before each run of changed ones.
/// With no `prev` (or one of another size) every cell is written, each row
/// starting with a cursor move to its first column.
struct Cells<'a> {
    buf: &'a Buf,
    prev: Option<&'a Buf>,
    /// Pixels per cell.
    block: (usize, usize),
    /// The cursor sits just after the last cell written.
    placed: bool,
}

impl<'a> Cells<'a> {
    fn new(buf: &'a Buf, prev: Option<&'a Buf>, block: (usize, usize)) -> Self {
        Cells {
            buf,
            prev: prev.filter(|p| (p.w, p.h) == (buf.w, buf.h)),
            block,
            placed: false,
        }
    }

    fn row_start(&mut self) {
        self.placed = false;
    }

    /// `false` if the cell at (`r`, `c`), whose block starts at pixel
    /// (`x`, `y`), needs no drawing; else the cursor is put there.
    fn begin(&mut self, out: &mut String, (r, c): (usize, usize), (x, y): (usize, usize)) -> bool {
        let (bw, bh) = self.block;
        if self
            .prev
            .is_some_and(|p| same_block(self.buf, p, x, y, bw, bh))
        {
            self.placed = false;
            return false;
        }
        if !self.placed {
            cup(out, r, c);
            self.placed = true;
        }
        true
    }
}

/// Half-block encoding: each cell is `▀`, fg = pixel (x, 2y), bg = pixel (x, 2y+1).
/// Pixels outside the buffer are black.
pub fn encode_halfblock(buf: &Buf, ox: usize, oy: usize, cols: usize, rows: usize) -> String {
    let mut out = String::new();
    encode_halfblock_into(&mut out, buf, None, ox, oy, cols, rows);
    out
}

/// `encode_halfblock` into `out` (cleared first), reusing its allocation.
/// Given the previous frame's pixels as `prev`, only the cells that differ
/// from it are written.
pub fn encode_halfblock_into(
    out: &mut String,
    buf: &Buf,
    prev: Option<&Buf>,
    ox: usize,
    oy: usize,
    cols: usize,
    rows: usize,
) {
    out.clear();
    let mut sgr = Sgr::default();
    let mut cells = Cells::new(buf, prev, (1, 2));
    for r in 0..rows {
        cells.row_start();
        for c in 0..cols {
            let (x, y) = (ox + c, oy + 2 * r);
            if !cells.begin(out, (r, c), (x, y)) {
                continue;
            }
            let top = px(buf, x, y).unwrap_or([0; 3]);
            let bot = px(buf, x, y + 1).unwrap_or([0; 3]);
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
    encode_octant_into(&mut out, buf, None, ox, oy, cols, rows);
    out
}

/// `encode_octant` into `out` (cleared first), reusing its allocation. No
/// per-cell allocation: the block's pixels sit in a fixed array. Given the
/// previous frame's pixels as `prev`, only the cells that differ from it
/// are written.
pub fn encode_octant_into(
    out: &mut String,
    buf: &Buf,
    prev: Option<&Buf>,
    ox: usize,
    oy: usize,
    cols: usize,
    rows: usize,
) {
    out.clear();
    let mut sgr = Sgr::default();
    let mut cells = Cells::new(buf, prev, (2, 4));
    for r in 0..rows {
        cells.row_start();
        for c in 0..cols {
            let (x, y) = (ox + 2 * c, oy + 4 * r);
            if !cells.begin(out, (r, c), (x, y)) {
                continue;
            }
            let block: [Option<Px>; 8] = std::array::from_fn(|i| px(buf, x + i % 2, y + i / 2));
            let mut inside = block.iter().flatten().copied();
            let first = inside.next().unwrap_or([0; 3]);
            let (mut fg, mut bg) = (first, first);
            let (mut lfg, mut lbg) = (luma(first), luma(first));
            for p in inside {
                let l = luma(p);
                if l > lfg {
                    (fg, lfg) = (p, l);
                }
                if l < lbg {
                    (bg, lbg) = (p, l);
                }
            }
            if fg == bg {
                sgr.bg(out, bg);
                out.push(' ');
                continue;
            }
            let mut bits = 0usize;
            for (i, p) in block.iter().enumerate() {
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
        encode_octant_into(&mut out, &b, None, 0, 0, 4, 3);
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
        encode_halfblock_into(&mut out, &b, None, 0, 0, 4, 3);
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

    /// Cells written by a frame body, as (row, col), 1-based.
    fn cells_written(s: &str) -> Vec<(usize, usize)> {
        let mut cells = Vec::new();
        let (mut row, mut col) = (0, 0);
        let mut it = s.chars();
        while let Some(ch) = it.next() {
            if ch != '\x1b' {
                cells.push((row, col));
                col += 1;
                continue;
            }
            assert_eq!(it.next(), Some('['));
            let seq: String = it
                .by_ref()
                .take_while(|c| !c.is_ascii_alphabetic())
                .collect();
            // take_while ate the final byte; a CUP has exactly one ';'.
            if let Some((r, c)) = seq.split_once(';')
                && !c.contains(';')
            {
                (row, col) = (r.parse().unwrap(), c.parse().unwrap());
            }
        }
        cells
    }

    #[test]
    fn diff_writes_only_changed_cells() {
        type Enc = fn(&mut String, &Buf, Option<&Buf>, usize, usize, usize, usize);
        let encoders: [(Enc, usize, usize); 2] =
            [(encode_octant_into, 2, 4), (encode_halfblock_into, 1, 2)];
        for (enc, bw, bh) in encoders {
            let (cols, rows) = (5, 3);
            let prev = gradient(cols * bw, rows * bh);
            let mut out = String::new();
            enc(&mut out, &prev, Some(&prev), 0, 0, cols, rows);
            assert_eq!(out, "", "an unchanged frame writes nothing");
            // Change one pixel in cells (row 1, col 0), (1, 1) and (2, 4).
            let mut next = gradient(cols * bw, rows * bh);
            for (r, c) in [(0, 0), (0, 1), (1, 4)] {
                next.set((c * bw) as i32, (r * bh) as i32, [255.0; 3]);
            }
            enc(&mut out, &next, Some(&prev), 0, 0, cols, rows);
            assert_eq!(cells_written(&out), [(1, 1), (1, 2), (2, 5)]);
            assert_eq!(out.matches('H').count(), 2, "one cursor move per run");
            let mut full = String::new();
            enc(&mut full, &next, None, 0, 0, cols, rows);
            assert_eq!(cells_written(&full).len(), cols * rows);
            // A previous frame of another size is ignored.
            enc(&mut out, &next, Some(&Buf::new(1, 1)), 0, 0, cols, rows);
            assert_eq!(out, full);
        }
    }
}
