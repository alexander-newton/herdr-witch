# Familiars: three rare scenes in the sigil pane — design

Date: 2026-11-16 · Status: approved; built (the look was revised when tuning, see "The look")

## Intent

Now and then something small happens in the sigil pane: a black cat wanders
through, a swarm of bats spirals across the sigil, or the witch takes off
her hat and a large magical toad jumps out. Each happens at random, about
once an hour.

They are for the person who has the plugin open all day. **They must not
distract.** The user's words: "it shouldn't feel very distracting at all -
in fact, hard to notice, and when you watch it for the first time it feels
like a nice surprise." Most of the time a scene will pass unseen; that is
intended.

## Goals

Three scenes, in the sigil pane only. Durations are indicative: where a
duration and "nothing moves fast" disagree, slow wins.

1. **Cat (about 25 s).** A black cat walks in from the right along the
   floor, stops just short of the witch, sits, looks up at the sigil, then
   turns to face the viewer. It blinks; its eyes turn red and return to
   normal; it blinks again. Then it stands and walks on, passing behind the
   witch, and out at the left.
2. **Bats (about 17 s).** A swarm of bats comes in from the top left,
   spirals in front of the sigil, and leaves at the right.
3. **Toad (about 10 s).** The witch takes off her hat. A large magical toad
   is sitting on top of her hair. It jumps down to the floor and hops
   slowly out at the left. She puts the hat back on.

Each scene starts at random with an average of once per hour of the pane
being on screen, independently of the others (so about three scenes an
hour in all). Only one scene plays at a time.

## Non-goals

- No config key and no README entry. The scenes are a surprise for anyone
  who installs the plugin.
- Nothing in the daemon, the state file or the grimoire pane changes.
- No versions of the cat and toad for the mini witch or for panes with no
  witch. Those panes get the bats only.
- No scripting language or data format for scenes.

## Anti-goals (failure even if it technically works)

- **Anything that pulls the eye:** flashes, fast movement, large bright
  shapes, or a scene changing the sigil's glow or lightning.
- **Changing what the pane shows when no scene is playing.** The existing
  frame snapshots must stay byte-for-byte the same.
- **A measurable idle cost.** With no scene playing, the feature may cost a
  few random numbers per frame and nothing more.

## Constraints and facts

- **Canvas.** The pane is a pixel canvas (octant: 2×4 pixels per character
  cell; half-block: 1×2). The sigil is an N×N square; directly below it is
  the witch's strip, N wide and 24 px tall, whose bottom row is the floor.
  The big witch (17×22) stands in the middle of the strip on that floor.
  With N = 58 (a typical 29-column octant pane) there are about 20 px of
  floor on each side of her.
- **The witch is seen from behind.** The dark V below her hat is her hair
  falling down her back. There is no art for the top of her head: the hat
  covers it.
- **The hat cannot be lifted.** Its tip is on the top row of the strip and
  the sigil's ring is directly above, so the hat comes off sideways.
- **Two colours per cell.** In octant mode every 2×4 block is reduced to
  its darkest and brightest colour, so fine detail inside a cell is lost.
  Sprites are small: the cat and toad are about 12×7 px, roughly 6 cells
  wide and 2 tall.
- **The background is nearly black** (13, 6, 20). A black shape on it is
  invisible without a lit edge.
- **Time.** The existing animation advances a fixed amount per frame, and
  `fps` is configurable (1 to 60). A pane in a hidden tab stops drawing.
- **The sigil process must not exit** in a herdr pane (the daemon treats
  that as a crash and reopens it), and it ignores Ctrl-C.
- **The pane writes only cells that changed** since the last frame. Scenes
  only change pixels, so this needs no attention.

## Approaches

**Who decides when a scene plays.**

- *Each sigil pane rolls its own dice.* Chosen. Nothing outside the pane
  changes. Since only the tab on screen is drawing, and a pane's clock
  stops while it is hidden, the user still sees about one of each scene per
  hour of looking.
