//! Familiars: three rare, quiet scenes in the sigil pane. A black cat
//! wanders past the witch, a swarm of bats spirals across the sigil, and a
//! large toad jumps out from under the witch's hat. Each scene is a pure
//! function from the seconds elapsed to what is where.

pub mod bats;
pub mod cat;
pub mod toad;

use super::anim::{Particle, rd, witch_x};
use super::fit::Fit;
use super::sprites::{Assets, WitchSize};
use crate::palette::{CYAN, GOLD, mul};
use crate::render::raster::Buf;
use std::collections::VecDeque;

/// The row of the witch's strip that things stand on.
pub const FLOOR: f32 = 22.0;
/// Seconds a spark from the toad's landing lasts.
const SPARK_LIFE: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Cat,
    Bats,
    Toad,
}

impl Scene {
    pub const ALL: [Scene; 3] = [Scene::Cat, Scene::Bats, Scene::Toad];

    pub fn name(self) -> &'static str {
        match self {
            Scene::Cat => "cat",
            Scene::Bats => "bats",
            Scene::Toad => "toad",
        }
    }

    pub fn parse(name: &str) -> Option<Scene> {
        match name {
            "cat" => Some(Scene::Cat),
            "bats" => Some(Scene::Bats),
            "toad" => Some(Scene::Toad),
            _ => None,
        }
    }
}

/// What the pane has room for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stage {
    /// The sigil's size in pixels.
    pub n: usize,
    /// The big witch's left edge in her strip, if she is shown.
    pub witch_x: Option<i32>,
}

impl Stage {
    pub fn of(fit: &Fit, assets: &Assets) -> Stage {
        let big = matches!(fit.witch, Some((WitchSize::Big, _)));
        Stage {
            n: fit.n,
            witch_x: big.then(|| witch_x(fit.n, &assets.big)),
        }
    }

    /// The cat and the toad need the big witch; the bats a sigil big
    /// enough to read.
    pub fn fits(&self, scene: Scene) -> bool {
        match scene {
            Scene::Cat | Scene::Toad => self.witch_x.is_some(),
            Scene::Bats => self.n >= bats::MIN_N,
        }
    }

    /// How long `scene` lasts here, in seconds.
    fn duration(&self, scene: Scene) -> f32 {
        let witch_x = self.witch_x.unwrap_or(0);
        match scene {
            Scene::Cat => cat::duration(self.n, witch_x),
            Scene::Bats => bats::duration(),
            Scene::Toad => toad::duration(witch_x),
        }
    }
}

/// The chance a scene starts on one frame: once an hour on average.
pub fn chance(fps: u8) -> f64 {
    1.0 / (3600.0 * f64::from(fps.max(1)))
}

/// Decides when a scene plays, and keeps the one playing. It has its own
/// random numbers, so the sigil's are the same with or without it.
pub struct Familiars {
    rng: fastrand::Rng,
    fps: u8,
    /// The scene playing and its seconds so far.
    playing: Option<(Scene, f32)>,
    /// The toad's turn has come, but the witch's arms are not down yet.
    toad_waiting: bool,
    /// Scenes to play next without the dice.
    forced: VecDeque<Scene>,
    /// What the pane had room for at the last `step`.
    stage: Stage,
    /// The bats of the swarm flying.
    swarm: Vec<bats::Bat>,
    /// Sparks from the toad's landings, and how many landings have had
    /// theirs. Their `life` is in seconds.
    sparks: Vec<Particle>,
    landings: u32,
}

impl Familiars {
    pub fn new(seed: u64, fps: u8) -> Self {
        Familiars {
            rng: fastrand::Rng::with_seed(seed),
            fps: fps.max(1),
            playing: None,
            toad_waiting: false,
            forced: VecDeque::new(),
            stage: Stage {
                n: 0,
                witch_x: None,
            },
            swarm: Vec::new(),
            sparks: Vec::new(),
            landings: 0,
        }
    }

