//! The bats: in from the top left, two turns of an inward spiral in front
//! of the sigil, and out at the right.

use crate::palette::Rgb;
use crate::render::raster::Buf;
use crate::sigil::anim::rd;
use crate::sigil::sprites::Pose;
use std::f32::consts::{PI, TAU};

/// A dim violet-grey, a little lighter than the background. Bats hide
/// what they pass over, so against the sigil they are moving gaps.
pub const BODY: Rgb = [40.0, 24.0, 62.0];

/// The smallest sigil that gets bats.
pub const MIN_N: usize = 16;
/// Seconds on each leg of the path. The legs in and out are timed so the
/// swarm keeps roughly the pace it has at the spiral's ends.
pub const FLY_IN: f32 = 1.2;
pub const SPIRAL: f32 = 10.5;
pub const FLY_OUT: f32 = 2.5;
/// Seconds between the first bat and the last.
pub const SPREAD: f32 = 1.2;
/// The spiral's radius at its start and end, as a share of the sigil.
const R_OUTER: f32 = 0.42;
const R_INNER: f32 = 0.16;

/// One bat of the swarm: how far behind the first it flies, and its wobble.
#[derive(Clone, Debug)]
pub struct Bat {
    pub delay: f32,
    pub phase: f32,
    pub amp: f32,
}

/// Five to nine bats, by the sigil's size. The first leads; the rest
/// follow within `SPREAD` seconds.
pub fn swarm(n: usize, rng: &mut fastrand::Rng) -> Vec<Bat> {
    (0..(n / 8).clamp(5, 9))
        .map(|i| Bat {
            delay: if i == 0 { 0.0 } else { rng.f32() * SPREAD },
            phase: rng.f32() * TAU,
            amp: 2.0 + rng.f32() * 3.0,
        })
        .collect()
}

/// The scene's length: until the last bat is out.
pub fn duration() -> f32 {
    FLY_IN + SPIRAL + FLY_OUT + SPREAD
}

/// The swarm's centre in sigil pixels (the sigil is the box 0..n), `t`
/// seconds in: a line to the top of the outer turn, two clockwise turns
/// inwards ending at the top heading right, and a line out. Before the
/// start and after the end it stays at the path's ends.
pub fn path(t: f32, n: f32) -> (f32, f32) {
    let c = (n - 1.0) / 2.0;
    let (outer, inner) = (R_OUTER * n, R_INNER * n);
    let line =
        |a: (f32, f32), b: (f32, f32), s: f32| (a.0 + (b.0 - a.0) * s, a.1 + (b.1 - a.1) * s);
    let t = t.clamp(0.0, FLY_IN + SPIRAL + FLY_OUT);
    if t < FLY_IN {
        line((-6.0, c - outer - 0.1 * n), (c, c - outer), t / FLY_IN)
    } else if t < FLY_IN + SPIRAL {
        let s = (t - FLY_IN) / SPIRAL;
        let angle = 1.5 * PI + 4.0 * PI * s;
        let r = outer + (inner - outer) * s;
        (c + r * angle.cos(), c + r * angle.sin())
    } else {
        let s = (t - FLY_IN - SPIRAL) / FLY_OUT;
        line((c, c - inner), (n + 6.0, c - inner - 0.1 * n), s)
    }
}

/// One bat `t` seconds in: its position and flap pose (three flaps a second).
pub fn bat_at(b: &Bat, t: f32, n: f32) -> ((f32, f32), usize) {
    let (x, y) = path(t - b.delay, n);
    let wobble = (
        b.amp * (2.3 * t + b.phase).sin(),
        b.amp * (1.7 * t + 2.0 * b.phase).cos(),
    );
    (
        (x + wobble.0, y + wobble.1),
        (t * 6.0 + b.phase) as usize % 2,
    )
}

/// Draw the swarm on the pane; `origin` is the sigil's top-left corner.
/// A bat is drawn only while it is on its way: not before it sets out,
/// nor once it has reached the path's end.
pub(crate) fn draw(
    buf: &mut Buf,
    poses: &[Pose],
    swarm: &[Bat],
    t: f32,
    n: usize,
    origin: (usize, usize),
) {
    for b in swarm {
        if !(0.0..FLY_IN + SPIRAL + FLY_OUT).contains(&(t - b.delay)) {
            continue;
        }
        let ((x, y), flap) = bat_at(b, t, n as f32);
        let Some(pose) = poses.get(flap) else {
            continue;
        };
        let x0 = origin.0 as i32 + rd(x) - pose.w as i32 / 2;
        let y0 = origin.1 as i32 + rd(y) - pose.h as i32 / 2;
        for j in 0..pose.h {
            for k in 0..pose.w {
                if pose.at(k, j).is_some() {
                    buf.set(x0 + k as i32, y0 + j as i32, BODY);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{PI, TAU};

    const N: f32 = 58.0;
    const C: f32 = 28.5;

    fn radius(t: f32) -> f32 {
        let (x, y) = path(t, N);
        (x - C).hypot(y - C)
    }

    #[test]
    fn in_from_the_top_left_and_out_at_the_right() {
        let (x, y) = path(0.0, N);
        assert!(
            x < 0.0 && y < N * 0.25,
            "starts beyond the top left: {x},{y}"
        );
        let (x, _) = path(FLY_IN + SPIRAL + FLY_OUT, N);
        assert!(x > N, "ends beyond the right edge: {x}");
        assert_eq!(path(-5.0, N), path(0.0, N));
    }

    #[test]
    fn two_turns_spiralling_inwards() {
        let steps = 900;
        let at = |i: usize| FLY_IN + SPIRAL * i as f32 / steps as f32;
        assert!((radius(at(0)) - 0.42 * N).abs() < 0.01);
        assert!((radius(at(steps)) - 0.16 * N).abs() < 0.01);
        let mut turned = 0.0;
        for i in 0..steps {
            assert!(radius(at(i + 1)) <= radius(at(i)) + 1e-3);
            let (a, b) = (path(at(i), N), path(at(i + 1), N));
            let d = (b.1 - C).atan2(b.0 - C) - (a.1 - C).atan2(a.0 - C);
            turned += (d + PI).rem_euclid(TAU) - PI;
        }
        assert!((turned - 4.0 * PI).abs() < 0.05, "{turned}");
    }

    #[test]
    fn never_fast_and_never_jumps() {
        let dt = 0.02;
        let mut t = 0.0;
        while t < FLY_IN + SPIRAL + FLY_OUT {
            let (a, b) = (path(t, N), path(t + dt, N));
            let speed = (b.0 - a.0).hypot(b.1 - a.1) / dt;
            assert!(speed <= 40.0, "{speed} px/s at {t}");
            t += dt;
        }
    }

    #[test]
    fn swarm_sizes_and_delays() {
        let mut rng = fastrand::Rng::with_seed(1);
        assert_eq!(swarm(16, &mut rng).len(), 5);
        assert_eq!(swarm(58, &mut rng).len(), 7);
        assert_eq!(swarm(200, &mut rng).len(), 9);
        let s = swarm(58, &mut rng);
        assert_eq!(s[0].delay, 0.0);
        assert!(
            s.iter()
                .all(|b| (0.0..=SPREAD).contains(&b.delay) && (2.0..=5.0).contains(&b.amp))
        );
        let ((x, _), pose) = bat_at(&s[0], 0.0, N);
        assert!(x < 3.0 && pose < 2);
    }
}
