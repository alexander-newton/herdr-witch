# herdr-witch — design

Date: 2026-11-16 · Status: awaiting approval

## Intent

A herdr plugin that gives each herdr tab a cyber-witchy summoning-circle
look. A pixel-art pentagram (the **sigil**) crackles like lightning more
violently the more coding agents are working at once. Beneath it, a small
witch seen from behind raises her arms in a summoning motion while the user
types. Along the bottom of each tab, one line of grimoire text is shown in
heavily cursed (diacritic-stacked) Unicode. The style references are pixel
art, JRPGs, FF7 and Golden Sun. The palette runs from dark purples up
through bright gold, with bright cyan for lightning.

It is for the user's own herdr setup: an aesthetic delight that also gives
an ambient read of how busy the agents are.

The reference mockup is v6, at
`docs/superpowers/specs/2026-11-16-herdr-witch-mockup.html`. It shows
the approved look, motion and proportions at real terminal resolution, and
its JS is a working reference for the sigil, witch and grimoire rendering.

## Goals

1. **Two panes are added automatically to every new tab** (this includes
   the first tab of a new workspace):
   - **Grimoire pane:** spans the full width of the tab at the bottom.
     Its content area is 1 row (the user asked for "1.5 lines" in the
     mockup, which in a real terminal means a 1-row pane plus herdr's
     border). It shows **one** line of text, **centred** horizontally.
   - **Sigil pane:** on the right, above the grimoire pane, **15% of the
     tab's width**. The sigil sits above the witch, and the pair is
     centred vertically in the pane.
   - Resulting layout: `main | sigil` on top, `grimoire` full width
     underneath. The grimoire therefore has to be split off the root pane
     first (down), and then the top pane split right for the sigil.
2. **The sigil shows the same live state in every tab.** Every sigil pane
   shows one global state: the count of agents in `working` across the
   whole herdr server, plus a "user is typing" flag.
3. **Sigil intensity rises with the working-agent count**, as in the
   mockup:
   - 0 working: dim violet embers, slow breathing.
   - 1–3 working: gold lines, light flicker, occasional lightning.
   - 4 or more: runes start flashing cyan, more bolts and sparks.
   - 5 or more: bolts escape the circle.
   - Random full-sigil "strike" frames become more frequent with the count.
   - Roughly 12 fps.
4. **The witch summons while the user types.** Her arms ease up (over
   about 0.4 s) and sway. Sparks come from her hands, lightning runs from
   her up into the circle, and the sigil gets a brightness boost. When
   typing stops, her arms ease down over about 0.7 s.
5. **The grimoire line stays fixed for the life of the tab.** Each tab gets
   a different line, with no repeats until the pool is used up.
   - The marks are applied at curse level 4.
   - The marks re-randomise a few words at a time to give a crawling,
     cursed shimmer. The underlying words never change.
   - It "glows" by cycling colour word by word: gold → bright gold → cyan
     flare, with violet dips. A terminal cannot blur, so no glow effect is
     attempted.
6. **Either pane can be closed on any single tab.** Once closed, the plugin
   never reopens it for that tab.
7. **The sigil works at any pane size.** It renders correctly for whatever
   column count the pane has, and redraws on resize. See *Resolution
   tiers*.
8. **herdr's own theme is left alone.** No `[theme.custom]` changes. Only
   the plugin's panes carry the palette.

## Non-goals (and how existing tabs are handled)

Tabs that already exist when the plugin is first installed get no panes
automatically; "Summon here" adds them. Tabs that come back after a
herdr restart are different: they *are* redecorated (see *Restarts and
persistence*).


- Drawing in herdr's own interface (the sidebar or tab bar). The plugin
  API doesn't allow it.
- Adding panes automatically to tabs that existed before the plugin was
  first installed. A manual action covers those (see *Actions*).
- Real keystroke capture. herdr exposes no input events (see *Typing
  detection*).
- Sound.

## Anti-goals (failure even if it technically works)

- **Recolouring herdr's own interface.**
- **Stealing focus or keystrokes.** Opening panes must never move focus
  away from the pane the user is working in, and must never change the
  user's own layout beyond adding the two splits.
