# Familiars Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three rare, quiet scenes in the sigil pane (a cat, a swarm of bats, a toad under the witch's hat), each starting at random about once an hour.

**Architecture:** Each scene is a pure function from seconds elapsed to a "shot" (what is where, in which pose), in its own file under `src/sigil/familiars/`. A small scheduler (`Familiars`) owns the clock, the dice and the scene playing, and exposes drawing hooks that `SigilAnim::draw` calls at three points: behind the witch, in front of her, and over the whole pane. Poses are text sprites in `assets/`. Familiars are off unless the sigil pane's loop or the hidden preview command switches them on.

**Tech Stack:** Rust 1.88 (edition 2024), `fastrand`, the project's own `Buf` pixel canvas. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-16-familiars-design.md`

## Global Constraints

- No config key, no README entry, no change to `herdr-plugin.toml`, the daemon, the state file or the grimoire pane.
- With familiars off, and with them on but no scene playing, every frame is byte-for-byte what it is today. `tests/frame_snapshots.rs` snapshots must not change.
- Familiars never draw from `SigilAnim::rng`; the existing animation's random sequence is untouched, during a scene too.
- With no scene playing the cost per frame is at most three random numbers.
- A scene never changes the sigil's glow, lightning or the witch's own rim light.
- Nothing moves fast or flashes. Where a duration in the spec and this rule disagree, slow wins.
- Scene time is in seconds: the clock advances `1 / fps` per frame drawn.
- Each scene starts with probability `1 / (3600 × fps)` per frame while nothing is playing.
- The code calls the feature "familiars". The hidden command is `herdr-witch scene <cat|bats|toad|all> [cols]` and is left out of the usage text.
- Strip coordinates: the witch's strip is N wide and 24 px tall, row 0 at the top, row 23 the floor line; things stand with their feet on row 22. The big witch's sprite has its top-left at (`witch_x`, 1).

## Review Focus

Conditions the spec implies but states no test for, most likely first. Each has its test in the task named.

1. **A hand-edited or truncated art file** (missing poses, ragged rows, empty). The built-in art is used; nothing panics or goes blank. (Task 1)
2. **The preview in a terminal that cannot show the scene, or with a bad name.** It prints usage or plays nothing, and always exits by itself: Ctrl-C is ignored there. (Task 7)
3. **A resize in mid-scene** to a pane without the big witch, or tiny. The scene is dropped and the next frame draws normally. (Tasks 5 and 6)
4. **Typing during the scenes.** A waiting toad never blocks the cat or bats; typing while the hat is off raises the other arm and leaves the existing random sequence as it was. (Tasks 5 and 6)
5. **`fps` at 1 and at 60.** Scenes last the same number of seconds; at 1 fps a 0.15 s blink may never be drawn but the scene still ends. (Task 5)

---

### Task 1: Text poses and the new art

**Files:**
- Create: `assets/cat.txt`, `assets/toad.txt`, `assets/bat.txt`, `assets/witch_bare.txt`
- Modify: `src/sigil/sprites.rs`

**Interfaces:**
- Produces:
  ```rust
  /// A text sprite kept as characters; the scene that draws it maps the
  /// letters to colours. `.` and space are empty.
  #[derive(Clone, Debug, PartialEq)]
  pub struct Pose { pub w: usize, pub h: usize, pub px: Vec<char> }
  impl Pose {
      /// The letter at (`x`, `y`); `None` if empty or outside.
      pub fn at(&self, x: usize, y: usize) -> Option<char>;
  }
  /// Poses separated by blank lines, each padded to its widest row.
  pub fn parse_poses(txt: &str) -> Vec<Pose>;
  ```
  and on `Assets`: `pub cat: Vec<Pose>` (4), `pub toad: Vec<Pose>` (2), `pub bat: Vec<Pose>` (2), `pub bare: Pose`.

First drafts of the art (Task 8 tunes them). Letters: `C` cat, `o` eye, `T` toad, `t` toad's underside, `B` bat, `K` hair.

`assets/cat.txt`, four 12×7 poses in this order: walking A, walking B, sitting and looking up, sitting and facing us.

