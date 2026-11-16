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

/// The smallest sigil that gets bats. They don't shrink with it: on a
/// smaller one the swarm would hide much of the sigil and not read as bats.
pub const MIN_N: usize = 26;
/// The sigil size the wobble was tuned on; below it the wobble shrinks.
const TUNED_N: f32 = 58.0;
/// Seconds on each leg of the path. The legs in and out are timed so the
/// swarm keeps roughly the pace it has at the spiral's ends.
pub const FLY_IN: f32 = 1.2;
pub const SPIRAL: f32 = 10.5;
pub const FLY_OUT: f32 = 2.5;
/// Seconds between the first bat and the last: nearly half a turn of the
/// spiral, so the swarm is strung out along it.
pub const SPREAD: f32 = 2.4;
/// The spiral's radius at its start and end, as a share of the sigil.
const R_OUTER: f32 = 0.42;
const R_INNER: f32 = 0.24;
/// The bats' orbits, as shares of those radii: each flies its own lane,
/// so the swarm fills a band and doesn't bunch up as the spiral tightens.
/// The widest stays inside the sigil.
const ORBITS: (f32, f32) = (0.55, 1.15);

/// One bat of the swarm: how far behind the first it flies, how wide its
/// orbit is (1 = the middle of the swarm), and its wobble.
#[derive(Clone, Debug)]
pub struct Bat {
    pub delay: f32,
    pub orbit: f32,
    pub phase: f32,
    pub amp: f32,
}

/// Five to nine bats, by the sigil's size. The first leads; the rest
/// follow within `SPREAD` seconds. Delays and orbits are dealt evenly
/// across their ranges (then jittered, and the orbits shuffled), so no two
/// bats share a place in the swarm.
pub fn swarm(n: usize, rng: &mut fastrand::Rng) -> Vec<Bat> {
    let count = (n / 8).clamp(5, 9);
    let share = |i: usize, rng: &mut fastrand::Rng| (i as f32 + rng.f32()) / count as f32;
    let mut lanes: Vec<usize> = (0..count).collect();
    rng.shuffle(&mut lanes);
    (0..count)
        .map(|i| Bat {
            delay: if i == 0 { 0.0 } else { share(i, rng) * SPREAD },
            orbit: ORBITS.0 + (ORBITS.1 - ORBITS.0) * share(lanes[i], rng),
            phase: rng.f32() * TAU,
            amp: 2.0 + rng.f32() * 3.0,
        })
        .collect()
}

/// The scene's length: until the last bat is out.
pub fn duration() -> f32 {
    FLY_IN + SPIRAL + FLY_OUT + SPREAD
}