- **Being a resource hog.** The total cost must be negligible, even with
  ten or more tabs open.
- **Reopening a pane the user closed.**
- **Visible tearing or flicker in the terminal** (as opposed to the
  intended lightning flicker). Write each frame with synchronized output,
  in a single write per frame.

## Constraints and facts (herdr 0.9.5)

- **Plugin manifest:** `herdr-plugin.toml` at the plugin root, installed
  via `herdr plugin link <path>`. Its sections are `build[]`, `startup[]`,
  `events[] {on, command}`, `panes[] {id, title, command, placement:
  overlay|popup|split|tab|zoomed, width, height}`, `actions[] {id, title,
  command, contexts}` and `link_handlers[]`.
- **Socket API** (checked against the schema by the reviewer; `herdr api
  schema --json` is the authority):
  - `events.subscribe` takes dotted names such as `tab.created`. Payloads
    use underscored names such as `tab_created`. The subscribable types
    are `tab.*`, `pane.created/closed/updated/focused/moved/exited/agent_detected`
    and `layout.updated`.
  - **There is no output-change subscription.** `pane_output_changed`
    exists only as a one-shot `events.wait` for a single pane, using
    `min_revision`.
  - **There is no server-wide agent-status subscription.**
    `pane.agent_status_changed` needs a `pane_id`. The global working
    count therefore needs either a subscription per pane (on
    `pane.created`, plus the initial `pane.list`) or polling `agent.list`.
  - `tab_created` carries `TabInfo` with no pane IDs. Find the root pane
    with `tab.get` or `pane.list`.
  - Layout is ratio-only: `pane.split --ratio`,
    `layout.set_split_ratio` and `pane.resize` all take floats.
    `plugin.pane.open` takes `width`/`height` as `PopupSize` and has no
    ratio, so splitting may need `pane.split` + run, or
    `plugin.pane.open` followed by `layout.set_split_ratio`.
  - `plugin.pane.open` defaults to `focus: false`.
- **Primary terminal:** a VTE-based terminal, with herdr's own terminal
  emulator sitting in between. Truecolor is available.
- **Toolchain:** Rust (stable, 1.100) via cargo. herdr itself is Rust.
- **Distribution:** a **public** GitHub repo at
  `alexander-newton/herdr-witch`. Install with `herdr plugin install
  alexander-newton/herdr-witch`; the manifest's `build` step runs
  `cargo build --release`.
- **Privacy (hard rule):** nothing private goes into the repo or its
  history. That means:
  - no personal paths, hostnames or machine names;
  - no email addresses (commits use the GitHub noreply address, which is
    already set in the repo's git config);
  - no references to the user's other projects or sync setup;
  - no logs, session files or herdr state.

  Runtime state lives outside the repo, in the plugin's config dir and
  `$XDG_STATE_HOME`. Before every push, scan the diff for anything
  private.

### Restarts and persistence (decided with the user)

After a herdr server restart, or a plugin or daemon restart, the daemon
**re-adds the panes to every tab that should have them**:

- **Re-adopt first.** On startup, list the panes in each tab and
  recognise any existing sigil or grimoire panes (by plugin pane info or
  process). Don't duplicate them.
- **Add what's missing,** except where the user closed that pane in that
  tab.
- **Persist closures on disk** in `$XDG_STATE_HOME/herdr-witch/`. A
  closure is a pane kind plus a stable tab identity.
- **Persist the grimoire line** with each tab's record as well, so a
  restored tab keeps its incantation.
- **Tab identity is a builder task.** First check whether tab IDs survive
  a herdr server restore. If they don't, key records by (workspace label
  or cwd, tab label, tab index) and accept the occasional mismatch.
- **Sigil panes survive a daemon restart:** they keep animating at idle
  intensity until the daemon is reachable again, then reconnect.

## Approaches considered

1. **Rust (chosen).** A single binary with subcommands. Each process uses
   about 2 MB of memory and steady CPU at 12 fps. It matches herdr's own
   stack, cargo is already present, and the manifest `build` step compiles
   it on install. Tweaking the art would mean recompiling, which is
   avoided by loading sprites and runes from plain data files at runtime.