```
.C.C.......C
.CCC.......C
CoCCC.....C.
.CCCCCCCCCC.
..CCCCCCCCC.
..C.C...C.C.
.C..C..C..C.

.C.C.......C
.CCC.......C
CoCCC.....C.
.CCCCCCCCCC.
..CCCCCCCCC.
..C.C...C.C.
..C..C..C.C.

.C.C........
CoCC........
CCCC........
.CCCC.......
.CCCCC......
.CCCCCC...C.
.C.CCCCCCCC.

.C...C......
.CCCCC......
.CoCoC......
.CCCCC......
..CCCC......
.CCCCCC...C.
.C.CCCCCCCC.
```

`assets/toad.txt`, two 9×6 poses: sitting (facing us), leaping (to the left).

```
.oT...To.
TTTTTTTTT
TTTTTTTTT
TTTTTTTTT
tTTtttTTt
tt.....tt

.oTT.....
TTTTTTT..
.TTTTTTTT
..tTTTTTt
....t..tt
.......t.
```

`assets/bat.txt`, two 3×2 poses:

```
B.B
.B.

.B.
B.B
```

`assets/witch_bare.txt`, one 9×4 pose, the top of her head:

```
..KKKKK..
.KKKKKKK.
KKKKKKKKK
KKKKKKKKK
```

- [ ] **Step 1: Write the failing tests** in `sprites.rs`'s `tests` module

```rust
#[test]
fn embedded_poses_parse() {
    let a = load_assets(None);
    assert_eq!(a.cat.len(), 4);
    assert!(a.cat.iter().all(|p| (p.w, p.h) == (12, 7)));
    assert_eq!(a.toad.len(), 2);
    assert!(a.toad.iter().all(|p| (p.w, p.h) == (9, 6)));
    assert_eq!(a.bat.len(), 2);
    assert_eq!((a.bare.w, a.bare.h), (9, 4));
    assert_eq!(a.cat[3].at(2, 2), Some('o'));
    assert_eq!(a.cat[3].at(0, 0), None);
    assert_eq!(a.cat[3].at(99, 0), None);
}

#[test]
fn poses_pad_ragged_rows() {
    let p = parse_poses("AB\nA\n\n\nC\n");
    assert_eq!(p.len(), 2);
    assert_eq!((p[0].w, p[0].h), (2, 2));
    assert_eq!(p[0].at(1, 1), None);
    assert_eq!(p[1].at(0, 0), Some('C'));
}

#[test]
fn bad_pose_files_fall_back() {
    let root = std::env::temp_dir().join(format!("hw-poses-{}", std::process::id()));
    std::fs::create_dir_all(root.join("assets")).unwrap();
    std::fs::write(root.join("assets/cat.txt"), "C\n").unwrap(); // one pose, not four
    std::fs::write(root.join("assets/toad.txt"), "").unwrap();
    let a = load_assets(Some(&root));
    assert_eq!(a.cat, load_assets(None).cat);
    assert_eq!(a.toad, load_assets(None).toad);
}
```

- [ ] **Step 2: Run** `cargo test --locked --lib sprites` — expect compile errors (`Pose`, `parse_poses`, the `Assets` fields).

- [ ] **Step 3: Implement.** `Pose`, `Pose::at`, `parse_poses`; the four `include_str!` fallbacks; in `load_assets`, a helper `read_poses(root, name, fallback, count) -> Vec<Pose>` that uses the file's poses only if there are exactly `count` of them and none is empty, else the fallback's.

- [ ] **Step 4: Run** `cargo test --locked --lib sprites` — expect PASS.

- [ ] **Step 5: Commit** `feat: text poses for the cat, toad, bats and the witch's bare head`

---

### Task 2: The cat's timeline

**Files:**
- Create: `src/sigil/familiars/mod.rs` (only `pub mod cat;` for now), `src/sigil/familiars/cat.rs`
- Modify: `src/sigil/mod.rs` (`pub mod familiars;`)

