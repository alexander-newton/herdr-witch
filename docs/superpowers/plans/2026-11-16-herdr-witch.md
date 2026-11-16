# herdr-witch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A herdr plugin, written in Rust, that adds two panes to every new
tab:
- a live, lightning-crackling pixel-art sigil pane with a summoning witch;
- a single cursed grimoire line, centred and fixed for the tab's life.

**Architecture:** There is one binary, `herdr-witch`, with these subcommands:
- `daemon`: a singleton started by the manifest's `startup` hook. It
  watches herdr over its socket, decorates tabs and persists per-tab
  records. It publishes the global `{working, typing}` state to a small
  runtime file.
- `sigil` and `grimoire`: the programs running inside the plugin panes.
  They render with truecolor ANSI at about 12 fps.
- `summon` and `test`: the two actions.

All rendering is pure functions over an RGB pixel buffer, so it can be
snapshot-tested. All daemon decisions are pure functions over a
`HerdrApi` trait, so they can be tested against a fake.

**Tech Stack:**
- Rust 2024 edition, with `rust-version = "1.85"`.
- Runtime crates: `serde`, `serde_json`, `toml`, `fastrand`, `rustix`
  (features `termios`, `fs`) and `unicode-width`.
- Dev crate: `insta`.

**Spec:** `docs/superpowers/specs/2026-11-16-herdr-witch-design.md`, with
the reference mockup at
`docs/superpowers/specs/2026-11-16-herdr-witch-mockup.html`. The mockup's
JS (`build`, `drawPent`, `drawWitch`, `quant`, `curse`, `tickGrim`) is the
behavioural reference for every visual constant not restated here.

## Global Constraints

- **Privacy (hard rule).** No personal paths, hostnames, machine names,
  emails, references to the user's other projects or sync tooling, logs,
  session files or herdr state, anywhere in the repo or its history.
  Commits use the repo-local noreply identity that is already configured.
  Before any push, both of these must print nothing:
  `git log -p | grep -nE -f .superpowers/private-patterns.txt` (an untracked, git-ignored pattern file)
  and
  `git log -p | grep -oE '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[a-z]+' | sort -u | grep -vE 'users\.noreply\.github\.com|noreply@anthropic\.com'`.
- **Leave herdr's theme alone.** Never write `[theme.custom]` or touch
  herdr's `config.toml`.
- **Never steal focus.** Every pane open or split passes
  `focus: false`.
- **Never move or resize the user's panes,** except for the two splits
  this plugin creates and the grimoire's ratio on window resize.
- **Never reopen a pane the user closed** in that tab, and that holds
  across restarts.
- **Palette** (spec): bg `#0d0614`, deep `#2a0f45`, halo `#3a1460`, violet
  `#6b2fa3`, orchid `#aa5adc`, gold `#f5c542`, bright `#ffe896`, cyan
  `#4ff0ff`, white `#ebfaff`.
- **Default config:** `render = "octant"`, `sigil_width_pct = 15`,
  `fps = 12`, `curse_level = 4`, `auto_open = true`.
- **Frame output:** each frame is a single `write` wrapped in
  synchronized output (`\x1b[?2026h` … `\x1b[?2026l`). The cursor is
  hidden, autowrap is off (`\x1b[?7l`) while running, and both are
  restored on exit.
- **Per-session isolation:** state files and the daemon lock are keyed by
  a hash of `HERDR_SOCKET_PATH`, so two herdr sessions never share sigil
  state or block each other's daemon.
- **Paths:**
  - Persistent state: `$HERDR_PLUGIN_STATE_DIR`, falling back to
    `$XDG_STATE_HOME/herdr-witch`, then `~/.local/state/herdr-witch`.
  - Runtime file: `$XDG_RUNTIME_DIR/herdr-witch/`, falling back to the
    state dir.
  - Config: `$HERDR_PLUGIN_CONFIG_DIR/config.toml`.

## Review Focus

These failure modes are implied by the spec but not exercised by the
feature tests. Each has a pinned test in the task named.

1. **Degenerate pane sizes.** herdr can hand a pane 0–2 columns or rows
   mid-resize. The sigil and grimoire must draw nothing and never panic.
   Tests: Task 3 `fit_degenerate_sizes`, Task 7 `render_line_tiny_widths`.
2. **Duplicate or replayed events.** Scripts create tabs fast, and a
   reconcile pass can race `tab.created`. Each tab must end up with
   exactly one sigil and one grimoire. Test: Task 9
   `tab_created_twice_decorates_once`.
3. **herdr socket disappears** (server stop or restart). The daemon
   reconnects with backoff and doesn't exit or spin. Sigil panes fall
   back to idle when state is stale. Tests: Task 5
   `stale_state_reads_idle`, Task 10 `client_reconnect_backoff`.
4. **Two herdr sessions at once.** Each gets its own daemon and its own
   sigil state. Test: Task 5 `state_paths_differ_per_socket`.
5. **A grimoire line wider than the pane, or combining marks miscounted
   as width.** A line must never wrap onto a second row. Tests: Task 7
   `fits_and_truncates_at_word`, `width_ignores_combining`.

---

## File Structure