2. **Python, stdlib only.** No build step and easy to tweak. But at 2
   processes per tab and roughly 10–15 MB each, ten tabs would cost about
   200–300 MB, and frame timing is less steady. Rejected under the
   resource anti-goal.
3. **Node.** Python's memory cost with no offsetting benefit. Rejected.

The user picked Rust.

## Architecture

The plugin is one binary, `herdr-witch`, with these subcommands:

- **`daemon`**, launched from the manifest `startup` hook. A single
  instance per herdr server; it must stay a singleton if started twice.
  - It subscribes to herdr events.
  - On `tab_created`, it lays out the two panes in that tab, with
    `focus: false`, and assigns the tab its grimoire line.
  - It records which panes it created for each tab. When one of them
    closes (`pane_closed`), it marks that pane kind as closed for that tab
    and never reopens it.
  - It maintains the global state (`working_count`, `typing`) and
    publishes it to every sigil pane. A local Unix socket or a
    shared-memory file in `$XDG_RUNTIME_DIR` are both fine; that choice is
    the builder's.
  - It cleans up per-tab records on `tab_closed`.
- **`sigil`**, the program running in a sigil pane. It reads the global
  state, renders the sigil and the witch for the current pane size, and
  handles resizing.
- **`grimoire --tab <id>`**, the program running in a grimoire pane. It
  receives its assigned line (via an env var or from the daemon), then
  renders it centred and cursed, with the colour shimmer.
- **`test`**, a diagnostic. It prints octant and half-block samples, plus
  a curse-level-4 line, so the user can eyeball what their terminal and
  herdr actually render.

### Typing detection

herdr exposes no keystroke events, so typing is inferred. "Typing" is true
while the **focused** pane's output changes and its agent status is
**not** `working`. The event used to see this is under *Detecting typing*
below. That covers keystrokes echoed into an idle agent
prompt or a shell. Typing ends about 900 ms after the last such change.
Output from the plugin's own panes must be excluded, because the sigil
animates constantly. It's accepted that this can misfire on non-agent panes
that update themselves, such as `htop`. If `pane_output_changed` turns out
to be too chatty or too coarse, the builder may pick another signal of the
same idea, but must say so.

### Resolution tiers (sigil)

Let `N` be the sigil's pixel width.

- **Half-blocks (`▀`, 1×2 pixels per cell):** `N` equals the pane's inner
  columns. Pixels are square, and the two colours a cell can hold map
  exactly onto its two pixels.
- **Octants (U+1CD00 block, 2×4 pixels per cell):** `N` is twice the inner
  columns. Each cell can only show 2 colours, so every 2×4 block is
  reduced to its darkest and brightest colour (as the mockup's `quant()`
  does).
- **Selection:** octant is the default, with half-block as the fallback,
  chosen by config (`render = "octant" | "halfblock"`). No automatic
  detection.

Geometry is rebuilt for each `N`, following the mockup's `build(N)`:

- **Outer ring** at about 0.466·N. **Inner ring** at about 0.352·N, or
  0.33·N when N < 30. The inner ring is dropped if the gap between rings
  is under 1.6 px.
- **Star** vertices on the inner ring.
- **Runes:** 3×3 glyphs when the gap between rings is at least 4.5 px,
  single dots when it's at least 2.5 px, otherwise none.
- **Centre eye:** a 5-pixel cross when N ≥ 30, otherwise 1 pixel.
- **Witch:** the full 17×22 sprite when N ≥ 34, otherwise the 9×11 mini
  sprite. Both are in the mockup.

The column counts that must each look right, at both tiers: 8 to 60 inner
columns. That covers 15% of tabs from about 80 to 400 columns, and it's
what "build the cases for different numbers of columns" means. Between the
fixed thresholds, scale proportionally as the mockup does.

The vertical layout is the sigil (N px tall) followed by the witch canvas
(24 or 13 px tall), centred as a block in the pane's rows. If the pane is
too short for both, draw the sigil alone. If it's too short even for the
sigil, shrink N to fit the height.

### Grimoire text