**Interfaces:**
- Produces, in `cat.rs`:
  ```rust
  pub const WALK_A: usize = 0;
  pub const WALK_B: usize = 1;
  pub const LOOK_UP: usize = 2;
  pub const FRONT: usize = 3;
  /// Pixels per second.
  pub const SPEED: f32 = 4.0;
  /// The cat's width in pixels.
  pub const WIDTH: f32 = 12.0;
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub enum Eyes { Open, Shut, /** 0 = amber, 1 = red. */ Red(f32) }
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Shot { /** Left edge, strip pixels. */ pub x: f32, pub pose: usize, pub eyes: Eyes }
  /// Where the cat stops (its left edge): 3 px of floor clear of the
  /// witch's robe, whose right edge is sprite column 15.
  pub fn stop_x(witch_x: i32) -> f32;      // witch_x + 19
  pub fn duration(n: usize, witch_x: i32) -> f32;
  pub fn shot(t: f32, n: usize, witch_x: i32) -> Shot;
  ```

The timeline, in order: walk in from `x = n` to `stop_x` at `SPEED`, poses `WALK_A`/`WALK_B` swapping every 0.25 s; then stopped: `LOOK_UP` 2.9 s (eyes open); `FRONT` for the rest of the stop: open 1.0 s, shut 0.15 s, `Red(sin(π·u))` over 2.0 s, open 0.6 s, shut 0.15 s, open 0.8 s; then walk out to `x = -WIDTH`.

- [ ] **Step 1: Write the failing tests** in `cat.rs`

```rust
fn samples(n: usize, wx: i32) -> Vec<Shot> {
    let d = duration(n, wx);
    (0..=(d / 0.01) as usize).map(|i| shot(i as f32 * 0.01, n, wx)).collect()
}

#[test]
fn walks_in_stops_and_leaves_at_the_left() {
    let s = samples(58, 21);
    assert_eq!(s[0].x, 58.0);
    assert!(s.windows(2).all(|w| w[1].x <= w[0].x), "never walks back");
    let stopped = s.iter().filter(|c| c.x == stop_x(21)).count();
    assert!((750..=770).contains(&stopped), "stopped for 7.6 s: {stopped}");
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
    assert!(s.iter().filter(|c| c.x != stop_x(21)).all(|c| c.pose <= WALK_B));
    assert!(s.iter().filter(|c| c.pose != FRONT).all(|c| c.eyes == Eyes::Open));
}

#[test]
fn a_narrow_strip_still_ends() {
    // N = 34: the cat stops half off the right edge, and still leaves.
    let s = samples(34, 9);
    assert!(s.iter().all(|c| c.x.is_finite()));
    assert!(shot(duration(34, 9), 34, 9).x <= -WIDTH);
}
```

- [ ] **Step 2: Run** `cargo test --locked --lib familiars::cat` — expect compile errors.
- [ ] **Step 3: Implement** `stop_x`, `duration`, `shot`. When `stop_x >= n` the walk-in has zero length.
- [ ] **Step 4: Run** `cargo test --locked --lib familiars::cat` — expect PASS.
- [ ] **Step 5: Commit** `feat: the cat's timeline`

---

### Task 3: The bats' timeline

**Files:**
- Create: `src/sigil/familiars/bats.rs`
- Modify: `src/sigil/familiars/mod.rs` (`pub mod bats;`)

**Interfaces:**
- Produces:
  ```rust
  /// The smallest sigil that gets bats.
  pub const MIN_N: usize = 16;
  pub const FLY_IN: f32 = 2.0;
  pub const SPIRAL: f32 = 9.0;
  pub const FLY_OUT: f32 = 2.0;
  /// Seconds between the first bat and the last.
  pub const SPREAD: f32 = 1.2;
  #[derive(Clone, Debug)]
  pub struct Bat { pub delay: f32, pub phase: f32, pub amp: f32 }
  /// `(n / 8).clamp(5, 9)` bats: delays in `0.0..=SPREAD` (the first is 0),
  /// phases in `0..TAU`, wobble `amp` in `1.0..=2.5` px.
  pub fn swarm(n: usize, rng: &mut fastrand::Rng) -> Vec<Bat>;
  pub fn duration() -> f32;                       // FLY_IN + SPIRAL + FLY_OUT + SPREAD
  /// The swarm's centre in sigil pixels (the sigil is the box 0..n).
  pub fn path(t: f32, n: f32) -> (f32, f32);
  /// One bat: its position and flap pose (0 or 1, three flaps a second).
  pub fn bat_at(b: &Bat, t: f32, n: f32) -> ((f32, f32), usize);
  ```

