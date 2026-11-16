//! The toad: the witch takes off her hat, a large toad sitting on her hair
//! leaps to the floor and hops slowly out at the left, and she puts the
//! hat back.

pub use super::FLOOR;
use crate::palette::Rgb;
use crate::render::raster::Buf;
use crate::sigil::anim::rd;
use crate::sigil::sprites::Pose;

/// Muted green, a darker underside, dark eyes. Nothing on the toad is
/// brighter than its body: in octant mode a cell shows two colours, and a
/// bright eye would take one of them, leaving holes in the body around it.
pub const BODY: Rgb = [62.0, 104.0, 58.0];
pub const UNDER: Rgb = [48.0, 82.0, 50.0];
pub const EYE: Rgb = [10.0, 14.0, 10.0];

/// The poses in `assets/toad.txt`, in the file's order.
pub const SIT: usize = 0;
pub const LEAP: usize = 1;

/// The rows of `witch_big.txt` that are the hat.
pub const HAT_ROWS: usize = 10;
/// Where she holds the hat, from where it sits. It cannot go up: its tip
/// is on the strip's top row, under the sigil.
pub const HAT_OFF: (f32, f32) = (9.0, 7.0);
/// The toad's width in pixels.
pub const WIDTH: f32 = 11.0;
/// Where the top of her head (`assets/witch_bare.txt`) goes, in
/// witch-sprite pixels.
pub const BARE_AT: (i32, i32) = (4, 6);

/// Seconds: her hand rises to the hat, takes it off, she holds it while
/// the toad waits and then leaps, puts it back, and lowers her arm.
pub const REACH: f32 = 0.8;
pub const LIFT: f32 = 1.0;
pub const WAIT: f32 = 1.2;
pub const JUMP: f32 = 0.9;
pub const RETURN: f32 = 1.2;
pub const LOWER: f32 = 0.6;
/// A hop: seconds in the air, seconds resting before it, pixels covered
/// and pixels high.
pub const HOP: f32 = 0.5;
pub const REST: f32 = 1.2;
pub const HOP_LEN: f32 = 6.0;
pub const HOP_HIGH: f32 = 3.0;

/// Where her hand takes the hat, on its brim (witch-sprite pixels).
const GRIP: (f32, f32) = (15.0, 8.0);
/// Where her lowered arm leaves her hand.
const HAND_REST: (f32, f32) = (15.05, 18.73);
/// The toad on her head: its left edge from the witch's, and its feet row.
const ON_HEAD: (i32, f32) = (3, 6.0);
/// Where the leap lands: the toad's left edge from the witch's.
const LANDING: i32 = -14;
/// How strongly the leap bows upwards: it rises about 3 px first.
const LEAP_ARC: f32 = 36.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shot {
    /// The hat's offset from where it sits.
    pub hat: (f32, f32),
    /// The hat arm's hand in witch-sprite pixels; `None` once the arm is
    /// hers again.
    pub hand: Option<(f32, f32)>,
    /// The toad: left edge and feet row in strip pixels, and its pose.
    pub toad: Option<(f32, f32, usize)>,
    /// How many of the toad's columns the hat has uncovered (`WIDTH` = all).
    pub shown: f32,
    /// Landings so far: the leap, then each hop.
    pub landings: u32,
}

