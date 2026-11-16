//! Sigil lightning and summoning-witch animation (port of the mockup's
//! `drawPent` / `drawWitch`). Bolts and sparks live in sigil-local pixel
//! coordinates (0..n); `draw` offsets them onto the pane canvas.
use super::fit::{Fit, Mode};
use super::geometry::{Kind, SigilGeometry};
use super::sprites::{Assets, Sprite, WitchSize};
use crate::palette::*;
use crate::render::raster::{Buf, line_pts, quant_octant};
use std::collections::HashSet;
use std::f32::consts::PI;

type Pt = (f32, f32);

#[derive(Clone, Debug)]
pub struct Bolt {
    pub from: Pt,
    pub to: Pt,
    pub life: u32,
    pub max: u32,
}

#[derive(Clone, Debug)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub life: f32,
    pub c: Rgb,
}

pub struct SigilAnim {
    pub(crate) rng: fastrand::Rng,
    pub t: f32,
    pub arms: f32,
    working: u32,
    pub(crate) bolts: Vec<Bolt>,
    pub(crate) sparks: Vec<Particle>,
    pub(crate) wsparks: Vec<Particle>,
}

/// JS `Math.round` (half rounds up).
fn rd(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Extra bolts per frame around the circle while the witch's arms are raised.
const TYPING_BOLT_RATE: f32 = 0.04;
/// Per-frame chance of a bolt from the witch's hands up into the sigil.
const TYPING_WITCH_BOLT_CHANCE: f32 = 0.05;
/// Extra per-frame chance of a full-sigil strike flash while summoning.
const TYPING_STRIKE_CHANCE: f32 = 0.005;

pub fn intensity(working: u32, arms: f32, t: f32) -> f32 {
    let base = if working == 0 {
        0.12 + 0.05 * (t * 0.8).sin()
    } else {
        (0.28 + 0.1 * working as f32).min(1.0)
    };
    (base + 0.22 * arms).min(1.15)
}

impl SigilAnim {
    pub fn new(seed: u64) -> Self {
        SigilAnim {
            rng: fastrand::Rng::with_seed(seed),
            t: 0.0,
            arms: 0.0,
            working: 0,
            bolts: Vec::new(),
            sparks: Vec::new(),
            wsparks: Vec::new(),
        }
    }

    fn rnd(&mut self, a: f32, b: f32) -> f32 {
        a + self.rng.f32() * (b - a)
    }

    /// Spawn count for a fractional per-frame rate.
    fn count(&mut self, rate: f32) -> usize {
        rate.floor() as usize + usize::from(self.rng.f32() < rate.fract())
    }

    /// Advance one frame: time, arm easing and the working count. Bolts and
    /// sparks are spawned and aged inside `draw`, which has the geometry.
    pub fn step(&mut self, working: u32, typing: bool) {
        self.t += 0.08;
        self.working = working;
        let d = if typing { 1.0 } else { 0.0 } - self.arms;
        self.arms += if d > 0.0 { d.min(0.22) } else { d.max(-0.12) };
    }

    /// Drop dead bolts, then spawn this frame's bolts and sparks.
    fn spawn(&mut self, geo: &SigilGeometry) {
        let n = self.working as f32;
        let nf = geo.n as f32;
        self.bolts.retain(|b| b.life > 0);
        self.sparks.retain(|s| s.life > 0.0);
        let rate = n * 0.3
            + if self.arms > 0.6 {
                TYPING_BOLT_RATE
            } else {
                0.0
            };
        for _ in 0..self.count(rate) {
            let ai = self.rng.usize(0..5);
            let a = geo.verts[ai];
            let (to, same) = if self.working >= 5 && self.rng.f32() < 0.35 {
                let ang = self.rnd(0.0, 2.0 * PI);
                let rr = self.rnd(nf * 0.48, nf * 0.6);
                ((geo.cx + rr * ang.cos(), geo.cy + rr * ang.sin()), false)
            } else {
                let bi = self.rng.usize(0..5);
                (geo.verts[bi], bi == ai)
            };
            if !same {
                let life = 2 + self.rng.u32(0..2);
                self.bolts.push(Bolt {
                    from: a,
                    to,
                    life,
                    max: life,
                });
            }
        }
        if self.arms > 0.7 && self.rng.f32() < TYPING_WITCH_BOLT_CHANCE {
            let x = self.rnd(nf * 0.3, nf * 0.7);
            let to = geo.verts[2 + self.rng.usize(0..2)];
            self.bolts.push(Bolt {
                from: (x, nf - 1.0),
                to,
                life: 2,
                max: 2,
            });
        }
        let sr = (n * 0.45 + self.arms * 0.6) * (nf / 44.0).min(1.0) + 0.05 * n;
        for _ in 0..self.count(sr) {
            if geo.core.is_empty() {
                break;
            }
            let (px, py, _) = geo.core[self.rng.usize(0..geo.core.len())];
            let (vx, vy) = (
                self.rnd(-0.3, 0.3),
                self.rnd(-0.9, -0.3) * (nf / 30.0).min(1.0),
            );
            let life = self.rnd(4.0, 10.0);
            let c = if self.rng.f32() < 0.3 + n * 0.05 {
                CYAN
            } else {
                GOLD
            };
            self.sparks.push(Particle {
                x: px as f32,
                y: py as f32,
                vx,
                vy,
                life,
                c,
            });
        }
    }

    fn bolt_path(&mut self, n: usize, b: &Bolt) -> Vec<Pt> {
        let nf = n as f32;
        let segs = ((nf / 6.0).round() as usize).max(3);
        let jit = (2.6 * nf / 44.0).max(1.0);
        let (dx, dy) = (b.to.0 - b.from.0, b.to.1 - b.from.1);
        let len = dx.hypot(dy);
        let len = if len == 0.0 { 1.0 } else { len };
        let (nx, ny) = (-dy / len, dx / len);
        (0..=segs)
            .map(|s| {
                let t = s as f32 / segs as f32;
                let j = if s == 0 || s == segs {
                    0.0
                } else {
                    self.rnd(-jit, jit)
                };
                (b.from.0 + dx * t + nx * j, b.from.1 + dy * t + ny * j)
            })
            .collect()
    }

    /// Render one frame. Must be called exactly once per `step`: it spawns
    /// and ages bolts and sparks.
    pub fn draw(
        &mut self,
        buf: &mut Buf,
        fit: &Fit,
        geo: &SigilGeometry,
        assets: &Assets,
        mode: Mode,
    ) {
        self.spawn(geo);
        let pent = self.draw_pent(geo);
        buf.clear(BG);
        blit(buf, &pent, fit.sigil_xy);
        if let Some((size, xy)) = fit.witch {
            let spr = match size {
                WitchSize::Big => &assets.big,
                WitchSize::Mini => &assets.mini,
            };
            let witch = self.draw_witch(geo.n, spr);
            blit(buf, &witch, xy);
        }
        if mode == Mode::Octant {
            quant_octant(buf);
        }
        self.age();
    }

    /// Per-frame ageing after drawing: bolts lose a life, sparks drift.
    fn age(&mut self) {
        for b in &mut self.bolts {
            b.life = b.life.saturating_sub(1);
        }
        for s in &mut self.sparks {
            s.x += s.vx;
            s.y += s.vy;
            s.life -= 1.0;
        }
    }

    fn draw_pent(&mut self, geo: &SigilGeometry) -> Buf {
        let n = self.working as f32;
        let nn = geo.n;
        let i = intensity(self.working, self.arms, self.t);
        let mut pb = Buf::new(nn, nn);
        pb.clear(BG);
        let mut f = 1.0 - self.rng.f32() * (0.06 + n * 0.07);
        let strike = self.rng.f32()
            < n * 0.018
                + if self.arms > 0.8 {
                    TYPING_STRIKE_CHANCE
                } else {
                    0.0
                };
        let mut base = if i < 0.5 {
            lerp_c(VIOLET, GOLD, i * 2.0)
        } else {
            lerp_c(GOLD, BRIGHT, ((i - 0.5) * 2.0).min(1.0))
        };
        if self.working == 0 {
            base = lerp_c(DEEP, VIOLET, 0.6 + i);
        }
        if strike {
            base = lerp_c(BRIGHT, WHITE, 0.6);
            f = 1.25;
        }
        let hk = (0.35 + i * 0.9) * f;
        for &(x, y) in &geo.halo {
            pb.add(x, y, if strike { VIOLET } else { HALO }, hk);
        }
        for &(x, y, kind) in &geo.core {
            let mut c = base;
            if kind == Kind::Ring {
                c = lerp_c(base, VIOLET, 0.25);
            }
            if self.rng.f32() < n * 0.006 {
                c = CYAN;
            }
            pb.set(x, y, mul(c, f));
        }
        for (k, px) in geo.runes.iter().enumerate() {
            let ph = 0.5 + 0.5 * (self.t * 1.4 + k as f32 * 0.9).sin();
            let mut c = lerp_c(VIOLET, GOLD, (ph * (0.3 + i)).min(1.0));
            if self.working >= 4 && ph > 0.85 {
                c = CYAN;
            }
            if self.working == 0 {
                c = lerp_c(DEEP, ORCHID, ph * 0.5);
            }
            for &(x, y) in px {
                pb.set(x, y, c);
            }
        }
        let ep = 0.5 + 0.5 * (self.t * 3.0).sin();
        for (k, &(x, y)) in geo.eye.iter().enumerate() {
            let c = if k == 1 || geo.eye.len() == 1 {
                lerp_c(VIOLET, CYAN, (ep * i * 1.5).min(1.0))
            } else {
                mul(VIOLET, 0.6 + i * 0.4)
            };
            pb.set(x, y, c);
        }
        let bolts = std::mem::take(&mut self.bolts);
        for b in bolts.iter().filter(|b| b.life > 0) {
            let pts = self.bolt_path(nn, b);
            let c = if b.life == b.max { WHITE } else { CYAN };
            for w in pts.windows(2) {
                for (x, y) in line_pts(rd(w[0].0), rd(w[0].1), rd(w[1].0), rd(w[1].1)) {
                    pb.set(x, y, c);
                    if nn >= 30 {
                        pb.add(x + 1, y, CYAN, 0.25);
                        pb.add(x - 1, y, CYAN, 0.25);
                    }
                }
            }
        }
        self.bolts = bolts;
        for s in &self.sparks {
            pb.add(rd(s.x), rd(s.y), s.c, (s.life / 6.0).min(1.0));
        }
        pb
    }

    fn draw_witch(&mut self, n: usize, spr: &Sprite) -> Buf {
        let i = intensity(self.working, self.arms, self.t);
        let hh = spr.canvas_h;
        let mut wb = Buf::new(n, hh);
        wb.clear(BG);
        for y in 0..hh.min(6) {
            for x in 0..n {
                wb.add(x as i32, y as i32, VIOLET, (6 - y) as f32 / 6.0 * 0.18 * i);
            }
        }
        for x in 0..n {
            let d = (x as f32 - (n as f32 - 1.0) / 2.0).abs() / (n as f32 / 2.0);
            wb.set(x as i32, hh as i32 - 1, lerp_c(DEEP, HALO, 1.0 - d));
            wb.add(x as i32, hh as i32 - 2, HALO, (1.0 - d) * 0.4);
        }
        let sx = rd((n as f32 - spr.w as f32) / 2.0);
        let sy = 1;
        let mut mask: HashSet<(i32, i32)> = HashSet::new();
        for j in 0..spr.h {
            for k in 0..spr.w {
                if let Some(c) = spr.px[j * spr.w + k] {
                    let p = (sx + k as i32, sy + j as i32);
                    wb.set(p.0, p.1, c);
                    mask.insert(p);
                }
            }
        }
        let a = self.arms;
        let sway = (self.t * 7.0).sin() * 0.14 * a;
        let ang_l = lerp(1.83, 4.19, a) + sway;
        let ang_r = PI - ang_l;
        for (side, &(ox, oy)) in spr.shoulders.iter().enumerate() {
            let ang = if side == 0 { ang_l } else { ang_r };
            let (shx, shy) = ((sx + ox) as f32, (sy + oy) as f32);
            let (hx, hy) = (shx + spr.arm_len * ang.cos(), shy + spr.arm_len * ang.sin());
            for (x, y) in line_pts(rd(shx), rd(shy), rd(hx), rd(hy)) {
                for t in 0..spr.arm_thick as i32 {
                    wb.set(x, y + t, SLEEVE);
                    mask.insert((x, y + t));
                }
            }
            let (hs, o): (&[(i32, i32)], i32) = if spr.arm_thick == 2 {
                (&[(0, 0), (1, 0), (0, 1), (1, 1)], 1)
            } else {
                (&[(0, 0)], 0)
            };
            for &(dx, dy) in hs {
                let p = (rd(hx) + dx - o, rd(hy) + dy - o);
                wb.set(p.0, p.1, SKIN);
                mask.insert(p);
            }
            if a > 0.7 && self.rng.f32() < 0.5 {
                let (vx, vy) = (self.rnd(-0.4, 0.4), self.rnd(-1.0, -0.3));
                let life = self.rnd(3.0, 7.0);
                let c = if self.rng.f32() < 0.5 { CYAN } else { GOLD };
                self.wsparks.push(Particle {
                    x: hx,
                    y: hy,
                    vx,
                    vy,
                    life,
                    c,
                });
            }
        }
        let rim = lerp_c(DEEP, GOLD, (i * 0.8).min(1.0));
        for &(x, y) in &mask {
            if !mask.contains(&(x, y - 1)) {
                wb.add(x, y - 1, rim, 0.5);
            }
        }
        self.wsparks.retain(|s| s.life > 0.0);
        for s in &mut self.wsparks {
            wb.add(rd(s.x), rd(s.y), s.c, (s.life / 4.0).min(1.0));
            s.x += s.vx;
            s.y += s.vy;
            s.life -= 1.0;
        }
        wb
    }
}

fn blit(dst: &mut Buf, src: &Buf, (ox, oy): (usize, usize)) {
    for y in 0..src.h {
        for x in 0..src.w {
            dst.set((ox + x) as i32, (oy + y) as i32, src.get(x, y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sigil::fit::fit;
    use crate::sigil::geometry::build;
    use crate::sigil::sprites::load_assets;

    fn setup(cols: usize, rows: usize, mode: Mode) -> (Fit, SigilGeometry, Assets) {
        let assets = load_assets(None);
        let f = fit(cols, rows, mode).unwrap();
        let geo = build(f.n, &assets.runes);
        (f, geo, assets)
    }

    #[test]
    fn intensity_values() {
        assert!((intensity(0, 0.0, 0.0) - 0.12).abs() < 1e-6);
        assert!((intensity(3, 0.0, 1.0) - 0.58).abs() < 1e-6);
        assert!((intensity(8, 0.0, 1.0) - 1.0).abs() < 1e-6);
        assert!((intensity(8, 1.0, 1.0) - 1.15).abs() < 1e-6);
    }

    #[test]
    fn arms_ease() {
        let mut a = SigilAnim::new(1);
        for _ in 0..5 {
            a.step(0, true);
        }
        assert!((a.arms - 1.0).abs() < 1e-5);
        for _ in 0..3 {
            a.step(0, false);
        }
        assert!((a.arms - 0.64).abs() < 1e-5);
    }

    fn run(
        seed: u64,
        frames: usize,
        working: u32,
        typing: bool,
        mut each: impl FnMut(&SigilAnim, &SigilGeometry),
    ) {
        let (f, geo, assets) = setup(22, 40, Mode::Octant); // n = 44
        let mut buf = Buf::new(f.px_w, f.px_h);
        let mut a = SigilAnim::new(seed);
        for _ in 0..frames {
            a.step(working, typing);
            a.draw(&mut buf, &f, &geo, &assets, Mode::Octant);
            each(&a, &geo);
        }
    }

    #[test]
    fn idle_has_no_bolts() {
        run(1, 1000, 0, false, |a, _| assert!(a.bolts.is_empty()));
    }

    #[test]
    fn busy_bolts_escape() {
        let mut escaped = false;
        run(1, 1000, 8, false, |a, geo| {
            let r = 0.47 * geo.n as f32;
            escaped |= a.bolts.iter().any(|b| {
                [b.from, b.to]
                    .iter()
                    .any(|p| (p.0 - geo.cx).hypot(p.1 - geo.cy) > r)
            });
        });
        assert!(escaped);
    }

    #[test]
    fn typing_links_witch() {
        let mut linked = false;
        run(1, 300, 0, true, |a, geo| {
            linked |= a.bolts.iter().any(|b| b.from.1 == geo.n as f32 - 1.0);
        });
        assert!(linked);
    }

    #[test]
    fn octant_mode_quantised() {
        let (f, geo, assets) = setup(22, 40, Mode::Octant);
        let mut buf = Buf::new(f.px_w, f.px_h);
        let mut a = SigilAnim::new(1);
        for _ in 0..30 {
            a.step(6, true);
            a.draw(&mut buf, &f, &geo, &assets, Mode::Octant);
        }
        for by in (0..buf.h).step_by(4) {
            for bx in (0..buf.w).step_by(2) {
                let mut cols = HashSet::new();
                for y in by..(by + 4).min(buf.h) {
                    for x in bx..(bx + 2).min(buf.w) {
                        cols.insert(buf.get(x, y).map(f32::to_bits));
                    }
                }
                assert!(cols.len() <= 2, "block {bx},{by}");
            }
        }
    }

    #[test]
    fn expired_sparks_not_drawn() {
        let (f, geo, assets) = setup(22, 40, Mode::HalfBlock);
        let mut buf = Buf::new(f.px_w, f.px_h);
        let mut a = SigilAnim::new(1);
        a.step(0, false);
        // A dead spark in the sigil's empty top-left corner must not darken it.
        a.sparks.push(Particle {
            x: 0.0,
            y: 0.0,
            vx: 0.0,
            vy: 0.0,
            life: -1.0,
            c: GOLD,
        });
        a.draw(&mut buf, &f, &geo, &assets, Mode::HalfBlock);
        assert_eq!(buf.get(f.sigil_xy.0, f.sigil_xy.1), BG);
    }

    #[test]
    fn small_and_mini_do_not_panic() {
        for (c, r) in [(4, 2), (6, 12), (13, 30), (27, 40), (40, 60)] {
            for m in [Mode::HalfBlock, Mode::Octant] {
                let (f, geo, assets) = setup(c, r, m);
                let mut buf = Buf::new(f.px_w, f.px_h);
                let mut a = SigilAnim::new(3);
                for _ in 0..60 {
                    a.step(8, true);
                    a.draw(&mut buf, &f, &geo, &assets, m);
                }
            }
        }
    }
}