```
Cargo.toml
herdr-plugin.toml            manifest (Task 10)
assets/
  grimoire.txt               verbatim public-domain lines, one per line (Task 7)
  witch_big.txt              17×22 sprite, mockup BODY (Task 3)
  witch_mini.txt             9×11 sprite, mockup MINI (Task 3)
  runes.txt                  ten 3×3 rune glyphs, mockup RUNES (Task 3)
scripts/gen_octants.py       generates src/render/octant_table.rs from UnicodeData.txt (Task 2)
src/
  main.rs                    subcommand dispatch
  config.rs                  Config + load()
  paths.rs                   state/runtime/config paths, socket hash
  palette.rs                 Rgb + palette constants
  render/
    mod.rs
    raster.rs                Buf, line_pts, quant_octant
    encode.rs                encode_halfblock, encode_octant
    octant_table.rs          generated: [char; 256]
    frame.rs                 FrameWriter (sync output, cursor/autowrap)
  sigil/
    mod.rs                   run() loop
    geometry.rs              SigilGeometry::build(n)
    sprites.rs               Sprite, load sprites/runes
    fit.rs                   fit(cols, rows, mode) -> Fit
    anim.rs                  SigilAnim (intensity, bolts, sparks, arms)
  grimoire/
    mod.rs                   run() loop
    curse.rs                 curse(), display_width()
    pool.rs                  LinePool
    view.rs                  render_line()
  ipc.rs                     GlobalState read/write
  herdr/
    mod.rs
    types.rs                 serde types: PaneInfo, TabInfo, AgentInfo, layout snapshot
    api.rs                   trait HerdrApi
    client.rs                SocketClient (impl HerdrApi), subscribe stream
    fake.rs                  FakeHerdr (cfg(test) + tests/ via feature "fake")
  daemon/
    mod.rs                   run(): event loop, singleton lock
    store.rs                 Store (persisted tab records)
    decorate.rs              decorate_tab(), reconcile()
    sizing.rs                grimoire_ratio(), sigil_ratio(), DragTracker
    typing.rs                TypingDetector
  test_cmd.rs                `herdr-witch test` output
tests/
  geometry_snapshots.rs
  frame_snapshots.rs
README.md, LICENSE (MIT)
```

---