The path, with `c = (n-1)/2`, `r0 = 0.42n`, `r1 = 0.16n`:

1. `FLY_IN`: a straight line from `(-6, c - r0 - 0.1n)` to the top of the outer turn, `(c, c - r0)`.
2. `SPIRAL`: angle `θ = 1.5π + 4π·s` and radius `r0 + (r1 - r0)·s` for `s` in 0..1, at `c + r·(cos θ, sin θ)`. Two clockwise turns, ending at the top heading right.
3. `FLY_OUT`: a straight line from `(c, c - r1)` to `(n + 6, c - r1 - 0.1n)`.

`t` is clamped to the path's ends. `bat_at` is `path(t - delay)` plus `amp·(sin(2.3t + phase), cos(1.7t + 2·phase))`.

- [ ] **Step 1: Write the failing tests** in `bats.rs`

```rust
const N: f32 = 58.0;
const C: f32 = 28.5;

fn radius(t: f32) -> f32 {
    let (x, y) = path(t, N);
    (x - C).hypot(y - C)
}

#[test]
fn in_from_the_top_left_and_out_at_the_right() {
    let (x, y) = path(0.0, N);
    assert!(x < 0.0 && y < N * 0.25, "starts beyond the top left: {x},{y}");
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
        turned += (d + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    }
    assert!((turned - 4.0 * std::f32::consts::PI).abs() < 0.05, "{turned}");
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
    assert!(s.iter().all(|b| (0.0..=SPREAD).contains(&b.delay) && (1.0..=2.5).contains(&b.amp)));
    let ((x, _), pose) = bat_at(&s[0], 0.0, N);
    assert!(x < 3.0 && pose < 2);
}
```

- [ ] **Step 2: Run** `cargo test --locked --lib familiars::bats` — expect compile errors.
- [ ] **Step 3: Implement** the five functions.
- [ ] **Step 4: Run** `cargo test --locked --lib familiars::bats` — expect PASS. If `never_fast_and_never_jumps` fails, lengthen `SPIRAL` (slow wins), not the limit.
- [ ] **Step 5: Commit** `feat: the bats' timeline`

---

### Task 4: The toad's timeline

**Files:**
- Create: `src/sigil/familiars/toad.rs`
- Modify: `src/sigil/familiars/mod.rs` (`pub mod toad;`)

**Interfaces:**
- Produces:
  ```rust
  pub const SIT: usize = 0;
  pub const LEAP: usize = 1;
  /// The rows of `witch_big.txt` that are the hat.
  pub const HAT_ROWS: usize = 10;
  /// Where she holds the hat, from where it sits.
  pub const HAT_OFF: (f32, f32) = (9.0, 7.0);
  /// The toad's width in pixels.
  pub const WIDTH: f32 = 9.0;
  /// The row things stand on.
  pub const FLOOR: f32 = 22.0;
  pub const REACH: f32 = 0.8;
  pub const LIFT: f32 = 1.0;
  pub const WAIT: f32 = 1.2;
  pub const JUMP: f32 = 0.9;
  pub const RETURN: f32 = 1.2;
  pub const LOWER: f32 = 0.6;
  pub const HOP: f32 = 0.5;
  pub const REST: f32 = 1.2;
  pub const HOP_LEN: f32 = 6.0;
  pub const HOP_HIGH: f32 = 3.0;
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct Shot {
      /// The hat's offset from where it sits.
      pub hat: (f32, f32),
      /// The hat arm's hand in witch-sprite pixels; `None` once the arm is hers again.
      pub hand: Option<(f32, f32)>,
      /// The toad: left edge and feet row in strip pixels, and its pose.
      pub toad: Option<(f32, f32, usize)>,
      /// How many of the toad's columns the hat has uncovered (`WIDTH` = all).
      pub shown: f32,
      /// Landings so far: the leap, then each hop.
      pub landings: u32,
  }
  pub fn duration(witch_x: i32) -> f32;
  pub fn shot(t: f32, witch_x: i32) -> Shot;
  ```

The timeline:

