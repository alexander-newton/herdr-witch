# herdr-witch

A [herdr](https://herdr.dev) plugin that gives every tab a witch's workbench:
an animated pixel-art summoning sigil that glows brighter as your agents work
(and a little witch who raises her arms while you type), plus a grimoire
line that mutters cursed fragments from old grimoires.

<!-- screenshot: TODO add a GIF or PNG of a decorated tab -->
*(screenshot to come)*

## Requirements

- herdr >= 0.9.5 (Linux)
- Rust >= 1.88, for the build step (`cargo build --release --locked`)
- A truecolor terminal
- For the default octant rendering, a terminal and font with Unicode 16
  "Symbols for Legacy Computing Supplement" octants. Otherwise set
  `render = "halfblock"`.

## Install

```
herdr plugin install alexander-newton/herdr-witch
```

Or from a local checkout (`plugin link` does not build, so build first):

```
cargo build --release --locked
herdr plugin link /path/to/herdr-witch
```

Then start the daemon, either by running the "Summon here" action on any tab
or by restarting the herdr server (the plugin has a startup hook).

## Actions

- **Summon here**: adds the sigil and grimoire to the current tab, even if you
  closed them before. Also starts the daemon if it is not running.
- **Test rendering**: opens a popup showing the sigil in octants and
  half-blocks, the palette, the octant glyph table and a sample cursed line.
  Run it once to check your terminal.

## Configuration

`config.toml` lives in the directory printed by
`herdr plugin config-dir herdr-witch`. All keys are optional; out-of-range
numbers are clamped, and an invalid file falls back to the defaults.

| key                | default     | allowed                       |
|--------------------|-------------|-------------------------------|
| `render`           | `"octant"`  | `"octant"`, `"halfblock"`     |
| `sigil_width_pct`  | `15`        | 5 to 50                       |
| `fps`              | `12`        | 1 to 60                       |
| `curse_level`      | `4`         | 0 to 4                        |
| `auto_open`        | `true`      | `true`, `false`               |

When changes apply:

- `render`, `fps` and `curse_level` are read by each witch pane when it
  opens. Close the pane and run "Summon here" on the tab, or restart the herdr
  server; "Summon here" alone only adds panes that are missing.
- `auto_open` and `sigil_width_pct` are read by the daemon when it starts.
  `herdr plugin disable herdr-witch` stops the daemon within about ten
  seconds; `herdr plugin enable herdr-witch` and then "Summon here" start it
  again with the new values.

## How it behaves

- New tabs are decorated automatically (`auto_open = true`), about half a
  second after they appear: the daemon waits 400 ms in case the tab came from
  moving a pane (those are left alone), then acts on its next 100 ms tick.
  Tabs that existed before the first daemon start are left alone; use
  "Summon here" on them.
- Closing a witch pane is remembered per tab: it stays closed, also across
  restarts, until you run "Summon here" on that tab again. Close it with
  herdr's close action; keys typed into a witch pane (Ctrl-C included) are
  ignored. If a witch pane crashes, the daemon puts it back (at most once per
  ten seconds).
- When the last of your own panes in a tab exits, is closed or is moved away,
  the daemon closes the sigil and grimoire too, so herdr closes the tab as it
  would without the plugin.
- Witch panes in tabs you are not looking at stop drawing (their last frame
  stays) and resume when the tab is focused again.
- After a herdr server restart, restored panes briefly appear as plain shells
  and are then replaced with the witch panes.
- Disabling or unlinking the plugin stops the daemon within about ten seconds.
  Witch panes already open stay until you close them.
- After rebuilding the plugin, running panes keep the old binary until they
  restart.

## Known limitations

- herdr clamps split ratios to 10-90%, so the grimoire is one line tall only on
  tabs of up to about 35 rows. Taller tabs give it a tenth of their height,
  and it grows with the window (a 60-row tab gives it 6 rows, about 4 rows of
  space inside its border); the line sits in the middle.
- Only the tab herdr reports as focused counts as visible. If several clients
  are attached and showing different tabs, witch panes in a tab that only
  another client shows stay paused.
- "Summon here" on a tab with several panes uses corner panes, so the grimoire
  may not span the full width.
- Typing is inferred by polling the focused pane's visible text (see Privacy),
  so it is approximate.

## Privacy

No network access. The plugin reads only herdr's local socket. Typing
detection hashes the visible text of the focused pane in memory to see whether
it changed; the text is never stored or logged.

## Credits

Grimoire fragments and sigil motifs are drawn from S. L. MacGregor Mathers'
*The Goetia* (1904) and *The Key of Solomon the King* (1889), both in the
public domain, using the transcriptions on archive.org.

## Licence

MIT, see [LICENSE](LICENSE).