- *The daemon decides and tells the panes.* One schedule for the session,
  but it means a new field in the state file, and the daemon cannot know
  whether the pane on screen is big enough for the scene.

**How a scene is described.**

- *One function per scene, poses in text files.* Chosen. Each scene is a
  pure function from seconds elapsed to what is where (positions, which
  pose, eye colour), plus drawing code. The poses are text sprites next to
  the witch's.
- *Scenes written entirely as data.* Editable without rebuilding, but the
  bats' spiral and the toad's hops are curves, not keyframes, and a player
  for such scripts is more code than three functions.

## Design

### Timing and chance

- Scenes run on a clock in seconds that advances by `1 / fps` for each
  frame drawn. It stops while the tab is hidden; a scene interrupted that
  way carries on when the tab is shown again.
- On each frame with no scene playing, each scene the pane can show starts
  with probability `1 / (3600 × fps)`. This uses its own random number
  generator, so the existing animation's random sequence is untouched.
- The toad needs the witch's arms fully down. If its turn comes while she
  is typing, it waits and starts once her arms are down and nothing else is
  playing. Waiting does not block the cat or the bats.
- Familiars are off unless switched on, and only the sigil pane's loop (and
  the preview) switches them on, passing its `fps`. The `test` popup, the
  existing tests and the frame snapshots therefore never see them.

### Which panes show what

- **Cat and toad:** only when the big witch is shown (N ≥ 34 and room for
  her strip).
- **Bats:** any pane whose sigil is big enough to read; the builder picks
  the smallest N.
- **A resize in mid-scene:** if the pane can no longer show the scene
  playing (or the toad that is waiting), the scene is dropped.
- **Narrow strips:** at N = 34 there are only about 9 px of floor on each
  side of the witch. Everything is placed relative to her and to the
  strip's edges and simply clips. If that looks bad in the preview, the
  cat and toad get a higher minimum N.

### The scenes

The cat and the toad live in the witch's strip and are clipped to it. The
bats are drawn on the whole pane, over everything, before the octant
reduction.

**Cat.** It walks at about 4 px per second with a two-pose walk. It stops
a few pixels to the right of the witch's robe and sits. It looks up
(about 2.5 s), turns to face us (about 1 s), blinks (about 0.15 s), its
eyes fade to red and back over about 2 s, it blinks again, waits a moment,
stands and walks on. It is drawn behind the witch for the whole scene; it
only overlaps her on the way out.

**Bats.** Between five and nine, by pane size, each 5×3 px with a two-pose
flap. The swarm follows one path: in from beyond the top left of the sigil
to its upper-left rim, about two turns of an inward spiral around the
centre, then out along the tangent to the right edge. Each bat follows the
path with its own delay (spread over about two and a half seconds), its
own orbit (from about half to a little over the swarm's radius) and a
small wobble, so the swarm fills a band and stays spread out as the spiral
tightens. (The user asked for this after seeing the first version bunch
up.)

**Toad.**

1. The witch's arm on the right of the screen rises to her hat (about
   0.8 s).
2. She takes the hat off sideways and down and holds it beside her (about
   1 s). Under it are the top of her head, in the same hair colour, and the
   toad sitting on it. The toad (11×7 px) is a little wider than her head.
3. The toad waits a beat (about 1.2 s), then leaps in an arc to the floor
   on her left (about 0.9 s) and lands with a few small sparks.
4. She puts the hat back and lowers her arm (about 1.2 s) while the toad
   starts to hop.
5. The toad hops out at the left: each hop about 6 px long and 3 px high,
   half a second in the air, a rest of about 1.2 s between hops, a few
   sparks on each landing.

The toad is drawn in front of the witch. If the user starts typing during
steps 1 to 4, her other arm rises as usual and the hat arm joins it once
the hat is back on.