/// A bat's path in sigil pixels (the sigil is the box 0..n), `t` seconds
/// after it sets out: a line to the top of the outer turn, two clockwise
/// turns inwards ending at the top heading right, and a line out. `orbit`
/// scales the turns' radii: 1 is the middle of the swarm. Before the start
/// and after the end it stays at the path's ends.
pub fn path(t: f32, n: f32, orbit: f32) -> (f32, f32) {
    let c = (n - 1.0) / 2.0;
    let (outer, inner) = (R_OUTER * n * orbit, R_INNER * n * orbit);
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
    let (x, y) = path(t - b.delay, n, b.orbit);
    let amp = b.amp * (n / TUNED_N).min(1.0);
    let wobble = (
        amp * (2.3 * t + b.phase).sin(),
        amp * (1.7 * t + 2.0 * b.phase).cos(),
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
        let (x, y) = path(t, N, 1.0);
        (x - C).hypot(y - C)
    }

    #[test]
    fn in_from_the_top_left_and_out_at_the_right() {
        for orbit in [ORBITS.0, 1.0, ORBITS.1] {
            let (x, y) = path(0.0, N, orbit);
            assert!(
                x < 0.0 && y < N * 0.3,
                "starts beyond the top left: {x},{y}"
            );
            let (x, _) = path(FLY_IN + SPIRAL + FLY_OUT, N, orbit);
            assert!(x > N, "ends beyond the right edge: {x}");
            assert_eq!(path(-5.0, N, orbit), path(0.0, N, orbit));
        }
    }

    #[test]
    fn two_turns_spiralling_inwards() {
        let steps = 900;
        let at = |i: usize| FLY_IN + SPIRAL * i as f32 / steps as f32;
        assert!((radius(at(0)) - 0.42 * N).abs() < 0.01);
        assert!((radius(at(steps)) - 0.24 * N).abs() < 0.01);
        let mut turned = 0.0;
        for i in 0..steps {
            assert!(radius(at(i + 1)) <= radius(at(i)) + 1e-3);
            let (a, b) = (path(at(i), N, 1.0), path(at(i + 1), N, 1.0));
            let d = (b.1 - C).atan2(b.0 - C) - (a.1 - C).atan2(a.0 - C);
            turned += (d + PI).rem_euclid(TAU) - PI;
        }
        assert!((turned - 4.0 * PI).abs() < 0.05, "{turned}");
    }

    #[test]
    fn never_fast_and_never_jumps() {
        let dt = 0.02;
        // The widest orbit is the fastest.
        for orbit in [ORBITS.0, 1.0, ORBITS.1] {
            let mut t = 0.0;
            while t < FLY_IN + SPIRAL + FLY_OUT {
                let (a, b) = (path(t, N, orbit), path(t + dt, N, orbit));
                let speed = (b.0 - a.0).hypot(b.1 - a.1) / dt;
                assert!(speed <= 40.0, "{speed} px/s at {t} on orbit {orbit}");
                t += dt;
            }
        }
        // And it stays in front of the sigil: inside its box as it turns.
        let (_, top) = path(FLY_IN, N, ORBITS.1);
        assert!(C - top <= N / 2.0, "the widest turn starts {top} px down");
    }

    /// The mean distance between two bats of the swarm, in pixels.
    fn mean_gap(swarm: &[Bat], t: f32) -> f32 {
        let at: Vec<(f32, f32)> = swarm.iter().map(|b| bat_at(b, t, N).0).collect();
        let mut gaps = Vec::new();
        for (i, a) in at.iter().enumerate() {
            for b in &at[i + 1..] {
                gaps.push((a.0 - b.0).hypot(a.1 - b.1));
            }
        }
        gaps.iter().sum::<f32>() / gaps.len() as f32
    }

    #[test]
    fn swarm_stays_spread_out_through_the_spiral() {
        // Seen bunching: every bat on one track, so the tightening spiral
        // packed them into a clump (under 5 px apart at its worst). A sixth
        // of the sigil between two bats, on average, keeps it a swarm.
        for seed in 1..=20 {
            let swarm = swarm(58, &mut fastrand::Rng::with_seed(seed));
            // From the last bat entering the spiral to the first leaving it.
            let mut t = FLY_IN + SPREAD;
            while t <= FLY_IN + SPIRAL {
                let gap = mean_gap(&swarm, t);
                assert!(gap >= N / 6.0, "seed {seed}: {gap} px apart at {t} s");
                t += 0.25;
            }
        }
    }

    #[test]
    fn wobble_shrinks_with_the_sigil() {
        let b = Bat {
            delay: 0.0,
            orbit: 1.0,
            phase: 0.7,
            amp: 5.0,
        };
        for (n, most) in [(58.0, 5.0), (116.0, 5.0), (29.0, 2.5)] {
            let mut t = 0.0;
            while t < duration() {
                let ((x, y), _) = bat_at(&b, t, n);
                let on_path = path(t, n, 1.0);
                assert!(
                    (x - on_path.0).abs() <= most + 1e-3 && (y - on_path.1).abs() <= most + 1e-3
                );
                t += 0.1;
            }
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
        assert!(s.iter().all(|b| (ORBITS.0..=ORBITS.1).contains(&b.orbit)));
        let ((x, _), pose) = bat_at(&s[0], 0.0, N);
        assert!(x < 3.0 && pose < 2);
    }
}
