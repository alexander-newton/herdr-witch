pub mod curse;
pub mod pool;
pub mod view;

use crate::config::Config;
use crate::ipc;
use crate::render::frame::FrameWriter;
use crate::render::pacer::{HIDDEN_POLL, HiddenGate, Pacer};
use crate::render::tty::TtyGuard;
use crate::sigil::term_size;
use pool::LinePool;
use std::collections::HashSet;
use view::GrimView;

/// The incantations, one per line (`#` comments). The daemon assigns tabs
/// indices into this pool, so both sides parse the same text.
pub const GRIMOIRE: &str = include_str!("../../assets/grimoire.txt");

/// The line this pane shows: `HERDR_WITCH_LINE` if it is a valid index, else random.
fn pick_line(pool: &mut LinePool) -> usize {
    std::env::var("HERDR_WITCH_LINE")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&i| i < pool.lines.len())
        .unwrap_or_else(|| pool.assign(&HashSet::new()))
}

/// The grimoire pane loop. Returns when stdout closes (the pane is gone).
pub fn run(cfg: &Config) {
    let cfg = cfg.sanitized();
    let dir = crate::paths::runtime_dir();
    let seed = ipc::now_ms() ^ u64::from(std::process::id());
    let mut pool = LinePool::from_text(GRIMOIRE, seed);
    if pool.lines.is_empty() {
        return;
    }
    let idx = pick_line(&mut pool);
    let mut view = GrimView::new("", cfg.curse_level, seed);
    view.set_frame_ms(1000 / u32::from(cfg.fps));
    let _tty = TtyGuard::new();
    let mut out = FrameWriter::new();
    if out.begin().is_err() {
        return;
    }
    let mut pacer = Pacer::new(cfg.fps);
    let mut fitted_cols = usize::MAX;
    let own = ipc::own_tab();
    let mut gate = HiddenGate::default();
    let mut shown = String::new();
    loop {
        let (cols, rows) = term_size();
        let st = ipc::read_state(&dir, ipc::now_ms());
        // Like the sigil: a hidden tab's grimoire stops after its final frame.
        if !gate.should_draw(ipc::tab_hidden(own.as_deref(), &st), (cols, rows)) {
            std::thread::sleep(HIDDEN_POLL);
            continue;
        }
        if cols != fitted_cols {
            fitted_cols = cols;
            view.set_line(&pool.fit_line(idx, cols));
        }
        view.tick(st.working);
        // Many ticks change nothing; the line on screen is then left alone.
        let body = view.render_line(cols, rows);
        if body != shown {
            if out.frame(&body).is_err() {
                return;
            }
            shown = body;
        }
        pacer.wait();
    }
}
