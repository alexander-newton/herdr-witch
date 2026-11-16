//! The cat: in from the right along the floor, a stop just short of the
//! witch to look at the sigil and then at us, and out at the left, passing
//! behind her.

use super::FLOOR;
use crate::palette::{Rgb, lerp_c};
use crate::render::raster::Buf;
use crate::sigil::anim::rd;
use crate::sigil::sprites::Pose;
use std::f32::consts::PI;

/// Darker than the background: a black cat.
pub const BODY: Rgb = [3.0, 2.0, 6.0];
/// A dim light added along its top edges, so its outline can be made out.
pub const RIM: Rgb = [32.0, 14.0, 49.0];
/// Its eyes, and what they turn in the middle of the scene.
pub const AMBER: Rgb = [230.0, 160.0, 50.0];
pub const RED: Rgb = [230.0, 40.0, 30.0];

/// The poses in `assets/cat.txt`, in the file's order.
pub const WALK_A: usize = 0;
pub const WALK_B: usize = 1;
pub const LOOK_UP: usize = 2;
pub const FRONT: usize = 3;

/// Pixels per second.
pub const SPEED: f32 = 4.0;
/// The cat's width in pixels.
pub const WIDTH: f32 = 12.0;
/// Seconds per walking pose.
const STRIDE: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Eyes {
    Open,
    Shut,
    /// 0 = amber, 1 = red.
    Red(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    /// Left edge, strip pixels.
    pub x: f32,
    pub pose: usize,
    pub eyes: Eyes,
}

/// What the cat does while stopped, in order: for how long, in which pose,
/// and its eyes (`None`: they redden and fade back).
const STOP: [(f32, usize, Option<Eyes>); 7] = [
    (2.9, LOOK_UP, Some(Eyes::Open)),
    (1.0, FRONT, Some(Eyes::Open)),
    (0.15, FRONT, Some(Eyes::Shut)),
    (2.0, FRONT, None),
    (0.6, FRONT, Some(Eyes::Open)),
    (0.15, FRONT, Some(Eyes::Shut)),
    (0.8, FRONT, Some(Eyes::Open)),
];

/// Where the cat stops (its left edge): 3 px of floor clear of the witch's
/// robe, whose right edge is sprite column 15.
pub fn stop_x(witch_x: i32) -> f32 {
    (witch_x + 19) as f32
}

/// Seconds from the right edge to the stop.
fn walk_in(n: usize, witch_x: i32) -> f32 {
    ((n as f32 - stop_x(witch_x)) / SPEED).max(0.0)
}

/// The scene's length: until the cat is just past the left edge.
pub fn duration(n: usize, witch_x: i32) -> f32 {
    let stopped: f32 = STOP.iter().map(|s| s.0).sum();
    walk_in(n, witch_x) + stopped + (stop_x(witch_x) + WIDTH) / SPEED + 0.1
}

pub fn shot(t: f32, n: usize, witch_x: i32) -> Shot {
    let stop = stop_x(witch_x);
    let walking = |x: f32| Shot {
        x,
        pose: if (t / STRIDE) as usize % 2 == 0 {
            WALK_A
        } else {
            WALK_B
        },
        eyes: Eyes::Open,
    };
    let arrive = walk_in(n, witch_x);
    if t < arrive {
        return walking(n as f32 - SPEED * t);
    }
    let mut u = t - arrive;
    for (len, pose, eyes) in STOP {
        if u < len {
            let eyes = eyes.unwrap_or(Eyes::Red((PI * u / len).sin()));
            return Shot {
                x: stop,
                pose,
                eyes,
            };
        }
        u -= len;
    }
    walking(stop - SPEED * u)
}

/// Draw the cat on the witch's strip, its feet on the floor.
pub(crate) fn draw(wb: &mut Buf, poses: &[Pose], shot: &Shot) {
    let Some(pose) = poses.get(shot.pose) else {
        return;
    };
    let (x0, y0) = (rd(shot.x), FLOOR as i32 + 1 - pose.h as i32);
    let eye = match shot.eyes {
        Eyes::Open => AMBER,
        Eyes::Shut => BODY,
        Eyes::Red(b) => lerp_c(AMBER, RED, b),
    };
    for j in 0..pose.h {
        for k in 0..pose.w {
            let Some(letter) = pose.at(k, j) else {
                continue;
            };
            let (x, y) = (x0 + k as i32, y0 + j as i32);
            wb.set(x, y, if letter == 'o' { eye } else { BODY });
            if j == 0 || pose.at(k, j - 1).is_none() {
                wb.add(x, y - 1, RIM, 1.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(n: usize, wx: i32) -> Vec<Shot> {
        let d = duration(n, wx);
        (0..=(d / 0.01) as usize)
            .map(|i| shot(i as f32 * 0.01, n, wx))
            .collect()
    }

    #[test]
    fn walks_in_stops_and_leaves_at_the_left() {
        let s = samples(58, 21);
        assert_eq!(s[0].x, 58.0);
        assert!(s.windows(2).all(|w| w[1].x <= w[0].x), "never walks back");
        let stopped = s.iter().filter(|c| c.x == stop_x(21)).count();
        assert!(
            (750..=770).contains(&stopped),
            "stopped for 7.6 s: {stopped}"
        );
        assert_eq!(stop_x(21), 40.0);
        assert!(shot(duration(58, 21), 58, 21).x <= -WIDTH);
        assert!((20.0..30.0).contains(&duration(58, 21)));
    }

    #[test]
    fn eyes_blink_redden_and_blink() {
        let mut seen: Vec<&str> = Vec::new();
        let mut peak = 0f32;
        for c in samples(58, 21).iter().filter(|c| c.pose == FRONT) {
            let name = match c.eyes {
                Eyes::Open => "open",
                Eyes::Shut => "shut",
                Eyes::Red(b) => {
                    assert!((0.0..=1.0).contains(&b));
                    peak = peak.max(b);
                    "red"
                }
            };
            if seen.last() != Some(&name) {
                seen.push(name);
            }
        }
        assert_eq!(seen, ["open", "shut", "red", "open", "shut", "open"]);
        assert!(peak > 0.99);
    }

    #[test]
    fn poses_in_order() {
        let s = samples(58, 21);
        let first = |p| s.iter().position(|c| c.pose == p).unwrap();
        assert!(first(WALK_A) < first(LOOK_UP) && first(LOOK_UP) < first(FRONT));
        assert!(s.iter().any(|c| c.pose == WALK_B));
        assert!(
            s.iter()
                .filter(|c| c.x != stop_x(21))
                .all(|c| c.pose <= WALK_B)
        );
        assert!(
            s.iter()
                .filter(|c| c.pose != FRONT)
                .all(|c| c.eyes == Eyes::Open)
        );
    }

    #[test]
    fn a_narrow_strip_still_ends() {
        // N = 34: the cat stops half off the right edge, and still leaves.
        let s = samples(34, 9);
        assert!(s.iter().all(|c| c.x.is_finite()));
        assert!(shot(duration(34, 9), 34, 9).x <= -WIDTH);
    }
}