    /// Once per frame, before drawing: advance the scene playing, or see
    /// whether one starts. `arms` is how far the witch's arms are raised.
    pub fn step(&mut self, stage: Stage, arms: f32) {
        self.stage = stage;
        if let Some((scene, t)) = &mut self.playing {
            *t += 1.0 / f32::from(self.fps);
            // Over, or the pane was resized and can't show it any more.
            if *t >= stage.duration(*scene) || !stage.fits(*scene) {
                self.playing = None;
            }
            return;
        }
        self.toad_waiting &= stage.fits(Scene::Toad);
        let mut next = self.forced.pop_front().filter(|s| stage.fits(*s));
        if next.is_none() {
            let chance = chance(self.fps);
            next = Scene::ALL
                .into_iter()
                .find(|s| stage.fits(*s) && self.rng.f64() < chance);
        }
        // The toad needs her arms down: she takes her hat off with one.
        if next == Some(Scene::Toad) {
            self.toad_waiting = true;
            next = None;
        }
        if next.is_none() && self.toad_waiting && arms == 0.0 {
            self.toad_waiting = false;
            next = Some(Scene::Toad);
        }
        if let Some(scene) = next {
            self.playing = Some((scene, 0.0));
            self.sparks.clear();
            self.landings = 0;
            if scene == Scene::Bats {
                self.swarm = bats::swarm(stage.n, &mut self.rng);
            }
        }
    }

    /// The cat, on the witch's strip, before she is drawn: it passes
    /// behind her.
    pub(crate) fn draw_behind_witch(&self, wb: &mut Buf, assets: &Assets) {
        if let (Some((Scene::Cat, t)), Some(witch_x)) = (self.playing, self.stage.witch_x) {
            cat::draw(wb, &assets.cat, &cat::shot(t, self.stage.n, witch_x));
        }
    }

    /// What the toad's scene asks of the witch this frame, if it is playing.
    pub(crate) fn toad_shot(&self) -> Option<toad::Shot> {
        match (self.playing, self.stage.witch_x) {
            (Some((Scene::Toad, t)), Some(witch_x)) => Some(toad::shot(t, witch_x)),
            _ => None,
        }
    }

    /// The toad and the sparks of its landings, on the strip, after the
    /// witch and her rim light.
    pub(crate) fn draw_in_front_of_witch(&mut self, wb: &mut Buf, assets: &Assets) {
        let Some(shot) = self.toad_shot() else {
            return;
        };
        toad::draw(wb, &assets.toad, &shot);
        if let Some((x, _, _)) = shot.toad
            && shot.landings > self.landings
        {
            for _ in 0..3 {
                let c = if self.rng.bool() { CYAN } else { GOLD };
                self.sparks.push(Particle {
                    x: x + self.rng.f32() * toad::WIDTH,
                    y: FLOOR,
                    vx: (self.rng.f32() - 0.5) * 6.0,
                    vy: -3.0 - self.rng.f32() * 6.0,
                    life: SPARK_LIFE,
                    c: mul(c, 0.5),
                });
            }
        }
        self.landings = shot.landings;
        let dt = 1.0 / f32::from(self.fps);
        self.sparks.retain(|s| s.life > 0.0);
        for s in &mut self.sparks {
            wb.add(rd(s.x), rd(s.y), s.c, s.life / SPARK_LIFE);
            s.x += s.vx * dt;
            s.y += s.vy * dt;
            s.life -= dt;
        }
    }

    /// The bats, on the whole pane, over everything.
    pub(crate) fn draw_over(&self, buf: &mut Buf, fit: &Fit, assets: &Assets) {
        if let Some((Scene::Bats, t)) = self.playing {
            bats::draw(buf, &assets.bat, &self.swarm, t, fit.n, fit.sigil_xy);
        }
    }

    /// Play `scene` next, without the dice (the preview and tests).
    pub fn force(&mut self, scene: Scene) {
        self.forced.push_back(scene);
    }

    /// The scene playing and its seconds so far.
    pub fn playing(&self) -> Option<(Scene, f32)> {
        self.playing
    }

