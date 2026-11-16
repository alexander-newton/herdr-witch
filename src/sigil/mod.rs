pub mod anim;
pub mod familiars;
pub mod fit;
pub mod geometry;
pub mod sprites;

use crate::config::Config;
use crate::ipc::{self, GlobalState};
use crate::render::encode::{encode_halfblock_into, encode_octant_into};
use crate::render::frame::FrameWriter;
use crate::render::pacer::{HIDDEN_POLL, HiddenGate, Pacer};
use crate::render::raster::Buf;
use crate::render::tty::TtyGuard;
use familiars::{Familiars, Scene};
use fit::Mode;
use geometry::SigilGeometry;
use sprites::{Assets, load_assets};
use std::collections::VecDeque;
use std::path::Path;

/// Caches the sigil geometry, rebuilding only when `n` changes.
#[derive(Default)]
pub struct GeoCache {
    geo: Option<SigilGeometry>,
}

impl GeoCache {
    fn get(&mut self, n: usize, assets: &Assets) -> &SigilGeometry {
        if self.geo.as_ref().is_none_or(|g| g.n != n) {
            self.geo = Some(geometry::build(n, &assets.runes));
        }
        self.geo.as_ref().expect("geometry just built")
    }
}

/// Render one frame body (no sync wrapper) and advance the animation one tick.
pub fn render_frame(
    cols: usize,
    rows: usize,
    mode: Mode,
    anim: &mut anim::SigilAnim,
    assets: &Assets,
    cache: &mut GeoCache,
    st: &GlobalState,
) -> String {
    SigilRenderer::default()
        .render(cols, rows, mode, anim, assets, cache, st)
        .to_string()
}

/// `render_frame` with the pixel buffers and the encoded body kept across
/// frames, so a steady pane allocates nothing per frame for them.
#[derive(Default)]
pub struct SigilRenderer {
    buf: Buf,
    /// The pixels of the frame before, for `render_changed`; 0x0 if the
    /// screen doesn't show them.
    prev: Buf,
    body: String,
}

impl SigilRenderer {
    /// A whole frame.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        cols: usize,
        rows: usize,
        mode: Mode,
        anim: &mut anim::SigilAnim,
        assets: &Assets,
        cache: &mut GeoCache,
        st: &GlobalState,
    ) -> &str {
        self.prev.reshape(0, 0);
        self.render_changed(cols, rows, mode, anim, assets, cache, st)
    }

    /// Only the cells that differ from the frame rendered before (which
    /// must be on screen); a whole frame if there is none of this size.
    #[allow(clippy::too_many_arguments)]
    pub fn render_changed(
        &mut self,
        cols: usize,
        rows: usize,
        mode: Mode,
        anim: &mut anim::SigilAnim,
        assets: &Assets,
        cache: &mut GeoCache,
        st: &GlobalState,
    ) -> &str {
        if !draw_into(&mut self.buf, cols, rows, mode, anim, assets, cache, st) {
            self.prev.reshape(0, 0);
            self.body.clear();
            self.body.push_str("\x1b[2J");
            return &self.body;
        }
        let (body, buf, prev) = (&mut self.body, &self.buf, Some(&self.prev));
        match mode {
            Mode::HalfBlock => encode_halfblock_into(body, buf, prev, 0, 0, cols, rows),
            Mode::Octant => encode_octant_into(body, buf, prev, 0, 0, cols, rows),
        }
        std::mem::swap(&mut self.buf, &mut self.prev);
        &self.body
    }
}

/// Advance the animation one tick and draw the frame's pixels for a
/// `cols`×`rows` pane; `None` if the pane is too small to draw in.
pub fn draw_frame(
    cols: usize,
    rows: usize,
    mode: Mode,
    anim: &mut anim::SigilAnim,
    assets: &Assets,
    cache: &mut GeoCache,
    st: &GlobalState,
) -> Option<Buf> {
    let mut buf = Buf::new(0, 0);
    draw_into(&mut buf, cols, rows, mode, anim, assets, cache, st).then_some(buf)
}

/// `draw_frame` into `buf`, reshaped to the frame's pixel size; `false` if
/// the pane is too small to draw in.
#[allow(clippy::too_many_arguments)]
fn draw_into(
    buf: &mut Buf,
    cols: usize,
    rows: usize,
    mode: Mode,
    anim: &mut anim::SigilAnim,
    assets: &Assets,
    cache: &mut GeoCache,
    st: &GlobalState,
) -> bool {
    anim.step(st.working, st.typing);
    let Some(fit) = fit::fit(cols, rows, mode) else {
        return false;
    };
    let geo = cache.get(fit.n, assets);
    buf.reshape(fit.px_w, fit.px_h);
    anim.draw(buf, &fit, geo, assets, mode);
    true
}

