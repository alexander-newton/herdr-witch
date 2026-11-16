use super::sprites::Glyph;
use crate::render::raster::line_pts;
use std::collections::HashSet;
use std::f64::consts::PI;

/// JS `Math.round` (half rounds up).
fn jsround(v: f64) -> i32 {
    (v + 0.5).floor() as i32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ring,
    Star,
}

#[derive(Clone, Debug)]
pub struct SigilGeometry {
    pub n: usize,
    pub cx: f32,
    pub cy: f32,
    pub verts: [(f32, f32); 5],
    pub core: Vec<(i32, i32, Kind)>,
    pub halo: Vec<(i32, i32)>,
    pub runes: Vec<Vec<(i32, i32)>>,
    pub eye: Vec<(i32, i32)>,
}

pub fn build(n: usize, runes: &[Glyph]) -> SigilGeometry {
    let nf = n as f64;
    let (cx, cy) = ((nf - 1.0) / 2.0, (nf - 1.0) / 2.0);
    let r1 = nf * 0.466;
    let r2 = nf * if n >= 30 { 0.352 } else { 0.33 };
    let in_bounds = |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < n && (y as usize) < n;

    let mut core: Vec<(i32, i32, Kind)> = Vec::new();
    let mut seen: HashSet<(i32, i32)> = HashSet::new();
    let mut add = |x: i32, y: i32, k: Kind| {
        if in_bounds(x, y) && seen.insert((x, y)) {
            core.push((x, y, k));
        }
    };
    let rings: &[f64] = if r1 - r2 >= 1.6 { &[r1, r2] } else { &[r1] };
    for &r in rings {
        let step = 0.4 / r.max(1.0);
        let mut i = 0usize;
        loop {
            let a = i as f64 * step;
            if a >= 2.0 * PI {
                break;
            }
            add(
                jsround(cx + r * a.cos()),
                jsround(cy + r * a.sin()),
                Kind::Ring,
            );
            i += 1;
        }
    }
    let verts_f: [(f64, f64); 5] = std::array::from_fn(|k| {
        let a = -PI / 2.0 + k as f64 * 2.0 * PI / 5.0;
        (cx + r2 * a.cos(), cy + r2 * a.sin())
    });
    for k in 0..5 {
        let (a, b) = (verts_f[k], verts_f[(k + 2) % 5]);
        for (x, y) in line_pts(jsround(a.0), jsround(a.1), jsround(b.0), jsround(b.1)) {
            add(x, y, Kind::Star);
        }
    }

    let band = r1 - r2;
    let rr = (r1 + r2) / 2.0;
    let mut rune_px = Vec::new();
    for k in 0..10usize {
        let a = -PI / 2.0 + k as f64 * PI / 5.0 + PI / 10.0;
        let mut px = Vec::new();
        if band >= 4.5 {
            let ox = jsround(cx + rr * a.cos()) - 1;
            let oy = jsround(cy + rr * a.sin()) - 1;
            if let Some(g) = runes.get(k) {
                px.extend(g.iter().map(|&(i, j)| (ox + i, oy + j)));
            }
        } else if band >= 2.5 {
            px.push((jsround(cx + rr * a.cos()), jsround(cy + rr * a.sin())));
        }
        if !px.is_empty() {
            rune_px.push(px);
        }
    }

    let (c0x, c0y) = (jsround(cx), jsround(cy));
    let eye = if n >= 30 {
        [(-1, 0), (0, 0), (1, 0), (0, -1), (0, 1)]
            .iter()
            .map(|&(dx, dy)| (c0x + dx, c0y + dy))
            .collect()
    } else {
        vec![(c0x, c0y)]
    };

    let mut halo = Vec::new();
    let mut hs: HashSet<(i32, i32)> = HashSet::new();
    for &(x, y, _) in &core {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let p = (x + dx, y + dy);
            if !seen.contains(&p) && in_bounds(p.0, p.1) && hs.insert(p) {
                halo.push(p);
            }
        }
    }

    SigilGeometry {
        n,
        cx: cx as f32,
        cy: cy as f32,
        verts: verts_f.map(|(x, y)| (x as f32, y as f32)),
        core,
        halo,
        runes: rune_px,
        eye,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sigil::sprites::load_assets;

    fn g(n: usize) -> SigilGeometry {
        build(n, &load_assets(None).runes)
    }

    #[test]
    fn inner_ring_dropped_small() {
        let geo = g(8);
        let r2 = 8.0 * 0.33;
        let near = |x: i32, y: i32| {
            let d = ((x as f32 - geo.cx).powi(2) + (y as f32 - geo.cy).powi(2)).sqrt();
            (d - r2).abs() < 0.3
        };
        assert!(
            !geo.core
                .iter()
                .any(|&(x, y, k)| k == Kind::Ring && near(x, y))
        );
        // and a big one does have ring pixels at R2
        let big = g(54);
        let r2 = 54.0 * 0.352;
        assert!(big.core.iter().any(|&(x, y, k)| {
            let d = ((x as f32 - big.cx).powi(2) + (y as f32 - big.cy).powi(2)).sqrt();
            k == Kind::Ring && (d - r2).abs() < 0.6
        }));
    }

    #[test]
    fn rune_modes() {
        assert!(g(16).runes.is_empty());
        let m = g(27);
        assert_eq!(m.runes.len(), 10);
        assert!(m.runes.iter().all(|r| r.len() == 1));
        let b = g(54);
        assert_eq!(b.runes.len(), 10);
        assert!(b.runes.iter().all(|r| r.len() >= 3));
    }

    #[test]
    fn eye_sizes() {
        assert_eq!(g(29).eye.len(), 1);
        assert_eq!(g(30).eye.len(), 5);
    }

    #[test]
    fn all_pixels_in_bounds() {
        for n in 1..=120usize {
            let geo = g(n);
            let ok = |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < n && (y as usize) < n;
            assert!(geo.core.iter().all(|&(x, y, _)| ok(x, y)), "core n={n}");
            assert!(geo.halo.iter().all(|&(x, y)| ok(x, y)), "halo n={n}");
            assert!(
                geo.runes.iter().flatten().all(|&(x, y)| ok(x, y)),
                "runes n={n}"
            );
            assert!(geo.eye.iter().all(|&(x, y)| ok(x, y)), "eye n={n}");
        }
    }
}