/// Smooth 0 to 1 over `u` in 0..1.
fn ease(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

fn between(a: (f32, f32), b: (f32, f32), s: f32) -> (f32, f32) {
    (a.0 + (b.0 - a.0) * s, a.1 + (b.1 - a.1) * s)
}

/// When the leap lands.
fn landed_at() -> f32 {
    REACH + LIFT + WAIT + JUMP
}

/// Where the leap lands (the toad's left edge, strip pixels): clear of her
/// robe, but not beyond the strip's edge where the strip is narrow.
fn landing_x(witch_x: i32) -> f32 {
    (witch_x + LANDING).max(0) as f32
}

/// Hops from the landing until the toad is past the left edge.
fn hops(witch_x: i32) -> u32 {
    ((landing_x(witch_x) + WIDTH) / HOP_LEN).ceil() as u32
}

/// The scene's length: a moment after the hat is back and the toad gone.
pub fn duration(witch_x: i32) -> f32 {
    let hat_back = landed_at() + RETURN + LOWER;
    let toad_gone = landed_at() + hops(witch_x) as f32 * (REST + HOP);
    hat_back.max(toad_gone) + 0.2
}

/// The scene `t` seconds in; it holds its last state after `duration`.
pub fn shot(t: f32, witch_x: i32) -> Shot {
    let (lift, wait) = (REACH, REACH + LIFT);
    let (jump, land) = (wait + WAIT, landed_at());
    let back = land + RETURN;

    // How far off the hat is: 0 on her head, 1 held beside her.
    let off = if t < lift {
        0.0
    } else if t < wait {
        ease((t - lift) / LIFT)
    } else if t < land {
        1.0
    } else {
        1.0 - ease((t - land) / RETURN)
    };
    // Sideways first, then down, so the brim clears her head.
    let hat = (HAT_OFF.0 * off, HAT_OFF.1 * off * off);
    let hand = if t < lift {
        Some(between(HAND_REST, GRIP, ease(t / REACH)))
    } else if t < back {
        Some((GRIP.0 + hat.0, GRIP.1 + hat.1))
    } else if t < back + LOWER {
        Some(between(GRIP, HAND_REST, ease((t - back) / LOWER)))
    } else {
        None
    };

    let head = ((witch_x + ON_HEAD.0) as f32, ON_HEAD.1);
    let floor_x = landing_x(witch_x);
    let (toad, landings) = if t < lift {
        (None, 0)
    } else if t < jump {
        (Some((head.0, head.1, SIT)), 0)
    } else if t < land {
        let u = (t - jump) / JUMP;
        let (x, y) = between(head, (floor_x, FLOOR), u);
        (Some((x, y - LEAP_ARC * u * (1.0 - u), LEAP)), 0)
    } else {
        let since = t - land;
        let done = ((since / (REST + HOP)) as u32).min(hops(witch_x));
        let into = since - done as f32 * (REST + HOP);
        let x = floor_x - done as f32 * HOP_LEN;
        if done == hops(witch_x) || into < REST {
            (Some((x, FLOOR, SIT)), 1 + done)
        } else {
            let u = (into - REST) / HOP;
            let y = FLOOR - 4.0 * HOP_HIGH * u * (1.0 - u);
            (Some((x - HOP_LEN * u, y, LEAP)), 1 + done)
        }
    };
    Shot {
        hat,
        hand,
        toad,
        shown: if t < wait { WIDTH * off } else { WIDTH },
        landings,
    }
}

/// Draw the toad on the witch's strip: the columns the hat has uncovered.
pub(crate) fn draw(wb: &mut Buf, poses: &[Pose], shot: &Shot) {
    let Some((x, feet, pose)) = shot.toad else {
        return;
    };
    let Some(pose) = poses.get(pose) else {
        return;
    };
    let (x0, y0) = (rd(x), rd(feet) + 1 - pose.h as i32);
    for j in 0..pose.h {
        for k in (0..pose.w).take_while(|k| (*k as f32) < shot.shown) {
            let c = match pose.at(k, j) {
                Some('o') => EYE,
                Some('t') => UNDER,
                Some(_) => BODY,
                None => continue,
            };
            wb.set(x0 + k as i32, y0 + j as i32, c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WX: i32 = 21;

    fn samples() -> Vec<(f32, Shot)> {
        let d = duration(WX);
        (0..=(d / 0.01) as usize)
            .map(|i| (i as f32 * 0.01, shot(i as f32 * 0.01, WX)))
            .collect()
    }

    #[test]
    fn hat_comes_off_and_goes_back() {
        assert_eq!(shot(0.0, WX).hat, (0.0, 0.0));
        let held = REACH + LIFT + WAIT * 0.5;
        assert_eq!(shot(held, WX).hat, HAT_OFF);
        let back = REACH + LIFT + WAIT + JUMP + RETURN;
        assert_eq!(shot(back, WX).hat, (0.0, 0.0));
        assert_eq!(shot(duration(WX), WX).hat, (0.0, 0.0));
        assert!(shot(REACH * 0.5, WX).hand.is_some());
        assert_eq!(shot(back + LOWER + 0.01, WX).hand, None);
        for (_, s) in samples() {
            assert!((0.0..=HAT_OFF.0).contains(&s.hat.0) && (0.0..=HAT_OFF.1).contains(&s.hat.1));
        }
    }

    #[test]
    fn toad_is_uncovered_then_leaps_to_the_floor() {
        assert_eq!(shot(REACH * 0.5, WX).toad, None);
        assert!(shot(REACH + 0.001, WX).shown < 0.1);
        let sitting = shot(REACH + LIFT + WAIT * 0.5, WX);
        assert_eq!(sitting.toad, Some((24.0, 6.0, SIT)));
        assert_eq!(sitting.shown, WIDTH);
        let mid = shot(REACH + LIFT + WAIT + JUMP * 0.5, WX).toad.unwrap();
        assert_eq!(mid.2, LEAP);
        let landed = shot(REACH + LIFT + WAIT + JUMP + 0.01, WX);
        assert_eq!(landed.toad, Some((7.0, FLOOR, SIT)));
        assert_eq!(landed.landings, 1);
    }

    #[test]
    fn lands_inside_a_narrow_strip() {
        // N = 34: 14 px left of her would be off the strip's edge.
        let landed = shot(REACH + LIFT + WAIT + JUMP + 0.01, 9);
        assert_eq!(landed.toad, Some((0.0, FLOOR, SIT)));
        assert!(shot(duration(9), 9).toad.unwrap().0 <= -WIDTH);
    }

    #[test]
    fn hops_slowly_out_at_the_left() {
        let s = samples();
        let toads: Vec<_> = s.iter().filter_map(|(_, s)| s.toad).collect();
        assert!(
            toads.windows(2).all(|w| w[1].0 <= w[0].0),
            "never hops back"
        );
        assert!(toads.iter().all(|t| t.1 <= FLOOR), "never below the floor");
        assert!(s.windows(2).all(|w| w[1].1.landings >= w[0].1.landings));
        let end = shot(duration(WX), WX);
        assert!(end.toad.unwrap().0 <= -WIDTH);
        assert_eq!(end.landings, 1 + 3, "18 px to cover in 6 px hops");
        assert!((9.0..16.0).contains(&duration(WX)), "{}", duration(WX));
    }
}