1. `REACH`: the hand goes from where the lowered arm leaves it, sprite (15.05, 18.73), to the hat's brim at (15, 8). No toad yet.
2. `LIFT`: with `e` an ease from 0 to 1, the hat's offset is `(HAT_OFF.0·e, HAT_OFF.1·e²)` (sideways first, then down); the hand is the brim point plus the offset. The toad sits on her head: left edge `witch_x + 4`, feet row 6, pose `SIT`, `shown = WIDTH·e`.
3. `WAIT`: everything holds.
4. `JUMP`: the toad leaps (pose `LEAP`) to left edge `witch_x - 8`, feet row `FLOOR`, on an arc that first rises 3 px. One landing.
5. `RETURN`, then `LOWER`: the hat goes back by the same curve reversed, then the hand returns to (15.05, 18.73) and `hand` becomes `None`.
6. From the landing, the toad rests `REST`, then hops (`HOP` in the air with pose `LEAP`, `HOP_LEN` to the left, `HOP_HIGH` high, then `REST` in pose `SIT`), a landing each, until its left edge is at or beyond `-WIDTH`. Steps 5 and 6 overlap; `duration` is 0.2 s after the later of them ends, and `shot` holds its last state from then on.

- [ ] **Step 1: Write the failing tests** in `toad.rs`

```rust
const WX: i32 = 21;

fn samples() -> Vec<(f32, Shot)> {
    let d = duration(WX);
    (0..=(d / 0.01) as usize).map(|i| (i as f32 * 0.01, shot(i as f32 * 0.01, WX))).collect()
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
    assert_eq!(sitting.toad, Some((25.0, 6.0, SIT)));
    assert_eq!(sitting.shown, WIDTH);
    let mid = shot(REACH + LIFT + WAIT + JUMP * 0.5, WX).toad.unwrap();
    assert_eq!(mid.2, LEAP);
    let landed = shot(REACH + LIFT + WAIT + JUMP + 0.01, WX);
    assert_eq!(landed.toad, Some((13.0, FLOOR, SIT)));
    assert_eq!(landed.landings, 1);
}

#[test]
fn hops_slowly_out_at_the_left() {
    let s = samples();
    let toads: Vec<_> = s.iter().filter_map(|(_, s)| s.toad).collect();
    assert!(toads.windows(2).all(|w| w[1].0 <= w[0].0), "never hops back");
    assert!(toads.iter().all(|t| t.1 <= FLOOR), "never below the floor");
    assert!(s.windows(2).all(|w| w[1].1.landings >= w[0].1.landings));
    let end = shot(duration(WX), WX);
    assert!(end.toad.unwrap().0 <= -WIDTH);
    assert_eq!(end.landings, 1 + 4, "22 px to cover in 6 px hops");
    assert!((9.0..16.0).contains(&duration(WX)), "{}", duration(WX));
}
```

- [ ] **Step 2: Run** `cargo test --locked --lib familiars::toad` — expect compile errors.
- [ ] **Step 3: Implement** `duration` and `shot`.
- [ ] **Step 4: Run** `cargo test --locked --lib familiars::toad` — expect PASS.
- [ ] **Step 5: Commit** `feat: the toad's timeline`

---

### Task 5: The scheduler