### Task 1: Crate scaffold, palette, raster buffer

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/palette.rs`,
  `src/render/mod.rs`, `src/render/raster.rs`, and an addition to
  `.gitignore` (`target/`).

**Interfaces:**
- Produces:
  - `palette::Rgb = [f32; 3]`, with the constants `BG, DEEP, HALO,
    VIOLET, ORCHID, GOLD, BRIGHT, CYAN, WHITE` (spec values) and
    `SLEEVE=[59,22,98]` and `SKIN=[232,198,168]`.
  - `fn lerp_c(a: Rgb, b: Rgb, t: f32) -> Rgb` and
    `fn mul(c: Rgb, k: f32) -> Rgb`.
  - `raster::Buf { w: usize, h: usize, d: Vec<Rgb> }` with the methods
    `new(w,h)`, `clear(c)`, `set(x: i32, y: i32, c)`,
    `add(x: i32, y: i32, c, a: f32)`, `get(x,y) -> Rgb` and
    `to_u8(x,y) -> [u8;3]` (clamped). Out-of-bounds coordinates are
    silently ignored.
  - `fn line_pts(x0,y0,x1,y1: i32) -> Vec<(i32,i32)>` (Bresenham).
  - `fn quant_octant(buf: &mut Buf)`, the mockup's `quant()` behaviour:
    each 2×4 block is reduced to its darkest and brightest colour by
    luma 0.3/0.59/0.11, and each pixel is assigned to the nearer of the
    two. Partial edge blocks are allowed.
- `main.rs` dispatches `daemon|sigil|grimoire|summon|test|--version`;
  stubs print "unimplemented" and exit 2.

- [ ] **Step 1:** `cargo init --name herdr-witch`. Set the edition to 2024,
  `rust-version = "1.85"`, `license = "MIT"`, no `authors` field, and add
  the dependencies listed in Tech Stack.
- [ ] **Step 2: Write failing tests** in `raster.rs`:
  - `line_pts_endpoints_inclusive`: `line_pts(0,0,3,1)` starts at
    `(0,0)`, ends at `(3,1)` and has length 4.
  - `add_and_clamp`: adding `[200,0,0]` twice to one pixel gives
    `to_u8 = [255,0,0]`.
  - `oob_ignored`: `set(-1,0,..)` and `set(w,0,..)` don't panic.
  - `quant_two_colours_per_block`: fill a 4×8 buffer with 8 distinct
    colours per block, run `quant_octant`, and assert each 2×4 block has
    ≤ 2 distinct colours, which are the block's darkest and brightest
    originals.
  - `quant_partial_block`: on a 3×5 buffer it runs without panic.
- [ ] **Step 3:** Run `cargo test raster`. Expected: FAIL (the functions
  are missing).
- [ ] **Step 4:** Implement the palette and raster per the Interfaces.
- [ ] **Step 5:** Run `cargo test raster`. Expected: PASS. Also run
  `cargo clippy -- -D warnings`, which should be clean.
- [ ] **Step 6:** Commit with the message
  `feat: crate scaffold, palette and raster buffer`.

### Task 2: Terminal encoders and frame writer

**Files:**
- Create: `scripts/gen_octants.py`, `src/render/octant_table.rs`
  (generated and committed), `src/render/encode.rs`,
  `src/render/frame.rs`.

**Interfaces:**
- Consumes: `Buf` (Task 1).
- Produces:
  - `pub static OCTANT: [char; 256]`. Bit `i` (0..8) of the index is the
    sub-cell numbered `i+1` in Unicode's octant numbering: rows top to
    bottom, left then right, so cells 1 2 / 3 4 / 5 6 / 7 8. Index 0 is
    `' '` and index 255 is `'█'`.
  - `fn encode_halfblock(buf: &Buf, ox: usize, oy: usize, cols: usize, rows: usize) -> String`.
    Each cell is `▀`, with fg = pixel `(x, 2y)` and bg = pixel
    `(x, 2y+1)`, using truecolor SGR `38;2` / `48;2`. An SGR is emitted
    only when the colour changes. Each row starts with a CUP
    (`\x1b[{row};1H`).
  - `fn encode_octant(buf: &Buf, ...) -> String` (same parameters).
    Each 2×4 block takes its brightest colour as fg and its darkest as
    bg. The pattern bits are the pixels equal to fg, and the character
    is `OCTANT[bits]`. A uniform block emits a space with bg only.
  - `FrameWriter { out: Stdout }` with these methods:
    - `begin()`: hide cursor, `?7l`, clear screen.
    - `frame(&mut self, body: &str)`: writes
      `\x1b[?2026h\x1b[H` + body + `\x1b[0m\x1b[?2026l` as one
      `write_all` + `flush`.
    - `Drop`: restores cursor and `?7h`, then `\x1b[0m`.
- `gen_octants.py`:
  - Downloads nothing itself; it takes a path to Unicode 16+
    `UnicodeData.txt` as its argument.
  - Maps every `BLOCK OCTANT-<digits>` name to its bit pattern.
  - Fills the remaining patterns from the pre-existing characters that
    the octant block deliberately omits. These are space, `█`, `▀ ▄ ▌ ▐`,
    the quadrants `▖▗▘▝▚▞▙▛▜▟`, `▂`/`▆` (lower ¼/¾) and the
    upper-¼/¾ blocks in Symbols for Legacy Computing. Find each by name
    in the same file.
  - Exits non-zero if any of the 256 patterns is still unmapped.
  - Writes `octant_table.rs`.

- [ ] **Step 1: Write failing tests** in `encode.rs`:
  - `octant_table_complete`: all 256 entries are distinct chars, with
    `OCTANT[0]==' '` and `OCTANT[255]=='█'`.
  - `octant_known_chars`: `OCTANT[0b0000_0101]=='▘'` (cells 1 and 3,
    the upper-left quadrant), `OCTANT[0b0101_0101]=='▌'` and
    `OCTANT[0b0000_1111]=='▀'`.
  - `halfblock_one_cell`: a 1×2 buffer with top red and bottom blue
    encodes to exactly
    `"\x1b[1;1H\x1b[38;2;255;0;0m\x1b[48;2;0;0;255m▀"`.
  - `halfblock_sgr_dedup`: a 3×2 uniform buffer emits exactly one fg SGR
    and one bg SGR.
  - `octant_uniform_block_is_space`.
- [ ] **Step 2:** Run `cargo test encode`. Expected: FAIL.
- [ ] **Step 3:** Fetch `https://www.unicode.org/Public/16.0.0/ucd/UnicodeData.txt`
  into the scratch area, outside the repo and uncommitted. Run
  `python3 -I scripts/gen_octants.py <path> > src/render/octant_table.rs`.
  Expected: exit 0.
- [ ] **Step 4:** Implement `encode_halfblock`, `encode_octant` and
  `FrameWriter`.
- [ ] **Step 5:** Run `cargo test encode`. Expected: PASS.
- [ ] **Step 6:** Commit with the message
  `feat: half-block/octant encoders and synchronized frame writer`.

### Task 3: Sigil geometry, sprites, and fitting for every column count

**Files:**
- Create: `assets/witch_big.txt`, `assets/witch_mini.txt`,
  `assets/runes.txt`, `src/sigil/mod.rs` (just `pub mod`s for now),
  `src/sigil/geometry.rs`, `src/sigil/sprites.rs`, `src/sigil/fit.rs`,
  `tests/geometry_snapshots.rs`.

**Interfaces:**
- Consumes: `line_pts` (Task 1).
- Produces:
  - `type Glyph = Vec<(i32,i32)>` (offsets of lit pixels in a 3×3 rune),
    `enum WitchSize { Big, Mini }`, and
    `struct Assets { big: Sprite, mini: Sprite, runes: Vec<Glyph> }`
    (in `sprites.rs`).
  - `struct SigilGeometry { n: usize, cx: f32, cy: f32, verts: [(f32,f32);5], core: Vec<(i32,i32,Kind)>, halo: Vec<(i32,i32)>, runes: Vec<Vec<(i32,i32)>>, eye: Vec<(i32,i32)> }`,
    with `enum Kind { Ring, Star }` and
    `fn build(n: usize, runes: &[Glyph]) -> SigilGeometry`.
  - `build` follows the spec's *Resolution tiers* and the mockup's
    `build(N)`:
    - `R1 = 0.466n`; `R2 = 0.352n` when n ≥ 30, otherwise `0.33n`.
    - The inner ring is drawn only when `R1-R2 ≥ 1.6`.
    - Runes are 3×3 glyphs when `R1-R2 ≥ 4.5`, single dots when it is
      ≥ 2.5, otherwise none.
    - The eye is a 5-pixel cross when n ≥ 30, otherwise 1 pixel.
    - Halo = 4-neighbours of core pixels that aren't core, clipped to
      bounds.
  - `struct Sprite { w: usize, h: usize, px: Vec<Option<Rgb>>, shoulders: [(i32,i32);2], arm_len: f32, arm_thick: u8 }`.
    Big: shoulders `(3,11),(13,11)`, len 8, thickness 2, and a canvas
    height of 24. Mini: `(1,6),(7,6)`, len 4, thickness 1, canvas height
    13.
  - `fn load_assets(root: Option<&Path>) -> Assets`.
    It reads `assets/*.txt` under `HERDR_PLUGIN_ROOT` when present,
    otherwise falls back to `include_str!` copies. The char→colour map
    is the mockup's `SC`.
  - `enum Mode { HalfBlock, Octant }` and
    `struct Fit { n: usize, px_w: usize, px_h: usize, sigil_xy: (usize,usize), witch: Option<(WitchSize,(usize,usize))> }`,
    where `px_w/px_h` is the whole pane canvas in pixels: `cols×2rows`
    for half-blocks, `2cols×4rows` for octants.
  - `fn fit(cols: usize, rows: usize, mode: Mode) -> Option<Fit>`.
    - `n = px_w`.
    - If `n + witch_h ≤ px_h`: centre sigil + witch vertically as a
      block. Use the big witch when n ≥ 34, the mini one otherwise.
    - Else if `n ≤ px_h`: centre the sigil alone.
    - Else: shrink to `n = px_h` and centre horizontally.
    - Returns `None` when `cols < 4 || rows < 2`.

- [ ] **Step 1:** Transcribe `BODY`, `MINI` and `RUNES` from the mockup
  into the three asset files. Use one row per line, with `.` for
  transparent.
- [ ] **Step 2: Write failing tests:**
  - `geometry.rs`:
    - `inner_ring_dropped_small`: `build(8)` has no pixels at radius
      ≈ R2.
    - `rune_modes`: `build(16)` gives runes of length 0, `build(27)` gives
      10 single-pixel runes, and `build(54)` gives 10 runes of ≥ 3 px.
    - `eye_sizes`: n=29 gives 1 pixel and n=30 gives 5.
    - `all_pixels_in_bounds` for every n in 8..=120.
  - `fit.rs`:
    - `fit_halfblock_27x40`: `fit(27,40,HalfBlock)` gives n=27 with the
      mini witch, and the sigil y-offset is
      `(80 - (27+13))/2 = 20`.
    - `fit_octant_27x40`: n=54 with the big witch.
    - `fit_short_pane_sigil_only`: `fit(27,16,HalfBlock)` has n=27 and
      witch=None.
    - `fit_very_short_shrinks`: `fit(27,10,HalfBlock)` has n=20.
    - `fit_degenerate_sizes`: `fit(0,0,_)`, `fit(3,40,_)` and
      `fit(27,1,_)` all return None (Review Focus 1).
  - `tests/geometry_snapshots.rs`: an `insta` snapshot of an ASCII
    rendering (`#` core, `+` halo, `r` rune, `o` eye, `.` empty) of
    `build(n)` for every n in 8..=60 (half-block widths) and every even
    n in 16..=120 (octant widths).
- [ ] **Step 3:** Run `cargo test`. Expected: FAIL.
- [ ] **Step 4:** Implement `geometry`, `sprites` and `fit`.
- [ ] **Step 5:** Run `cargo insta test --review`. Eyeball n = 8, 17,
  27, 34, 44 and 54 for a recognisable pentagram, then accept. Run
  `cargo test`. Expected: PASS.
- [ ] **Step 6:** Commit with the message
  `feat: sigil geometry, sprites and pane fitting for all widths`.

### Task 4: Sigil and witch animation

**Files:**
- Create: `src/sigil/anim.rs`.

**Interfaces:**
- Consumes: `Buf`, the palette, `SigilGeometry`, `Sprite`, `Fit`,
  `quant_octant`.
- Produces:
  - `struct SigilAnim { rng: fastrand::Rng, t: f32, arms: f32, bolts: Vec<Bolt>, sparks: Vec<Particle>, wsparks: Vec<Particle> }`.
  - `fn new(seed: u64) -> Self`.
  - `fn intensity(working: u32, arms: f32, t: f32) -> f32` (free fn).
  - `fn step(&mut self, working: u32, typing: bool)`, which advances one
    frame: `t += 0.08`, and arms ease toward the target by at most +0.22
    or −0.12 per frame.
  - `fn draw(&mut self, buf: &mut Buf, fit: &Fit, geo: &SigilGeometry, big: &Sprite, mini: &Sprite, mode: Mode)`.
    It clears to BG, draws the sigil at `fit.sigil_xy` and the witch at
    its offset, then runs `quant_octant` when `mode == Octant`.
  - Port `drawPent` and `drawWitch` from the mockup verbatim in
    behaviour. That covers the flicker amount `0.06+0.07n`, strike chance
    `0.018n (+0.03 when arms>0.8)`, crackle `0.006n`, bolt rate
    `0.3n (+0.25 when arms>0.6)`, escaping bolts at n≥5 with chance
    0.35, bolts from the witch to the sigil when arms>0.7 with chance
    0.22, the spark rate, the rim light and the hand sparks. Witch-origin
    bolts start at the bottom edge of the sigil box.
- `intensity` returns:
  - for working=0: `0.12 + 0.05·sin(0.8t)`;
  - otherwise: `min(1, 0.28 + 0.1·working)`;
  - then `min(1.15, that + 0.22·arms)`.

- [ ] **Step 1: Write failing tests:**
  - `intensity_values`: `intensity(0,0,0)=0.12`, `intensity(3,0,_)=0.58`,
    `intensity(8,0,_)=1.0` and `intensity(8,1,_)=1.15`.
  - `arms_ease`: five `step(_, true)` calls from 0 give arms=1.0 (0.22
    per step, clamped), and then three `step(_, false)` calls give 0.64.
  - `idle_has_no_bolts`: with seed 1, 1000 frames at working=0 and
    typing=false never create a bolt.
  - `busy_bolts_escape`: with seed 1, n=44 and working=8, in 1000 frames
    at least one bolt endpoint lies outside radius `0.47n`.
  - `typing_links_witch`: with seed 1, n=44 and typing=true, in 300
    frames at least one bolt starts at `y == sigil bottom`.
  - `octant_mode_quantised`: after `draw` in Octant mode, every 2×4
    block has ≤ 2 colours.
- [ ] **Step 2:** Run `cargo test anim`. Expected: FAIL.
- [ ] **Step 3:** Implement `SigilAnim`.
- [ ] **Step 4:** Run `cargo test anim`. Expected: PASS.
- [ ] **Step 5:** Commit with the message
  `feat: sigil lightning and summoning witch animation`.

### Task 5: Config, paths and the global state file

**Files:**
- Create: `src/config.rs`, `src/paths.rs`, `src/ipc.rs`.

**Interfaces:**
- Produces:
  - `struct Config { render: Mode, sigil_width_pct: u8, fps: u8, curse_level: u8, auto_open: bool }`
    with `Default` set to the Global Constraints values, and
    `fn load() -> Config`. A missing or partial file falls back to the
    defaults per field, and an invalid file logs to stderr and uses the
    defaults.
  - `fn session_key(socket: &str) -> String`: the first 12 hex chars of
    an FNV-1a 64 hash.
  - `fn state_dir() -> PathBuf` and `fn runtime_dir() -> PathBuf`
    (Global Constraints order), both suffixed with `/<session_key>`.
  - `struct GlobalState { working: u32, typing: bool, written_ms: u64 }`.
  - `fn write_state(dir: &Path, s: &GlobalState) -> io::Result<()>`:
    writes JSON to `state.json.tmp`, then renames it to `state.json`.
  - `fn read_state(dir: &Path, now_ms: u64) -> GlobalState`: returns
    `{0,false,_}` if the file is missing, unparsable, or more than 3000 ms
    old.

- [ ] **Step 1: Write failing tests:**
  - `config_defaults`: an empty TOML string parses to the defaults.
  - `config_partial`: `fps = 8` keeps the other defaults.
  - `state_roundtrip`.
  - `stale_state_reads_idle`: a file written with `written_ms = now−3001`
    reads as idle (Review Focus 3).
  - `state_paths_differ_per_socket`: two socket paths give different
    `runtime_dir`s (Review Focus 4).
  - `missing_state_reads_idle`.
- [ ] **Step 2:** Run `cargo test config ipc paths`. Expected: FAIL.
- [ ] **Step 3:** Implement the three modules.
- [ ] **Step 4:** Run `cargo test`. Expected: PASS.
- [ ] **Step 5:** Commit with the message
  `feat: config, per-session paths and global state file`.

### Task 6: The `sigil` pane program

**Files:**
- Modify: `src/sigil/mod.rs` and `src/main.rs`.
- Create: `tests/frame_snapshots.rs`.

**Interfaces:**
- Consumes: Tasks 2–5.
- Produces:
  - `fn render_frame(cols: usize, rows: usize, mode: Mode, anim: &mut SigilAnim, assets: &Assets, cache: &mut GeoCache, st: &GlobalState) -> String`.
    It is pure apart from the anim's rng. `GeoCache` rebuilds
    `SigilGeometry` only when `n` changes. When `fit` is None it returns
    a clear-screen body.
  - `fn run(cfg: &Config) -> !`. Each tick it reads the terminal size
    via `rustix::termios::tcgetwinsize(stdout)`, reads the state,
    renders, and calls `FrameWriter::frame`. It sleeps to hold `cfg.fps`,
    and exits cleanly on SIGHUP or a stdout write error, i.e. when the
    pane closes.

- [ ] **Step 1: Write failing tests** in `tests/frame_snapshots.rs`:
  - `frame_halfblock_29x40`: seed 7, 10 steps at working=2, then insta
    snapshot the `render_frame` output with escapes made visible.
  - `frame_octant_29x40`: same, in octant mode.
  - `frame_tiny_is_blank`: `render_frame(2,1,..)` contains no `▀` and no
    octant chars.
  - `frame_never_exceeds_width`: for cols in 4..=60 and rows in 2..=50,
    no row of the output holds more than `cols` printable cells. Count
    the chars between CUPs, excluding SGRs.
- [ ] **Step 2:** Run `cargo test --test frame_snapshots`. Expected: FAIL.
- [ ] **Step 3:** Implement `render_frame` and `run`, and wire up
  `main.rs sigil`.
- [ ] **Step 4:** Run `cargo insta test --review`, accept, then
  `cargo test`. Expected: PASS.
- [ ] **Step 5: Manual check.** In a plain terminal (not herdr), run
  `cargo run --release -- sigil` and resize the window. The sigil and
  witch should stay centred and change tier with no tearing. Write `{"working":6,"typing":true,"written_ms":<now>}` into the runtime
  state file, and lightning plus raised arms should appear. Check CPU in
  `top`: it should be under 2% of one core.
- [ ] **Step 6:** Commit with the message
  `feat: sigil pane program`.

### Task 7: Grimoire text, cursing and the `grimoire` pane program

**Files:**
- Create: `assets/grimoire.txt`, `src/grimoire/curse.rs`,
  `src/grimoire/pool.rs`, `src/grimoire/view.rs`,
  `src/grimoire/mod.rs`.
- Modify: `src/main.rs`.

**Interfaces:**
- Produces:
  - `fn curse(word: &str, level: u8, rng: &mut Rng) -> String`. This is
    the mockup's `curse()`, with the same UP/DN/MID code-point tables,
    and marks only after ASCII letters.
  - `fn display_width(s: &str) -> usize`: `unicode-width` over the
    string with combining marks contributing 0.
  - `struct LinePool { lines: Vec<String>, order: Vec<usize>, next: usize }`
    with these methods:
    - `fn from_text(text: &str, seed: u64) -> Self`: skips blanks and
      `#` comments, then shuffles.
    - `fn assign(&mut self, live: &HashSet<usize>) -> usize`: the next
      index in shuffled order that isn't in `live`. It reshuffles once
      the pool is used up.
    - `fn fit_line(&self, idx: usize, width: usize) -> String`: if the
      line is too wide, truncate at the last space that fits. If even
      the first word doesn't fit, hard-cut to `width`.
  - `struct GrimView { words: Vec<Word>, rng: Rng }` with these methods:
    - `fn new(line: &str, level: u8, seed: u64) -> Self`.
    - `fn tick(&mut self, working: u32)`: per frame, re-curses a random
      word with p=0.35, flares cyan with p=`0.08+0.02·working` for
      220–520 ms, toggles a violet dip with p=0.15, and goes hot (bright
      gold) with p=0.2 for 400 ms.
    - `fn render_line(&self, cols: usize, rows: usize) -> String`:
      centred horizontally on `rows/2` (0-based), with no wrap. Default
      background, coloured fg only: gold base, bright when hot, cyan on
      flare, `#b07fe0` on a dip.
  - `run(cfg)`: reads the line index from `HERDR_WITCH_LINE` (set by the
    daemon) or else picks one at random, re-fits it on resize, and ticks
    at `cfg.fps`.
- `assets/grimoire.txt`: verbatim phrases of 30–70 characters, taken from
  S. L. Mathers' *The Goetia* (1904) and *The Key of Solomon the King*
  (1889), both public domain. Source them from the Project Gutenberg or
  Wikisource transcriptions, with at least 60 lines. A header comment
  names the sources.

- [ ] **Step 1: Write failing tests:**
  - `curse_preserves_base`: stripping combining marks from `curse(w,4)`
    gives `w` back, for 100 random words.
  - `curse_level_density`: at level 4, the mean number of marks per
    letter over 10k letters is between 3.0 and 5.0, and level 0 returns
    its input unchanged.
  - `width_ignores_combining`: `display_width(curse("Spirit",4)) == 6`
    (Review Focus 5).
  - `pool_no_live_duplicates`: with 5 lines and 4 live, `assign` returns
    the 5th.
  - `pool_no_repeat_until_exhausted`: 5 assigns over 5 lines return a
    permutation.
  - `fits_and_truncates_at_word`: `fit_line` of
    "Behold the Pentacle of Solomon" at width 20 gives
    "Behold the Pentacle" (Review Focus 5).
  - `render_line_centred`: a 10-char line at cols=30 starts at column 11
    (1-based) on the middle row.
  - `render_line_tiny_widths`: cols 0–3 don't panic, and the output's
    display width is ≤ cols (Review Focus 1).
  - `grimoire_txt_lengths`: every line of the asset is 30–70 chars, and
    there are at least 60 lines.
- [ ] **Step 2:** Run `cargo test grimoire`. Expected: FAIL.
- [ ] **Step 3:** Source and write `assets/grimoire.txt`, then implement
  the modules and `run`.
- [ ] **Step 4:** Run `cargo test`. Expected: PASS.
- [ ] **Step 5: Manual check.** `HERDR_WITCH_LINE=3 cargo run --release -- grimoire`
  in a 3-row terminal should show one centred, shimmering cursed line,
  with no wrap at any width.
- [ ] **Step 6:** Commit with the message
  `feat: grimoire line pool, cursing and pane program`.

### Task 8: herdr socket client and fake

**Files:**
- Create: `src/herdr/mod.rs`, `src/herdr/types.rs`, `src/herdr/api.rs`,
  `src/herdr/client.rs`, `src/herdr/fake.rs`.

**Interfaces:**
- Produces:
  - `trait HerdrApi` with these methods:
    - `tabs(&mut self) -> Result<Vec<TabInfo>>`
    - `panes(&mut self, tab: &str) -> Result<Vec<PaneInfo>>`
    - `layout(&mut self, tab: &str) -> Result<LayoutSnapshot>`
    - `agents(&mut self) -> Result<Vec<AgentInfo>>`
    - `open_plugin_pane(&mut self, req: OpenPane) -> Result<String /*pane_id*/>`
    - `set_split_ratio(&mut self, tab: &str, path: &[bool], ratio: f32) -> Result<()>`
    - `focused_pane(&mut self) -> Result<Option<PaneInfo>>`
  - `struct OpenPane { entrypoint: &'static str, target_pane_id: String, direction: Dir, env: Vec<(String,String)> }`,
    always sent with `placement: "split"` and `focus: false`.
  - `SocketClient::connect(path) -> Result<Self>` (implements
    `HerdrApi`).
  - `SocketClient::subscribe(path, kinds: &[&str]) -> Result<impl Iterator<Item = Result<Event>>>`
    opens a separate connection for the event stream.
  - `enum Event { TabCreated(TabInfo), TabClosed{tab_id}, PaneClosed{pane_id, tab_id}, PaneUpdated(PaneInfo), PaneMoved{pane_id, previous_pane_id, created_tab: Option<String>}, LayoutUpdated{tab_id}, Other }`.
  - `struct Backoff`: 250 ms doubling to a 10 s cap, reset on success.
  - `FakeHerdr`: an in-memory tabs/panes/layout tree that records every
    call, built under `#[cfg(any(test, feature = "fake"))]`.
- Types follow `herdr api schema --json` (schema_version 1). Fields not
  needed are ignored via serde defaults, so schema drift doesn't break
  parsing.

- [ ] **Step 1: Probe the wire protocol** (read-only). Against the live
  socket in `HERDR_SOCKET_PATH`, send a `ping` and a `tab.list` in the
  schema's request shape (`{"id":"1","method":"ping","params":{}}`) to
  confirm the framing (newline-delimited JSON is expected). Send an
  `events.subscribe` for `tab.created`, open and close a tab in a
  throwaway session (`herdr --session witchdev`), and record one real
  payload for each event kind used. Put these as fixtures in
  `src/herdr/fixtures/*.json`. Strip cwd, labels and IDs down to
  neutral values first (privacy).
- [ ] **Step 2: Write failing tests:**
  - `parse_fixtures`: every fixture parses to the expected `Event`
    variant.
  - `request_shape`: `open_plugin_pane` serialises
    `placement:"split"` and `focus:false`.
  - `client_reconnect_backoff`: `Backoff` yields 250, 500, 1000 … 10000,
    10000 (Review Focus 3).
  - `fake_records_calls`.
- [ ] **Step 3:** Run `cargo test herdr`. Expected: FAIL.
- [ ] **Step 4:** Implement the module.
- [ ] **Step 5:** Run `cargo test herdr`. Expected: PASS. Then run
  `cargo test -- --ignored live_ping`, an `#[ignore]` test that pings
  `$HERDR_SOCKET_PATH`. Expected: PASS.
- [ ] **Step 6:** Commit with the message
  `feat: herdr socket client, event stream and fake`.

### Task 9: Daemon logic (decorate, persist, size, detect typing)

**Files:**
- Create: `src/daemon/store.rs`, `src/daemon/decorate.rs`,
  `src/daemon/sizing.rs`, `src/daemon/typing.rs`.

**Interfaces:**
- Consumes: `HerdrApi`, `FakeHerdr`, `LinePool`, `Config`.
- Produces:
  - `struct TabKey(String)`.
    - `fn tab_key(t: &TabInfo, ws_label: &str) -> TabKey` uses
      `tab_id` if IDs survive restore (decided in Step 1), otherwise
      `"{ws_label}\u{1f}{tab.label}\u{1f}{tab.number}"`.
  - `struct TabRecord { line: usize, sigil_pane: Option<String>, grim_pane: Option<String>, sigil_closed: bool, grim_closed: bool, user_sized: bool }`.
  - `struct Store { tabs: HashMap<TabKey, TabRecord> }` with
    `load(dir)` and `save(dir)` (atomic JSON `tabs.json`).
  - `fn decorate_tab(api, store, pool, tab: &TabInfo, cfg) -> Result<()>`.
    It is idempotent:
    - Re-adopt existing witch panes first, recognised by
      `PaneInfo.title`/label set by the plugin pane (`✶ sigil` /
      `✶ grimoire`), or by the plugin pane listing if
      `plugin.pane`-opened panes are reported distinctly. Decide which
      in Step 1.
    - Skip kinds that are closed.
    - Open the grimoire first: split the tab's root pane `down`, with
      env `HERDR_WITCH_LINE`.
    - Then the sigil: split the original pane `right`.
    - Then apply the ratios from `sizing`.
  - `fn reconcile(api, store, pool, cfg) -> Result<()>`: on daemon
    start, decorates every tab that has a record, plus every tab created
    since start. Tabs with no record are left alone, because they
    predate install (spec non-goal).
  - `fn on_event(api, store, pool, cfg, ev: &Event, created_by_move: &mut HashSet<String>) -> Result<()>`:
    - TabCreated: decorate when `cfg.auto_open`, unless the tab is in
      `created_by_move`.
    - PaneMoved with `created_tab`: insert into `created_by_move`.
    - PaneClosed for a recorded witch pane: set `*_closed = true` and
      save.
    - TabClosed: remove the record.
    - LayoutUpdated: hand to the `DragTracker`.
  - `fn grimoire_ratio(area_rows: u16) -> f32`: the first-child share
    that leaves the grimoire pane with **1 content row**. Its outer
    height (content + herdr border rows) is measured once in Step 1 and
    stored as `const GRIM_OUTER_ROWS`.
  - `fn sigil_ratio(pct: u8) -> f32 = 1 - pct/100`.
  - `struct DragTracker { last_outer: HashMap<TabKey,(u16,u16)> }` with
    `fn on_layout(&mut self, key, outer: (u16,u16), grim_rows: u16) -> SizingAction`,
    where `enum SizingAction { Reapply, MarkUserSized, Nothing }`:
    - outer size changed and the tab isn't user-sized → `Reapply`;
    - outer unchanged but grim_rows ≠ 1 → `MarkUserSized`;
    - otherwise `Nothing`.
  - `struct TypingDetector { last_rev: HashMap<String,u64>, until_ms: u64 }`
    with these methods:
    - `fn on_pane_updated(&mut self, p: &PaneInfo, now_ms: u64, ours: &HashSet<String>)`:
      if `p.focused`, `p` is not ours, `p.agent_status != Working`, and
      the revision increased, set `until_ms = now+900`.
    - `fn typing(&self, now_ms) -> bool`.

- [ ] **Step 1: Probe** in the throwaway `witchdev` session and record
  the findings as comments in `decorate.rs`:
  - Do tab IDs survive `herdr server stop` followed by a restart with
    restore?
  - Do `plugin.pane.open` panes report a recognisable title or label?
  - What are the grimoire's outer rows for 1 content row
    (`GRIM_OUTER_ROWS`)?
  - Does `layout.set_split_ratio` `path=[]` address the root split, and
    `[false]` its first child?
  - Does creating a workspace emit `tab.created` for its first tab? If
    it doesn't, also subscribe to `workspace.created` and decorate its
    tab.
  - Does `pane.updated` fire when a pane's output changes? If it
    doesn't, implement the spec fallback: re-arm an `events.wait`
    `pane_output_changed` on the focused pane.
- [ ] **Step 2: Write failing tests** against `FakeHerdr`:
  - `auto_open_false_skips`: with `auto_open = false`, TabCreated opens
    nothing.
  - `new_tab_gets_both_panes`: grimoire split down, then sigil split
    right, both with focus false, ratios applied, and a line assigned.
  - `tab_created_twice_decorates_once` (Review Focus 2).
  - `closed_pane_never_reopened`: a closed sigil doesn't come back
    after a TabCreated replay or after `reconcile`.
  - `closure_survives_restart`: save, load into a new Store, reconcile,
    and the closed pane stays closed while the other is re-added.
  - `line_kept_across_restart`.
  - `moved_pane_tab_not_decorated`.
  - `preexisting_tabs_ignored_on_first_install`.
  - `drag_marks_user_sized`.
  - `window_resize_reapplies`.
  - `typing_from_focused_idle_pane`.
  - `typing_ignores_working_agent`.
  - `typing_ignores_own_panes`.
  - `typing_expires_after_900ms`.
- [ ] **Step 3:** Run `cargo test daemon`. Expected: FAIL.
- [ ] **Step 4:** Implement the four modules.
- [ ] **Step 5:** Run `cargo test daemon`. Expected: PASS.
- [ ] **Step 6:** Commit with the message
  `feat: daemon decoration, persistence, sizing and typing logic`.

### Task 10: Daemon runtime, actions, `test` command and manifest

**Files:**
- Create: `src/daemon/mod.rs`, `src/test_cmd.rs`, `herdr-plugin.toml`.
- Modify: `src/main.rs`.

**Interfaces:**
- Consumes: everything above.
- Produces:
  - `daemon::run(cfg) -> !`:
    - Take a `rustix::fs::flock` exclusive non-blocking lock on
      `state_dir()/daemon.lock`. If the lock is held, exit 0.
    - Connect with `Backoff`, then `reconcile`.
    - Subscribe to `tab.created`, `tab.closed`, `pane.closed`,
      `pane.updated`, `pane.moved` and `layout.updated`.
    - Poll `agents()` once a second for the count of
      `agent_status == Working`.
    - Call `write_state` every 250 ms and on every change.
    - If the socket drops, back off, reconnect and reconcile again.
  - `summon`, the action: reads `HERDR_PLUGIN_CONTEXT_JSON.tab_id`,
    clears that tab's `*_closed` flags, and runs `decorate_tab`. For a
    tab with more than one pane, the grimoire must span the full width,
    so wrap the existing tree via `layout.export` + `layout.apply`:
    `split(down, <existing>, grimoire)`.
  - `test`: prints a palette strip, a 24-column half-block sigil and the
    same sigil with octants, then the full 256-entry octant table in
    16 rows, a curse-level-4 line, and one line of instructions: "If the
    octant sigil looks broken, set render = \"halfblock\" in <config
    path>".
- `herdr-plugin.toml`. Field names come from the schema
  (`InstalledPluginInfo`); confirm the top-level layout with
  `herdr plugin link . --disabled` and `herdr plugin list --json`.
  - Top level: `name = "herdr-witch"`, `version`, `description` and
    `min_herdr_version = "0.9.5"`.
  - `[[build]]`: `cargo build --release --locked`.
  - `[[startup]]`: `target/release/herdr-witch daemon`.
  - `[[panes]]`: `sigil` and `grimoire` (placement split), and `test`
    (popup, 80%×80%).
  - `[[actions]]`:
    - `summon`: "Summon here", contexts `["tab"]`.
    - `test-rendering`: "Test rendering", which opens the `test` pane.
  - If command paths don't resolve relative to the plugin root, use a
    `bin/herdr-witch` shim that execs
    `$HERDR_PLUGIN_ROOT/target/release/herdr-witch`.

- [ ] **Step 1: Write failing tests:**
  - `lock_is_exclusive`: a second `try_lock` on the same path fails.
  - `summon_wraps_multi_pane_tab` (`FakeHerdr`): the resulting layout
    root is a `split(down)` whose second child is the grimoire.
  - `summon_clears_closed_flags`.
- [ ] **Step 2:** Run `cargo test`. Expected: FAIL for the new tests.
- [ ] **Step 3:** Implement the daemon runtime, `summon`, `test` and the
  manifest.
- [ ] **Step 4:** Run `cargo test && cargo clippy -- -D warnings`.
  Expected: PASS and clean.
- [ ] **Step 5: End-to-end check.** In the throwaway session, run
  `herdr --session witchdev`, then `herdr plugin link "$PWD"` and
  `herdr plugin list --json`. Expected: no warnings.
  - A new tab gets the grimoire full width with 1 row, and the sigil on
    the right at 15%. Focus stays on the original pane.
  - Close the sigil in one tab; other tabs keep theirs.
  - Start an agent: the sigil intensifies in every tab.
  - Type into an idle agent prompt: the witch raises her arms.
  - `herdr server stop`, then relaunch: tabs come back decorated and the
    closed sigil stays closed.
  - Open a second session at the same time: its sigils are independent.
  - Run the "Test rendering" action and look at the octants in GNOME
    Terminal.
  - Check `herdr plugin log list` for errors.
- [ ] **Step 6:** Commit with the message
  `feat: daemon runtime, summon/test actions and plugin manifest`.

### Task 11: README, licence, privacy gate, publish

**Files:**
- Create: `README.md` (what it is, a GIF or screenshot placeholder,
  install, config keys, the "Test rendering" tip, credits for the
  public-domain sources) and `LICENSE` (MIT, holder "Alexander Newton").

- [ ] **Step 1:** Write the README and LICENSE.
- [ ] **Step 2: Privacy gate.** Run both Global Constraints greps, plus `git ls-files | xargs grep -nE -f .superpowers/private-patterns.txt`.
  Expected: no output. Also check
  `git log --format='%ae' | sort -u` prints only the noreply address.
- [ ] **Step 3:** Commit with the message
  `docs: README and licence`.
- [ ] **Step 4: Ask the user before publishing.** Once they confirm, run
  `gh repo create alexander-newton/herdr-witch --public --source . --push`.
  Then, in the throwaway session, run
  `herdr plugin install alexander-newton/herdr-witch --yes`. Expected:
  the build succeeds and the plugin is listed.
