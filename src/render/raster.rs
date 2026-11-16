use crate::palette::Rgb;

pub struct Buf {
    pub w: usize,
    pub h: usize,
    pub d: Vec<Rgb>,
}

impl Buf {
    pub fn new(w: usize, h: usize) -> Self {
        Buf {
            w,
            h,
            d: vec![[0.0; 3]; w * h],
        }
    }

    /// Make this a `w`×`h` buffer, keeping the allocation when the size is
    /// unchanged. The contents are unspecified until the next `clear`.
    pub fn reshape(&mut self, w: usize, h: usize) {
        if (self.w, self.h) != (w, h) {
            self.w = w;
            self.h = h;
            self.d.resize(w * h, [0.0; 3]);
        }
    }

    pub fn clear(&mut self, c: Rgb) {
        self.d.fill(c);
    }

    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            None
        } else {
            Some(y as usize * self.w + x as usize)
        }
    }

    pub fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if let Some(i) = self.idx(x, y) {
            self.d[i] = c;
        }
    }

    pub fn add(&mut self, x: i32, y: i32, c: Rgb, a: f32) {
        if let Some(i) = self.idx(x, y) {
            for k in 0..3 {
                self.d[i][k] += c[k] * a;
            }
        }
    }

    pub fn get(&self, x: usize, y: usize) -> Rgb {
        self.d[y * self.w + x]
    }

    pub fn to_u8(&self, x: usize, y: usize) -> [u8; 3] {
        self.get(x, y).map(|v| v.clamp(0.0, 255.0).round() as u8)
    }
}

/// Bresenham line, endpoints inclusive.
pub fn line_pts(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let (mut x, mut y) = (x0, y0);
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut e = dx + dy;
    let mut pts = Vec::new();
    loop {
        pts.push((x, y));
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * e;
        if e2 >= dy {
            e += dy;
            x += sx;
        }
        if e2 <= dx {
            e += dx;
            y += sy;
        }
    }
    pts
}

fn luma(c: Rgb) -> f32 {
    c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11
}

fn dist(a: Rgb, b: Rgb) -> f32 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum()
}

/// Reduce every 2x4 block to its darkest and brightest colour (terminal
/// cells show only two colours); each pixel snaps to the nearer one.
pub fn quant_octant(buf: &mut Buf) {
    let (w, h) = (buf.w, buf.h);
    for by in (0..h).step_by(4) {
        for bx in (0..w).step_by(2) {
            let mut idx = Vec::with_capacity(8);
            for y in by..(by + 4).min(h) {
                for x in bx..(bx + 2).min(w) {
                    idx.push(y * w + x);
                }
            }
            let (mut lo, mut hi) = (idx[0], idx[0]);
            for &i in &idx {
                if luma(buf.d[i]) < luma(buf.d[lo]) {
                    lo = i;
                }
                if luma(buf.d[i]) > luma(buf.d[hi]) {
                    hi = i;
                }
            }
            let (cl, ch) = (buf.d[lo], buf.d[hi]);
            for &i in &idx {
                let p = buf.d[i];
                buf.d[i] = if dist(p, cl) <= dist(p, ch) { cl } else { ch };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reshape_keeps_allocation_for_same_size() {
        let mut b = super::Buf::new(4, 3);
        let ptr = b.d.as_ptr();
        b.reshape(4, 3);
        assert_eq!(b.d.as_ptr(), ptr);
        b.reshape(2, 2);
        assert_eq!((b.w, b.h, b.d.len()), (2, 2, 4));
        b.reshape(5, 5);
        assert_eq!((b.w, b.h, b.d.len()), (5, 5, 25));
    }

    use super::*;
    use std::collections::HashSet;

    #[test]
    fn line_pts_endpoints_inclusive() {
        let p = line_pts(0, 0, 3, 1);
        assert_eq!(p.first(), Some(&(0, 0)));
        assert_eq!(p.last(), Some(&(3, 1)));
        assert_eq!(p.len(), 4);
    }

    #[test]
    fn add_and_clamp() {
        let mut b = Buf::new(2, 2);
        b.add(1, 1, [200.0, 0.0, 0.0], 1.0);
        b.add(1, 1, [200.0, 0.0, 0.0], 1.0);
        assert_eq!(b.to_u8(1, 1), [255, 0, 0]);
    }

    #[test]
    fn oob_ignored() {
        let mut b = Buf::new(2, 2);
        b.set(-1, 0, [1.0; 3]);
        b.set(2, 0, [1.0; 3]);
        b.set(0, 2, [1.0; 3]);
        b.add(-1, -1, [1.0; 3], 1.0);
        assert!(b.d.iter().all(|c| *c == [0.0; 3]));
    }

    #[test]
    fn quant_two_colours_per_block() {
        let mut b = Buf::new(4, 8);
        for y in 0..8 {
            for x in 0..4 {
                let (bx, by) = (x / 2, y / 4);
                let n = ((y % 4) * 2 + x % 2) as f32;
                let base = (by * 2 + bx) as f32 * 7.0;
                b.set(
                    x,
                    y,
                    [n * 30.0 + base, 40.0 + n * 5.0 + base, 200.0 - n * 20.0],
                );
            }
        }
        let orig = b.d.clone();
        quant_octant(&mut b);
        for by in 0..2 {
            for bx in 0..2 {
                let mut cols = HashSet::new();
                let mut ids = vec![];
                for y in by * 4..by * 4 + 4 {
                    for x in bx * 2..bx * 2 + 2 {
                        ids.push(y * 4 + x);
                        cols.insert(b.get(x, y).map(f32::to_bits));
                    }
                }
                assert!(cols.len() <= 2);
                let lo = ids
                    .iter()
                    .map(|&i| orig[i])
                    .min_by(|a, b| luma(*a).total_cmp(&luma(*b)))
                    .unwrap();
                let hi = ids
                    .iter()
                    .map(|&i| orig[i])
                    .max_by(|a, b| luma(*a).total_cmp(&luma(*b)))
                    .unwrap();
                assert!(cols.contains(&lo.map(f32::to_bits)));
                assert!(cols.contains(&hi.map(f32::to_bits)));
            }
        }
    }

    #[test]
    fn quant_partial_block() {
        let mut b = Buf::new(3, 5);
        for (i, c) in b.d.iter_mut().enumerate() {
            *c = [i as f32 * 10.0; 3];
        }
        quant_octant(&mut b);
    }
}