- **Source:** verbatim public-domain passages, starting with S. L. Mathers'
  *The Goetia* (1904) and *The Key of Solomon the King* (1889). Bundle them
  as a plain-text pool file, one line per entry. The mockup's lines are
  only stand-ins.
- **Length:** each entry is a short phrase of about 30–70 characters.
  Pick a line that fits the pane's width; if none fits, truncate at a word
  boundary.
- **Assignment:** the daemon works through a shuffled pool. No line
  repeats until the whole pool has been used, and two live tabs never
  share a line. A tab keeps its line for its whole life.
- **Cursing:** combining marks above, through and below, at level 4, as
  in the mockup's `curse()`. Marks go on letters only. Display width is
  computed from the base characters, so centring is correct.

### Palette (from the mockup)

| Name | Hex |
|---|---|
| bg | `#0d0614` |
| deep | `#2a0f45` |
| halo | `#3a1460` |
| violet | `#6b2fa3` |
| orchid | `#aa5adc` |
| gold | `#f5c542` |
| bright | `#ffe896` |
| cyan | `#4ff0ff` |
| white | `#ebfaff` |

The sigil pane paints its own `bg` backdrop. The grimoire pane uses the
terminal's default background, and only its text is coloured.

### Actions (manifest `actions`)

- **"Summon here":** adds whichever of the two panes is missing to the
  current tab. Context: tab. This also covers tabs that predate the plugin
  and tabs where a pane was closed.
- **"Test rendering":** opens `herdr-witch test` in a popup.

### Config

`herdr plugin config-dir herdr-witch` holds a TOML file with:

- `render` (`octant` | `halfblock`)
- `sigil_width_pct` (default 15)
- `fps` (default 12)
- `curse_level` (default 4)
- `auto_open` (default true)

## Decided with the user

- The pentagram lives in its own pane, not in herdr's interface.
- Layout: the grimoire runs full width at the bottom, and the sigil is on
  the right at 15% width. The grimoire is one row of text, centred.
- Same sigil state in all tabs.
- Witch: seen from behind, under the sigil, arms raised while typing.
- Grimoire: curse level 4, one line per tab, fixed for the tab's life.
- herdr's own colours are unchanged.
- Rust.
- Octant rendering with a half-block fallback, so it handles any column
  count.
- After a restart, restored tabs get their panes back, except where the
  user closed one.
- A public GitHub repo, with nothing private in it.

### Sizing (my call)

Sizes are set from the tab's dimensions when the panes open: a 15% ratio
for the sigil, and a ratio for the grimoire worked out so it comes to
exactly 1 content row plus its border. The daemon re-applies the
grimoire's 1-row ratio only when the tab's total size changes, i.e. a
window resize, detected via `layout.updated` by comparing the tab's
outer size. If a `layout.updated` leaves the tab's outer size unchanged,
treat it as the user dragging the split, and from then on leave that
tab's sizing alone. Never fight a deliberate drag.

### Which new tabs (my call)

Every `tab.created` gets the panes, with one exception: a tab created by
moving a pane (`pane_moved` carrying `created_tab`). If a witch pane is
moved into another tab, that tab counts as having that pane.

### Detecting typing (revised after the API check)

Use `pane.updated` on the focused pane. When its `revision` advances
while its agent is not `working`, count that as typing. If `pane.updated`
turns out not to fire on output changes, fall back to re-arming an
`events.wait` for `pane_output_changed` on the focused pane. Either way,
the plugin's own panes are excluded.

## Calls I made (user can override)

- Workspaces' first tabs count as new tabs.
- Tabs that existed before the plugin was first installed get no panes
  automatically; the "Summon here" action adds them.
- Closed panes stay closed for that tab.
- The typing heuristic described above.
- The grimoire source books.
- Octant is the default, chosen by config rather than auto-detected.

## Left to the builder

- Module layout.
- Which terminal-output crate to use (e.g. raw ANSI vs `crossterm`).
- The IPC mechanism between the daemon and the panes.
- How to run the singleton daemon.
- Exact easing curves.
- Bolt and spark tuning within the mockup's character.
- Test strategy. Snapshot-testing the sigil geometry per N is strongly
  suggested.