pub(crate) fn term_size() -> (usize, usize) {
    rustix::termios::tcgetwinsize(std::io::stdout())
        .map_or((0, 0), |w| (w.ws_col as usize, w.ws_row as usize))
}

/// The sigil pane loop. Returns when stdout closes (the pane is gone).
pub fn run(cfg: &Config) {
    let cfg = cfg.sanitized();
    let assets = load_assets(
        std::env::var_os("HERDR_PLUGIN_ROOT")
            .as_deref()
            .map(Path::new),
    );
    let dir = crate::paths::runtime_dir();
    let seed = ipc::now_ms() ^ u64::from(std::process::id());
    let mut anim = anim::SigilAnim::new(seed);
    anim.enable_familiars(cfg.fps);
    let mut cache = GeoCache::default();
    let mut renderer = SigilRenderer::default();
    let _tty = TtyGuard::new();
    let mut out = FrameWriter::new();
    if out.begin().is_err() {
        return;
    }
    let mut pacer = Pacer::new(cfg.fps);
    let own = ipc::own_tab();
    let mut gate = HiddenGate::default();
    // Most frames write only the cells that changed. A whole frame every
    // two seconds repairs anything that didn't reach the screen intact (a
    // pane resized there and back between two frames, say).
    let repaint_every = u64::from(cfg.fps) * 2;
    let mut frames = 0u64;
    loop {
        let (cols, rows) = term_size();
        let st = ipc::read_state(&dir, ipc::now_ms());
        // A hidden tab's sigil stops after its final frame (#2: ten tabs
        // animating unseen cost the plugin and herdr ~25% of a core).
        if !gate.should_draw(ipc::tab_hidden(own.as_deref(), &st), (cols, rows)) {
            std::thread::sleep(HIDDEN_POLL);
            continue;
        }
        let body = if frames.is_multiple_of(repaint_every) {
            renderer.render(cols, rows, cfg.render, &mut anim, &assets, &mut cache, &st)
        } else {
            renderer.render_changed(cols, rows, cfg.render, &mut anim, &assets, &mut cache, &st)
        };
        frames += 1;
        if out.frame(body).is_err() {
            return;
        }
        pacer.wait();
    }
}

/// The preview's width unless told otherwise: a typical sigil pane.
pub const PREVIEW_COLS: usize = 29;

/// The scenes and the width a preview was asked for:
/// `<cat|bats|toad|all> [columns]`.
fn parse_preview(args: &[String]) -> Option<(Vec<Scene>, usize)> {
    let (name, cols) = match args {
        [name] => (name, PREVIEW_COLS),
        [name, cols] => (name, cols.parse().ok()?),
        _ => return None,
    };
    let scenes = match name.as_str() {
        "all" => Scene::ALL.to_vec(),
        name => vec![Scene::parse(name)?],
    };
    Some((scenes, cols))
}

/// Plays scenes in turn, with a second before, between and after them.
struct Preview {
    scenes: VecDeque<Scene>,
    /// Frames in a second, and frames left of the current pause.
    second: u32,
    pause: u32,
    /// The scene asked for last, and whether it has been seen playing.
    current: Option<(Scene, bool)>,
    /// The scenes the pane had no room for.
    skipped: Vec<Scene>,
}

impl Preview {
    fn new(scenes: Vec<Scene>, fps: u8) -> Self {
        let second = u32::from(fps.max(1));
        Preview {
            scenes: scenes.into(),
            second,
            pause: second,
            current: None,
            skipped: Vec::new(),
        }
    }

    /// Once per frame, after drawing; `false` when the preview is over. A
    /// scene the pane has no room for is dropped by `Familiars`, so this
    /// never waits on one.
    fn next(&mut self, familiars: &mut Familiars) -> bool {
        if let Some((scene, played)) = &mut self.current {
            *played |= familiars.playing().is_some_and(|p| p.0 == *scene);
        }
        if !familiars.idle() {
            return true;
        }
        if let Some((scene, false)) = self.current.take() {
            self.skipped.push(scene);
        }
        if self.pause > 0 {
            self.pause -= 1;
            return true;
        }
        let Some(scene) = self.scenes.pop_front() else {
            return false;
        };
        familiars.force(scene);
        self.current = Some((scene, false));
        self.pause = self.second;
        true
    }
}

