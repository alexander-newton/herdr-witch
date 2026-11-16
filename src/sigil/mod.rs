pub mod anim;
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
use fit::Mode;
use geometry::SigilGeometry;
use sprites::{Assets, load_assets};
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
