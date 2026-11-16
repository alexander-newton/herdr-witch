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

/// `render_frame` with the pixel buffer and the encoded body kept across
/// frames, so a steady pane allocates nothing per frame for them.
pub struct SigilRenderer {
    buf: Buf,
    body: String,
}

impl Default for SigilRenderer {
    fn default() -> Self {
        SigilRenderer {
            buf: Buf::new(0, 0),
            body: String::new(),
        }
    }
}

impl SigilRenderer {
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
        if !draw_into(&mut self.buf, cols, rows, mode, anim, assets, cache, st) {
            self.body.clear();
            self.body.push_str("\x1b[2J");
            return &self.body;
        }
        match mode {
            Mode::HalfBlock => encode_halfblock_into(&mut self.body, &self.buf, 0, 0, cols, rows),
            Mode::Octant => encode_octant_into(&mut self.body, &self.buf, 0, 0, cols, rows),
        }
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
    loop {
        let (cols, rows) = term_size();
        let st = ipc::read_state(&dir, ipc::now_ms());
        // A hidden tab's sigil stops after its final frame (#2: ten tabs
        // animating unseen cost the plugin and herdr ~25% of a core).
        if !gate.should_draw(ipc::tab_hidden(own.as_deref(), &st), (cols, rows)) {
            std::thread::sleep(HIDDEN_POLL);
            continue;
        }
        let body = renderer.render(cols, rows, cfg.render, &mut anim, &assets, &mut cache, &st);
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
            for _ in 0..4 {
                let want = render_frame(29, 40, mode, &mut a1, &assets, &mut c1, &st());
                let got = r.render(29, 40, mode, &mut a2, &assets, &mut c2, &st());
                assert_eq!(got, want);
                ptrs.push((r.buf.d.as_ptr(), r.body.as_ptr()));
            }
            assert!(ptrs[1..].iter().all(|p| *p == ptrs[1]), "{mode:?}: reused");
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