**Files:**
- Modify: `src/sigil/familiars/mod.rs`, `src/sigil/anim.rs` (expose the witch's x)

**Interfaces:**
- Consumes: `cat::duration(n, witch_x)`, `bats::duration()`, `bats::MIN_N`, `bats::swarm(n, rng)`, `toad::duration(witch_x)`.
- Produces, in `anim.rs`:
  ```rust
  /// The big witch's left edge in her strip.
  pub(crate) fn witch_x(n: usize, spr: &Sprite) -> i32;   // what draw_witch computes as `sx`
  ```
  and in `familiars/mod.rs`:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum Scene { Cat, Bats, Toad }
  impl Scene {
      pub const ALL: [Scene; 3] = [Scene::Cat, Scene::Bats, Scene::Toad];
      pub fn parse(name: &str) -> Option<Scene>;          // "cat" | "bats" | "toad"
  }
  /// What the pane has room for.
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct Stage {
      pub n: usize,
      /// The big witch's left edge in her strip, if she is shown.
      pub witch_x: Option<i32>,
  }
  impl Stage {
      pub fn of(fit: &Fit, assets: &Assets) -> Stage;
      /// Cat and toad need the big witch; bats need `n >= bats::MIN_N`.
      pub fn fits(&self, scene: Scene) -> bool;
  }
  /// The chance a scene starts on one frame: once an hour on average.
  pub fn chance(fps: u8) -> f64;                          // 1.0 / (3600.0 * fps)
  pub struct Familiars { /* private */ }
  impl Familiars {
      pub fn new(seed: u64, fps: u8) -> Self;
      /// Once per frame, before drawing.
      pub fn step(&mut self, stage: Stage, arms: f32);
      /// Play `scene` next, without the dice (the preview and tests).
      pub fn force(&mut self, scene: Scene);
      /// The scene playing and its seconds so far.
      pub fn playing(&self) -> Option<(Scene, f32)>;
      /// Nothing is playing, waiting or forced.
      pub fn idle(&self) -> bool;
  }
  ```

`step`, in order:

1. If a scene is playing: add `1 / fps` to its clock; drop it if the stage no longer fits it or its duration has passed. Return.
2. Drop a waiting toad the stage no longer fits.
3. Take the next forced scene if the stage fits it (one it does not fit is dropped). With no forced scene to start, roll `rng.f64() < chance(fps)` for each of `Scene::ALL` the stage fits. The first hit starts, except a toad, which is marked waiting.
4. If nothing started and a toad is waiting and `arms == 0.0`, start it. A forced toad waits the same way.

Starting the bats draws their `swarm` from the familiars' own generator.

- [ ] **Step 1: Write the failing tests** in `familiars/mod.rs`

```rust
const BIG: Stage = Stage { n: 58, witch_x: Some(21) };
const MINI: Stage = Stage { n: 29, witch_x: None };
const TINY: Stage = Stage { n: 8, witch_x: None };

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
        assert!((secs - cat::duration(58, 21)).abs() <= 1.5, "{secs} s at {fps} fps");
    }
}
```

- [ ] **Step 2: Run** `cargo test --locked --lib familiars::tests` — expect compile errors.
- [ ] **Step 3: Implement** `witch_x` (and use it in `draw_witch`), `Scene`, `Stage`, `chance`, `Familiars`.
- [ ] **Step 4: Run** `cargo test --locked --lib familiars` — expect PASS (all four modules).
- [ ] **Step 5: Commit** `feat: the familiars' scheduler`

---

### Task 6: Drawing the scenes

**Files:**
- Modify: `src/sigil/familiars/mod.rs`, `src/sigil/familiars/cat.rs`, `src/sigil/familiars/bats.rs`, `src/sigil/familiars/toad.rs`, `src/sigil/anim.rs`

**Interfaces:**
- Consumes: everything Tasks 1 to 5 produce.
- Produces, on `SigilAnim`:
  ```rust
  /// `None` unless switched on: the pane's loop and the preview do that.
  pub familiars: Option<Familiars>,
  pub fn enable_familiars(&mut self, fps: u8);      // seeded from the anim's own seed
  ```
  on `Familiars` (all `pub(crate)`):
  ```rust
  /// The cat, on the witch's strip, before she is drawn.
  fn draw_behind_witch(&self, wb: &mut Buf, assets: &Assets);
  /// What the toad scene asks of the witch this frame.
  fn toad_shot(&self) -> Option<toad::Shot>;
  /// The toad and its sparks, on the strip, after the witch and her rim light.
  fn draw_in_front_of_witch(&mut self, wb: &mut Buf, assets: &Assets);
  /// The bats, on the whole pane, after both blits and before `quant_octant`.
  fn draw_over(&self, buf: &mut Buf, fit: &Fit, assets: &Assets);
  ```
  and the colours, as `pub const` in the scene's file:
  `cat::BODY = [3.0, 2.0, 6.0]`, `cat::RIM = [32.0, 14.0, 49.0]`, `cat::AMBER = [230.0, 160.0, 50.0]`, `cat::RED = [230.0, 40.0, 30.0]`, `bats::BODY = [40.0, 24.0, 62.0]`, `toad::BODY = [62.0, 104.0, 58.0]`, `toad::UNDER = [38.0, 66.0, 44.0]`, `toad::EYE = [220.0, 180.0, 70.0]`.