    /// Nothing is playing, waiting or forced.
    pub fn idle(&self) -> bool {
        self.playing.is_none() && !self.toad_waiting && self.forced.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIG: Stage = Stage {
        n: 58,
        witch_x: Some(21),
    };
    const MINI: Stage = Stage {
        n: 29,
        witch_x: None,
    };
    const TINY: Stage = Stage {
        n: 8,
        witch_x: None,
    };

    /// Scene starts over `hours` at 1 fps: [cat, bats, toad].
    fn starts(stage: Stage, arms: f32, hours: usize) -> [usize; 3] {
        let mut f = Familiars::new(11, 1);
        let mut counts = [0; 3];
        let mut was = None;
        for _ in 0..hours * 3600 {
            f.step(stage, arms);
            let now = f.playing().map(|p| p.0);
            if let (None, Some(s)) = (was, now) {
                counts[Scene::ALL.iter().position(|a| *a == s).unwrap()] += 1;
            }
            was = now;
        }
        counts
    }

    #[test]
    fn each_scene_about_once_an_hour() {
        for c in starts(BIG, 0.0, 3000) {
            assert!((2700..=3300).contains(&c), "{c} starts in 3000 hours");
        }
        assert_eq!(chance(12), 1.0 / 43_200.0);
        assert_eq!(chance(60), 1.0 / 216_000.0);
    }

    #[test]
    fn small_panes_get_only_the_bats() {
        let [cat, bats, toad] = starts(MINI, 0.0, 300);
        assert_eq!((cat, toad), (0, 0));
        assert!(bats > 200);
        assert_eq!(starts(TINY, 0.0, 300), [0, 0, 0]);
    }

    #[test]
    fn a_sigil_too_small_for_the_swarm_gets_no_bats() {
        // The bats don't shrink with the sigil: on a small one the swarm
        // would blot out a third of it.
        let sigil = |n| Stage { n, witch_x: None };
        assert!(!sigil(20).fits(Scene::Bats));
        assert!(sigil(26).fits(Scene::Bats));
        assert_eq!(starts(sigil(20), 0.0, 300), [0, 0, 0]);
    }

    #[test]
    fn toad_waits_for_her_arms_and_blocks_nothing() {
        let [cat, bats, toad] = starts(BIG, 1.0, 300);
        assert_eq!(toad, 0);
        assert!(cat > 200 && bats > 200);
        let mut f = Familiars::new(1, 12);
        f.force(Scene::Toad);
        f.step(BIG, 0.5);
        assert_eq!(f.playing(), None);
        assert!(!f.idle(), "it is waiting");
        f.step(BIG, 0.0);
        assert_eq!(f.playing().map(|p| p.0), Some(Scene::Toad));
    }

    #[test]
    fn a_scene_the_pane_cannot_show_is_dropped() {
        let mut f = Familiars::new(1, 12);
        f.force(Scene::Cat);
        f.step(MINI, 0.0);
        assert!(f.idle(), "never starts, never waits");
        f.force(Scene::Cat);
        f.step(BIG, 0.0);
        f.step(BIG, 0.0);
        assert!(f.playing().is_some());
        f.step(MINI, 0.0); // resized in mid-scene
        assert!(f.idle());
        f.force(Scene::Toad);
        f.step(BIG, 1.0); // waiting
        f.step(MINI, 1.0);
        assert!(f.idle());
    }

    #[test]
    fn scenes_last_the_same_at_any_fps() {
        for fps in [1u8, 12, 60] {
            let mut f = Familiars::new(1, fps);
            f.force(Scene::Cat);
            let mut frames = 0;
            loop {
                f.step(BIG, 0.0);
                if f.idle() {
                    break;
                }
                frames += 1;
                assert!(frames < 100_000, "never ends at {fps} fps");
            }
            let secs = frames as f32 / f32::from(fps);
            assert!(
                (secs - cat::duration(58, 21)).abs() <= 1.5,
                "{secs} s at {fps} fps"
            );
        }
    }
}