/// `herdr-witch scene <cat|bats|toad|all> [columns]`: play familiars now,
/// in this terminal, and return the exit code. Hidden (it is not in the
/// usage text): the scenes are meant to be come upon. The sigil is drawn
/// pane-sized at the left, idle whatever the daemon says, and the preview
/// ends by itself, since Ctrl-C is ignored here as in any witch pane.
pub fn preview(cfg: &Config, args: &[String]) -> i32 {
    let Some((scenes, cols)) = parse_preview(args) else {
        eprintln!("usage: herdr-witch scene cat|bats|toad|all [columns]");
        return 2;
    };
    let cfg = cfg.sanitized();
    let assets = load_assets(
        std::env::var_os("HERDR_PLUGIN_ROOT")
            .as_deref()
            .map(Path::new),
    );
    let mut anim = anim::SigilAnim::new(ipc::now_ms());
    anim.enable_familiars(cfg.fps);
    let mut preview = Preview::new(scenes, cfg.fps);
    let mut cache = GeoCache::default();
    let mut renderer = SigilRenderer::default();
    let idle = GlobalState::default();
    let _tty = TtyGuard::new();
    let mut out = FrameWriter::new();
    if out.begin().is_err() {
        return 1;
    }
    let mut pacer = Pacer::new(cfg.fps);
    loop {
        let (term_cols, rows) = term_size();
        let cols = cols.min(term_cols);
        // Nothing drawn means no frame for a scene to play on: stop rather
        // than wait for one.
        if fit::fit(cols, rows, cfg.render).is_none() {
            drop(out);
            eprintln!("herdr-witch scene: the terminal is too small to draw in");
            return 1;
        }
        let body = renderer.render(
            cols, rows, cfg.render, &mut anim, &assets, &mut cache, &idle,
        );
        if out.frame(body).is_err() {
            return 1;
        }
        let over = anim.familiars.as_mut().is_none_or(|f| !preview.next(f));
        if over {
            // The cursor back, on a line of its own under the last frame.
            drop(out);
            println!();
            for scene in &preview.skipped {
                let why = match scene {
                    Scene::Bats => "the sigil is too small",
                    Scene::Cat | Scene::Toad => {
                        "it needs the full-size witch under the sigil: at least 17 columns \
                         (34 with half-blocks), and rows for both"
                    }
                };
                eprintln!(
                    "herdr-witch scene: no room for the {} here ({why})",
                    scene.name()
                );
            }
            return 0;
        }
        pacer.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st() -> GlobalState {
        GlobalState {
            working: 2,
            typing: false,
            written_ms: 0,
            visible_tabs: Vec::new(),
        }
    }

    #[test]
    fn renderer_matches_render_frame_and_reuses_buffers() {
        let assets = load_assets(None);
        for mode in [Mode::Octant, Mode::HalfBlock] {
            let (mut a1, mut a2) = (anim::SigilAnim::new(7), anim::SigilAnim::new(7));
            let (mut c1, mut c2) = (GeoCache::default(), GeoCache::default());
            let mut r = SigilRenderer::default();
            let mut ptrs = Vec::new();
            for _ in 0..6 {
                let want = render_frame(29, 40, mode, &mut a1, &assets, &mut c1, &st());
                let got = r.render(29, 40, mode, &mut a2, &assets, &mut c2, &st());
                assert_eq!(got, want);
                ptrs.push((r.buf.d.as_ptr(), r.body.as_ptr()));
            }
            // The two pixel buffers swap each frame.
            assert!(ptrs[2..].iter().all(|p| p.1 == ptrs[2].1), "{mode:?}: body");
            assert_eq!((ptrs[2].0, ptrs[3].0), (ptrs[4].0, ptrs[5].0), "{mode:?}");
        }
    }

    /// A terminal that keeps what each cell last showed, colours included.
    #[derive(Default, PartialEq, Debug)]
    struct Screen {
        cells: std::collections::BTreeMap<(usize, usize), (String, String, char)>,
    }

    impl Screen {
        fn write(&mut self, body: &str) {
            let (mut row, mut col) = (1, 1);
            let (mut fg, mut bg) = (String::new(), String::new());
            let mut it = body.chars();
            while let Some(ch) = it.next() {
                if ch != '\x1b' {
                    // A space shows only its background.
                    let fg = if ch == ' ' { String::new() } else { fg.clone() };
                    self.cells.insert((row, col), (fg, bg.clone(), ch));
                    col += 1;
                    continue;
                }
                assert_eq!(it.next(), Some('['));
                let mut seq = String::new();
                let end = loop {
                    match it.next().expect("unterminated sequence") {
                        c if c.is_ascii_alphabetic() => break c,
                        c => seq.push(c),
                    }
                };
                match end {
                    'H' => {
                        let (r, c) = seq.split_once(';').unwrap();
                        (row, col) = (r.parse().unwrap(), c.parse().unwrap());
                    }
                    'm' if seq.starts_with("38;") => fg = seq,
                    'm' if seq.starts_with("48;") => bg = seq,
                    _ => panic!("unexpected sequence {seq:?}{end}"),
                }
            }
        }
    }

    #[test]
    fn changed_cells_alone_keep_the_screen_right() {
        let assets = load_assets(None);
        let busy = GlobalState {
            working: 3,
            typing: true,
            ..st()
        };
        for mode in [Mode::Octant, Mode::HalfBlock] {
            let (mut a1, mut a2) = (anim::SigilAnim::new(7), anim::SigilAnim::new(7));
            let (mut c1, mut c2) = (GeoCache::default(), GeoCache::default());
            let mut r = SigilRenderer::default();
            let mut diffed = Screen::default();
            let (mut full_len, mut diff_len) = (0, 0);
            // Idle, then busy, then a resize and back.
            for i in 0..120 {
                let state = if (40..80).contains(&i) { &busy } else { &st() };
                let (cols, rows) = if (90..100).contains(&i) {
                    (20, 30)
                } else {
                    (29, 40)
                };
                if i == 90 || i == 100 {
                    diffed = Screen::default(); // a resize: herdr reflows the pane
                }
                let want = render_frame(cols, rows, mode, &mut a1, &assets, &mut c1, state);
                let mut full = Screen::default();
                full.write(&want);
                let got = r.render_changed(cols, rows, mode, &mut a2, &assets, &mut c2, state);
                diffed.write(got);
                assert_eq!(diffed, full, "{mode:?} frame {i}");
                full_len += want.len();
                diff_len += got.len();
            }
            assert!(
                diff_len * 4 < full_len * 3,
                "{mode:?}: {diff_len} of {full_len}"
            );
        }
    }

    use familiars::{Stage, bats};

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn preview_arguments() {
        assert_eq!(
            parse_preview(&args("cat")),
            Some((vec![Scene::Cat], PREVIEW_COLS))
        );
        assert_eq!(
            parse_preview(&args("all 40")),
            Some((Scene::ALL.to_vec(), 40))
        );
        for bad in ["", "dog", "cat x", "cat 40 9"] {
            assert_eq!(parse_preview(&args(bad)), None, "{bad:?}");
        }
    }

    #[test]
    fn preview_names_the_scenes_it_could_not_play() {
        let run = |stage: Stage| {
            let mut f = Familiars::new(1, 12);
            let mut p = Preview::new(Scene::ALL.to_vec(), 12);
            loop {
                f.step(stage, 0.0);
                if !p.next(&mut f) {
                    return p.skipped;
                }
            }
        };
        // No witch: only the bats have room.
        let mini = Stage {
            n: 29,
            witch_x: None,
        };
        assert_eq!(run(mini), [Scene::Cat, Scene::Toad]);
        let big = Stage {
            n: 58,
            witch_x: Some(21),
        };
        assert_eq!(run(big), []);
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
        let big = Stage {
            n: 58,
            witch_x: Some(21),
        };
        let bats = (bats::duration() * 12.0) as u32;
        let one = preview_frames(vec![Scene::Bats], big);
        assert!(
            (bats + 20..=bats + 30).contains(&one),
            "a second each side: {one}"
        );
        // All three in turn: their lengths, and a second around each.
        let scenes =
            familiars::cat::duration(58, 21) + bats::duration() + familiars::toad::duration(21);
        let want = (scenes * 12.0) as u32 + 4 * 12;
        let all = preview_frames(Scene::ALL.to_vec(), big);
        assert!(
            (want..=want + 12).contains(&all),
            "{all} frames, about {want}"
        );
    }

    #[test]
    fn preview_ends_when_nothing_can_play() {
        // A terminal too small for the witch: the cat is dropped, not waited for.
        let tiny = Stage {
            n: 8,
            witch_x: None,
        };
        assert!(preview_frames(Scene::ALL.to_vec(), tiny) <= 12 * 6);
    }

    #[test]
    fn renderer_too_small_clears() {
        let assets = load_assets(None);
        let mut r = SigilRenderer::default();
        let mut a = anim::SigilAnim::new(1);
        let got = r.render(
            0,
            0,
            Mode::Octant,
            &mut a,
            &assets,
            &mut GeoCache::default(),
            &st(),
        );
        assert_eq!(got, "\x1b[2J");
    }
}