How each is drawn:

- **Cat:** each `C` is `cat::BODY`; each `o` is amber, body-coloured when shut, or `lerp_c(AMBER, RED, b)`. Every body pixel with no body pixel above it adds `cat::RIM` to the pixel above. The pose's bottom row is on strip row 22.
- **Witch with the toad's shot** (`draw_witch`): the sprite's rows from `HAT_ROWS` down as today; then, if `shot.hat != (0, 0)`, the bare head (hair colour `K`) at sprite (4, 6); then her free arm as today; then the hat rows at the rounded offset; then the hat arm as a line from her shoulder (13, 11) to `shot.hand`, same thickness and hand as her arms. All of it goes in the rim-light mask. With no shot, or `hand == None`, an arm is drawn exactly as today.
- **`SigilAnim::rng` during the toad scene:** `draw_witch` makes the same draws from it as it would without a scene. The hat arm's hand makes no sparks, but its random numbers are still drawn.
- **Toad:** `T`, `t`, `o` in its colours; only columns `< shown` while it sits on her head. On each new landing, three sparks at its feet from the familiars' own generator: cyan or gold at half brightness, drifting up, gone in half a second. Sparks left when the scene ends are dropped with it.
- **Bats:** each `B` set to `bats::BODY` at `fit.sigil_xy` plus the bat's position.

`SigilAnim::draw` calls `familiars.step(Stage::of(fit, assets), self.arms)` after `spawn`, then the four hooks at the points named.

- [ ] **Step 1: Write the failing tests** in `anim.rs`'s `tests` module

```rust
use crate::sigil::familiars::{Scene, bats, cat, toad};

/// A half-block pane 40 columns wide: N = 40, big witch, no quantising.
fn stage(cols: usize, rows: usize, mode: Mode, seed: u64) -> (Fit, SigilGeometry, Assets, Buf, SigilAnim) {
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
        assert!(on.familiars.as_ref().unwrap().idle(), "seed 5 rolls nothing in 600 frames");
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
    assert!(!has(&buf, cat::AMBER) && !has(&buf, cat::BODY), "gone by the end");
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
    for (c, r) in [(4, 2), (6, 12), (13, 30), (17, 30), (27, 40), (34, 40), (40, 60)] {
        for m in [Mode::HalfBlock, Mode::Octant] {
            for scene in Scene::ALL {
                let (f, geo, assets, mut buf, mut a) = stage(c, r, m, 3);
                a.familiars.as_mut().unwrap().force(scene);
                for _ in 0..400 {
                    a.step(8, true);
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
```

- [ ] **Step 2: Run** `cargo test --locked --lib sigil::anim` — expect compile errors.
- [ ] **Step 3: Implement** `enable_familiars`, the four hooks and the per-scene drawing, and the changes to `draw` and `draw_witch` described above.
- [ ] **Step 4: Run** `cargo test --locked` — expect PASS, with `tests/frame_snapshots.rs` and `tests/geometry_snapshots.rs` unchanged (no `.snap.new` files).
- [ ] **Step 5: Commit** `feat: draw the cat, the bats and the toad`

---

### Task 7: Switch them on, and the preview

**Files:**
- Modify: `src/sigil/mod.rs`, `src/main.rs`

**Interfaces:**
- Consumes: `SigilAnim::enable_familiars`, `Familiars::{force, idle}`, `Scene::{ALL, parse}`.
- Produces, in `sigil/mod.rs`:
  ```rust
  /// The preview's width unless told otherwise: a typical sigil pane.
  pub const PREVIEW_COLS: usize = 29;
  /// The scenes and width asked for: `<cat|bats|toad|all> [cols]`.
  fn parse_preview(args: &[String]) -> Option<(Vec<Scene>, usize)>;
  /// Plays scenes in turn with a second before, between and after.
  struct Preview { /* private */ }
  impl Preview {
      fn new(scenes: Vec<Scene>, fps: u8) -> Self;
      /// Once per frame, after drawing; `false` when the preview is over.
      fn next(&mut self, familiars: &mut Familiars) -> bool;
  }
  /// `herdr-witch scene ...`; returns the exit code (2 with a usage line
  /// on stderr for bad arguments).
  pub fn preview(cfg: &Config, args: &[String]) -> i32;
  ```