Two cheats are expected here. Her arm is a rigid 8 px line today, which
cannot both reach the hat and hold it clear of her, so the hat arm may
change length or slide its grip. And the toad is wider than the hat's
cone, so it may be revealed as the hat's brim uncovers it.

### The look

- **Cat:** darker than the background, with a dim violet light along its
  top edges (back, ears, tail) so its outline can just be made out. The
  light is constant; it does not follow the sigil. Its eyes are one pixel
  each, amber, and red in the middle of the scene.
- **Bats:** a dim violet-grey, a little lighter than the background. They
  hide what they pass over, so against the sigil's lines they show as
  moving gaps.
- **Toad:** muted green with a darker underside and dark eyes. Its sparks
  are the witch's existing cyan and gold sparks, dimmer.
- **What octant mode allows** (found when tuning): a cell shows only its
  darkest and brightest colour, so anything brighter than a sprite's body
  takes one of the two and the body around it is drawn as background.
  Hence the toad's eyes are dark, not gold; the cat's bright eyes work
  only because its body is the darkest thing in the pane; and the light
  on the cat's edges is kept very faint.
- Nothing else changes: the sigil, its lightning and the witch's own rim
  light behave exactly as without a scene.

### Art as text

The poses are text files in `assets/`, loaded from the plugin's directory
with a copy built into the binary, as the witch and runes are: the cat
(two walking poses, sitting and looking up, sitting and facing us), the
toad (sitting, leaping), the bat (two poses) and the top of the witch's
head. The hat is the top rows of the existing witch sprite, cut out when
it is loaded. The user can redraw any of them.

### Preview

A hidden command, `herdr-witch scene <cat|bats|toad|all>`, left out of the
usage text. It runs the sigil in the current terminal, starts the named
scene after a second, and exits a second after it ends (`all` plays the
three in turn). It has to end by itself because the sigil ignores Ctrl-C.
It is how the builder tunes the art and timing, and how the user judges
the result without waiting an hour.

- It draws in a pane-shaped area (29 columns unless told otherwise, the
  terminal's height), because in a wide terminal the sigil would fill the
  height and leave no room for the witch.
- It uses a fixed idle state, not the daemon's. Run in an ordinary herdr
  pane, its own animation would otherwise be read as typing, and the toad
  would wait for ever.

### Tests

- Each scene's timeline as a pure function: the cat's eyes go normal,
  shut, red, normal, shut, normal; it stops, then leaves at the left; the
  bats start outside the pane, pass inside the sigil and end beyond the
  right edge; the hat ends where it began and the toad reaches the floor
  and leaves at the left.
- Chance: over many simulated hours each scene starts about once an hour,
  and two never overlap.
- The toad does not start while the witch's arms are up.
- The cat and toad never start without the big witch. Every scene, forced
  to play, draws without panicking at every pane size the existing tests
  cover, in both render modes.
- The existing frame snapshots do not change.

## Calls I made (user can override)

- "Once per hour" is per scene, not for the three together.
- The feature is called "familiars" in the code.
- Pace, durations and colours as above.
- The cat stops short of the witch rather than in front of her.
- The hat comes off sideways, with the arm on the right of the screen.
- The toad waits for the witch's arms to be down; the other two scenes
  play whatever the sigil is doing.
- Small panes get the bats only.
- No README entry, no config key.
- The preview is a hidden command, drawn 29 columns wide with a fixed idle
  state.
- The bats take nearer 10 s than the 6 s first proposed: at 6 s they would
  be by far the fastest thing in the pane.
- A scene the pane can no longer show after a resize is dropped.

## Left to the builder

- The pixel art, and every time, speed and offset above, tuned by eye in
  the preview within the pace and tone described.
- Module layout and how the scenes hook into the existing drawing code.
- Easing curves, the bats' exact path, how many bats, the smallest sigil
  that gets bats.
- How the tests are arranged.
