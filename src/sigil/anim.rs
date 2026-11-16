//! Sigil lightning and summoning-witch animation (port of the mockup's
//! `drawPent` / `drawWitch`). Bolts and sparks live in sigil-local pixel
//! coordinates (0..n); `draw` offsets them onto the pane canvas.
use super::familiars::{Familiars, Stage, toad};
use super::fit::{Fit, Mode};
use super::geometry::{Kind, SigilGeometry};
use super::sprites::{Assets, Sprite, WitchSize, colour};
use crate::palette::*;
use crate::render::raster::{Buf, line_pts, quant_octant};
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
    /// Scratch canvases for the sigil and the witch, kept across frames.
    pent: Buf,
    witch: Buf,
    mask: Vec<bool>,
    seed: u64,
    /// The rare scenes (`sigil::familiars`). `None` unless switched on:
    /// the pane's loop and the preview do that.
    pub familiars: Option<Familiars>,
}

/// JS `Math.round` (half rounds up).
pub(crate) fn rd(v: f32) -> i32 {
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
            pent: Buf::default(),
            witch: Buf::default(),
            mask: Vec::new(),
            seed,
            familiars: None,
        }
    }

    /// Switch the familiars on, for a pane drawing `fps` frames a second.
    pub fn enable_familiars(&mut self, fps: u8) {
        // Their own random numbers, on a seed of their own.
        self.familiars = Some(Familiars::new(self.seed ^ 0x6661_6d69_6c69_6172, fps));
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
        if let Some(f) = &mut self.familiars {
            f.step(Stage::of(fit, assets), self.arms);
        }
        let mut pent = std::mem::take(&mut self.pent);
        self.draw_pent(&mut pent, geo);
        buf.clear(BG);
        blit(buf, &pent, fit.sigil_xy);
        self.pent = pent;
        if let Some((size, xy)) = fit.witch {
            let spr = match size {
                WitchSize::Big => &assets.big,
                WitchSize::Mini => &assets.mini,
            };
            let mut witch = std::mem::take(&mut self.witch);
            self.draw_witch(&mut witch, geo.n, spr, assets);
            blit(buf, &witch, xy);
            self.witch = witch;
        }
        if let Some(f) = &self.familiars {
            f.draw_over(buf, fit, assets);
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

    fn draw_pent(&mut self, pb: &mut Buf, geo: &SigilGeometry) {
        let n = self.working as f32;
        let nn = geo.n;
        let i = intensity(self.working, self.arms, self.t);
        pb.reshape(nn, nn);
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
    }

    fn draw_witch(&mut self, wb: &mut Buf, n: usize, spr: &Sprite, assets: &Assets) {
        let i = intensity(self.working, self.arms, self.t);
        let hh = spr.canvas_h;
        wb.reshape(n, hh);
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
        if let Some(f) = &self.familiars {
            f.draw_behind_witch(wb, assets);
        }
        let sx = witch_x(n, spr);
        let sy = 1;
        // The toad's scene: her hat is off (by this many pixels) while one
        // hand holds it.
        let toad = self.familiars.as_ref().and_then(Familiars::toad_shot);
        let hat = toad
            .filter(|s| s.hand.is_some())
            .map(|s| (rd(s.hat.0), rd(s.hat.1)))
            .filter(|off| *off != (0, 0));
        // The witch's silhouette, for the rim light: `mask[y * n + x]` for
        // x < n, y <= hh. A pixel elsewhere could not light a rim on the
        // canvas (the rim sits one row above its pixel), so it isn't kept.
        let mut mask = std::mem::take(&mut self.mask);
        mask.clear();
        mask.resize(n * (hh + 1), false);
        let mut mark = |(x, y): (i32, i32)| {
            if x >= 0 && y >= 0 && (x as usize) < n && y as usize <= hh {
                mask[y as usize * n + x as usize] = true;
            }
        };
        // With the hat off, its rows are drawn later, where she holds it.
        let body_from = if hat.is_some() { toad::HAT_ROWS } else { 0 };
        for j in body_from..spr.h {
            for k in 0..spr.w {
                if let Some(c) = spr.px[j * spr.w + k] {
                    let p = (sx + k as i32, sy + j as i32);
                    wb.set(p.0, p.1, c);
                    mark(p);
                }
            }
        }
        if hat.is_some() {
            // The top of her head, in her hair's colour.
            let hair = colour('K').unwrap_or(DEEP);
            let bare = &assets.bare;
            for j in 0..bare.h {
                for k in 0..bare.w {
                    if bare.at(k, j).is_some() {
                        let p = (
                            sx + toad::BARE_AT.0 + k as i32,
                            sy + toad::BARE_AT.1 + j as i32,
                        );
                        wb.set(p.0, p.1, hair);
                        mark(p);
                    }
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
            let (mut hx, mut hy) = (shx + spr.arm_len * ang.cos(), shy + spr.arm_len * ang.sin());
            // The hand on the hat goes where the toad's scene puts it.
            let hat_hand = toad.and_then(|s| s.hand).filter(|_| side == 1);
            if let Some(hand) = hat_hand {
                (hx, hy) = (sx as f32 + hand.0, sy as f32 + hand.1);
                if let Some((dx, dy)) = hat {
                    // The hat where she holds it, in front of her other arm.
                    for j in 0..toad::HAT_ROWS.min(spr.h) {
                        for k in 0..spr.w {
                            if let Some(c) = spr.px[j * spr.w + k] {
                                let p = (sx + k as i32 + dx, sy + j as i32 + dy);
                                wb.set(p.0, p.1, c);
                                mark(p);
                            }
                        }
                    }
                }
            }
            for (x, y) in line_pts(rd(shx), rd(shy), rd(hx), rd(hy)) {
                for t in 0..spr.arm_thick as i32 {
                    wb.set(x, y + t, SLEEVE);
                    mark((x, y + t));
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
                mark(p);
            }
            if a > 0.7 && self.rng.f32() < 0.5 {
                let (vx, vy) = (self.rnd(-0.4, 0.4), self.rnd(-1.0, -0.3));
                let life = self.rnd(3.0, 7.0);
                let c = if self.rng.f32() < 0.5 { CYAN } else { GOLD };
                // The hand on the hat makes no sparks, but the random
                // numbers above are drawn all the same: the sigil's
                // sequence must not depend on a scene playing.
                if hat_hand.is_none() {
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
        }
        let rim = lerp_c(DEEP, GOLD, (i * 0.8).min(1.0));
        for y in 1..=hh {
            for x in 0..n {
                if mask[y * n + x] && !mask[(y - 1) * n + x] {
                    wb.add(x as i32, y as i32 - 1, rim, 0.5);
                }
            }
        }
        self.mask = mask;
        if let Some(f) = &mut self.familiars {
            f.draw_in_front_of_witch(wb, assets);
        }
        self.wsparks.retain(|s| s.life > 0.0);
        for s in &mut self.wsparks {
            wb.add(rd(s.x), rd(s.y), s.c, (s.life / 4.0).min(1.0));
            s.x += s.vx;
            s.y += s.vy;
            s.life -= 1.0;
        }
    }
}

/// The witch's left edge in her strip: centred.
pub(crate) fn witch_x(n: usize, spr: &Sprite) -> i32 {
    rd((n as f32 - spr.w as f32) / 2.0)
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
    use std::collections::HashSet;

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

    use crate::sigil::familiars::{Scene, bats, cat, toad};

    /// A pane with familiars switched on. 40 columns of half-blocks is
    /// N = 40: the big witch, and no quantising.
    fn stage(
        cols: usize,
        rows: usize,
        mode: Mode,
        seed: u64,
    ) -> (Fit, SigilGeometry, Assets, Buf, SigilAnim) {
        let (f, geo, assets) = setup(cols, rows, mode);
        let buf = Buf::new(f.px_w, f.px_h);
        let mut a = SigilAnim::new(seed);
        a.enable_familiars(12);
        (f, geo, assets, buf, a)
    }

    fn has(buf: &Buf, c: Rgb) -> bool {
        buf.d.contains(&c)
    }

    #[test]
    fn off_by_default_and_silent_when_idle() {
        assert!(SigilAnim::new(1).familiars.is_none());
        let (f, geo, assets, mut b1, mut on) = stage(40, 40, Mode::HalfBlock, 5);
        let (mut b2, mut off) = (Buf::new(f.px_w, f.px_h), SigilAnim::new(5));
        for i in 0..600 {
            let typing = i % 100 < 30;
            on.step(3, typing);
            off.step(3, typing);
            on.draw(&mut b1, &f, &geo, &assets, Mode::HalfBlock);
            off.draw(&mut b2, &f, &geo, &assets, Mode::HalfBlock);
            assert!(
                on.familiars.as_ref().unwrap().idle(),
                "seed 5 rolls nothing in 600 frames"
            );
            assert_eq!(b1.d, b2.d, "frame {i}");
        }
    }

    #[test]
    fn every_scene_draws_and_leaves_no_trace() {
        for scene in Scene::ALL {
            let (f, geo, assets, mut b1, mut on) = stage(40, 40, Mode::HalfBlock, 5);
            let (mut b2, mut off) = (Buf::new(f.px_w, f.px_h), SigilAnim::new(5));
            on.familiars.as_mut().unwrap().force(scene);
            let mut differed = 0;
            for i in 0..600 {
                // Typing from frame 20: the toad has started by then.
                let typing = (20..120).contains(&i);
                on.step(0, typing);
                off.step(0, typing);
                on.draw(&mut b1, &f, &geo, &assets, Mode::HalfBlock);
                off.draw(&mut b2, &f, &geo, &assets, Mode::HalfBlock);
                if on.familiars.as_ref().unwrap().idle() {
                    assert_eq!(b1.d, b2.d, "{scene:?}: frame {i} after the scene");
                } else {
                    differed += usize::from(b1.d != b2.d);
                }
            }
            assert!(differed > 50, "{scene:?} drew on {differed} frames");
            assert!(on.familiars.as_ref().unwrap().idle(), "{scene:?} ended");
        }
    }

    #[test]
    fn cat_eyes_turn_red_only_mid_scene() {
        // N = 40: the cat stops at 2.25 s and its eyes redden from 6.3 s to
        // 8.3 s, frames 76 to 100 at 12 fps.
        let (f, geo, assets, mut buf, mut a) = stage(40, 40, Mode::HalfBlock, 5);
        a.familiars.as_mut().unwrap().force(Scene::Cat);
        let (mut amber, mut body) = (false, false);
        let mut red = Vec::new();
        for i in 0..600 {
            a.step(0, false);
            a.draw(&mut buf, &f, &geo, &assets, Mode::HalfBlock);
            amber |= has(&buf, cat::AMBER);
            body |= has(&buf, cat::BODY);
            if buf.d.iter().any(|c| c[0] > 200.0 && c[1] < 100.0) {
                red.push(i);
            }
        }
        assert!(amber && body);
        assert!(red.len() >= 5, "{red:?}");
        assert!(red[0] > 70 && *red.last().unwrap() < 105, "{red:?}");
        assert!(
            !has(&buf, cat::AMBER) && !has(&buf, cat::BODY),
            "gone by the end"
        );
    }

    #[test]
    fn bats_and_toad_use_their_colours() {
        for (scene, colour) in [(Scene::Bats, bats::BODY), (Scene::Toad, toad::BODY)] {
            let (f, geo, assets, mut buf, mut a) = stage(40, 40, Mode::HalfBlock, 5);
            a.familiars.as_mut().unwrap().force(scene);
            let mut seen = false;
            for _ in 0..400 {
                a.step(0, false);
                a.draw(&mut buf, &f, &geo, &assets, Mode::HalfBlock);
                seen |= has(&buf, colour);
            }
            assert!(seen, "{scene:?}");
        }
    }

    #[test]
    fn the_new_colours_are_dim() {
        let luma = |c: Rgb| 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2];
        assert!(luma(cat::BODY) < luma(BG));
        assert!(luma(bats::BODY) < 40.0 && luma(bats::BODY) > luma(BG));
        assert!(luma(cat::RIM) + luma(BG) < 50.0);
        assert!(luma(toad::BODY) < 100.0);
    }

    #[test]
    fn scenes_survive_every_size_and_a_resize() {
        for (c, r) in [
            (4, 2),
            (6, 12),
            (13, 30),
            (17, 30),
            (27, 40),
            (34, 40),
            (40, 60),
        ] {
            for m in [Mode::HalfBlock, Mode::Octant] {
                for scene in Scene::ALL {
                    let (f, geo, assets, mut buf, mut a) = stage(c, r, m, 3);
                    a.familiars.as_mut().unwrap().force(scene);
                    for i in 0..400 {
                        // Typing only once the toad has had its chance to start.
                        a.step(8, i >= 30);
                        a.draw(&mut buf, &f, &geo, &assets, m);
                    }
                }
            }
        }
        // The pane shrinks under a playing scene: dropped, and nothing breaks.
        let (f, geo, assets, mut buf, mut a) = stage(40, 40, Mode::HalfBlock, 3);
        a.familiars.as_mut().unwrap().force(Scene::Toad);
        for _ in 0..30 {
            a.step(0, false);
            a.draw(&mut buf, &f, &geo, &assets, Mode::HalfBlock);
        }
        assert!(!a.familiars.as_ref().unwrap().idle());
        let (f2, geo2, _) = setup(20, 40, Mode::HalfBlock);
        let mut small = Buf::new(f2.px_w, f2.px_h);
        a.step(0, false);
        a.draw(&mut small, &f2, &geo2, &assets, Mode::HalfBlock);
        assert!(a.familiars.as_ref().unwrap().idle());
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
