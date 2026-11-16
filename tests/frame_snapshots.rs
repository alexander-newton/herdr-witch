use herdr_witch::ipc::GlobalState;
use herdr_witch::sigil::GeoCache;
use herdr_witch::sigil::anim::SigilAnim;
use herdr_witch::sigil::fit::Mode;
use herdr_witch::sigil::render_frame;
use herdr_witch::sigil::sprites::load_assets;
use unicode_width::UnicodeWidthChar;

fn state(working: u32) -> GlobalState {
    GlobalState {
        working,
        typing: false,
        written_ms: 0,
        visible_tabs: Vec::new(),
    }
}

fn frame(cols: usize, rows: usize, mode: Mode, steps: usize) -> String {
    let assets = load_assets(None);
    let mut anim = SigilAnim::new(7);
    let mut cache = GeoCache::default();
    let st = state(2);
    let mut out = String::new();
    for _ in 0..steps {
        out = render_frame(cols, rows, mode, &mut anim, &assets, &mut cache, &st);
    }
    out
}

fn visible(s: &str) -> String {
    s.replace('\x1b', "<ESC>")
}

#[test]
fn frame_halfblock_29x40() {
    insta::assert_snapshot!(visible(&frame(29, 40, Mode::HalfBlock, 10)));
}

#[test]
fn frame_octant_29x40() {
    insta::assert_snapshot!(visible(&frame(29, 40, Mode::Octant, 10)));
}

#[test]
fn frame_tiny_is_blank() {
    let out = frame(2, 1, Mode::Octant, 1);
    assert_eq!(out, "\x1b[2J");
    assert!(!out.contains('▀'));
    let out = frame(2, 1, Mode::HalfBlock, 1);
    assert!(!out.contains('▀'));
}

/// Printable cells per row: chars between CUPs, skipping SGR sequences.
fn row_widths(out: &str) -> Vec<usize> {
    let mut widths = Vec::new();
    let mut chars = out.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            assert_eq!(chars.next(), Some('['));
            let mut body = String::new();
            let mut fin = ' ';
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    fin = d;
                    break;
                }
                body.push(d);
            }
            if fin == 'H' {
                widths.push(0);
            }
        } else if let Some(w) = widths.last_mut() {
            *w += c.width().unwrap_or(1);
        }
    }
    widths
}

#[test]
fn frame_never_exceeds_width() {
    let assets = load_assets(None);
    for mode in [Mode::HalfBlock, Mode::Octant] {
        let mut anim = SigilAnim::new(3);
        let mut cache = GeoCache::default();
        let st = state(4);
        for cols in 4..=60 {
            for rows in 2..=50 {
                let out = render_frame(cols, rows, mode, &mut anim, &assets, &mut cache, &st);
                let w = row_widths(&out);
                assert_eq!(w.len(), rows, "{cols}x{rows}");
                assert!(w.iter().all(|&x| x <= cols), "{cols}x{rows}: {w:?}");
            }
        }
    }
}