`run` calls `anim.enable_familiars(cfg.fps)` once, before its loop. `preview` draws like `run` but with `GlobalState::default()` every frame (never the daemon's), no hidden-tab gate, the terminal's rows, and `cols` clamped to the terminal's. `main.rs` gains `Some("scene") => std::process::exit(sigil::preview(&config::load(), &args[2..]))`; the usage string stays as it is.

- [ ] **Step 1: Write the failing tests** in `sigil/mod.rs`'s `tests` module

```rust
fn args(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

#[test]
fn preview_arguments() {
    assert_eq!(parse_preview(&args("cat")), Some((vec![Scene::Cat], PREVIEW_COLS)));
    assert_eq!(parse_preview(&args("all 40")), Some((Scene::ALL.to_vec(), 40)));
    for bad in ["", "dog", "cat x", "cat 40 9"] {
        assert_eq!(parse_preview(&args(bad)), None, "{bad:?}");
    }
}

/// Frames a preview of `scenes` runs for on `stage`.
fn preview_frames(scenes: Vec<Scene>, stage: Stage) -> u32 {
    let mut f = Familiars::new(1, 12);
    let mut p = Preview::new(scenes, 12);
    let mut frames = 0;
    loop {
        f.step(stage, 0.0);
        if !p.next(&mut f) {
            return frames;
        }
        frames += 1;
        assert!(frames < 12 * 120, "the preview must end by itself");
    }
}

#[test]
fn preview_plays_then_ends() {
    let big = Stage { n: 58, witch_x: Some(21) };
    let bats = (bats::duration() * 12.0) as u32;
    let one = preview_frames(vec![Scene::Bats], big);
    assert!((bats + 20..=bats + 30).contains(&one), "a second each side: {one}");
    let all = preview_frames(Scene::ALL.to_vec(), big);
    assert!(all > one * 3);
}

#[test]
fn preview_ends_when_nothing_can_play() {
    // A terminal too small for the witch: the cat is dropped, not waited for.
    let tiny = Stage { n: 8, witch_x: None };
    assert!(preview_frames(Scene::ALL.to_vec(), tiny) <= 12 * 6);
}
```

- [ ] **Step 2: Run** `cargo test --locked --lib sigil::tests` — expect compile errors.
- [ ] **Step 3: Implement** `parse_preview`, `Preview`, `preview`, the `run` change and the `main.rs` arm.
- [ ] **Step 4: Run** `cargo test --locked` — expect PASS. Then `cargo build --release --locked && target/release/herdr-witch scene dog; echo $?` — expect a usage line and `2`; and `target/release/herdr-witch 2>&1 | grep -c scene` — expect `0`.
- [ ] **Step 5: Commit** `feat: familiars in the sigil pane, and a hidden preview`

---

### Task 8: Tune the art and the pace by eye

**Files:**
- Modify: `assets/cat.txt`, `assets/toad.txt`, `assets/bat.txt`, `assets/witch_bare.txt`, the constants in `src/sigil/familiars/*.rs`, and any test value a changed constant moves.

There is no test for "it looks right". Look at each scene in the preview at 29 columns in octant mode, and at 40 columns with `render = "halfblock"`, and at the narrowest big-witch pane (17 columns, octant), and judge against the spec:

- [ ] **Step 1: The cat** reads as a cat in all four poses; its outline can just be made out; the eyes are the only thing that draws the eye, and only when looked at.
- [ ] **Step 2: The bats** are easy to miss against the background and show as moving gaps over the sigil's lines; nothing about them reads as fast.
- [ ] **Step 3: The toad** is clearly a toad and clearly large; the hat reads as taken off and put back; the hat arm does not look broken; the reveal has no pop.
- [ ] **Step 4: The narrowest pane** (N = 34): the cat and the held hat clip at the edge. If that looks wrong, raise the cat's and toad's minimum N in `Stage::fits` and add the value to `small_panes_get_only_the_bats`.
- [ ] **Step 5: Run** `cargo fmt --check && cargo clippy --all-targets --locked && cargo test --locked` — expect no warnings and PASS, snapshots unchanged.
- [ ] **Step 6: Commit** `feat: tune the familiars' art and pace`
