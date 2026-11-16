use super::curse::{curse, display_width};
use fastrand::Rng;
use std::fmt::Write;

const GOLD: [u8; 3] = [245, 197, 66];
const BRIGHT: [u8; 3] = [255, 232, 150];
const CYAN: [u8; 3] = [79, 240, 255];
const DIP: [u8; 3] = [176, 127, 224];

pub struct Word {
    raw: String,
    text: String,
    hot_until: u64,
    flare_until: u64,
    dim: bool,
}

/// One grimoire line: cursed words that shimmer by changing colour.
pub struct GrimView {
    pub words: Vec<Word>,
    pub rng: Rng,
    level: u8,
    clock_ms: u64,
    frame_ms: u32,
}

impl GrimView {
    pub fn new(line: &str, level: u8, seed: u64) -> Self {
        let mut v = GrimView {
            words: Vec::new(),
            rng: Rng::with_seed(seed),
            level,
            clock_ms: 0,
            frame_ms: 83,
        };
        v.set_line(line);
        v
    }

    /// Milliseconds the internal clock advances per `tick`.
    pub fn set_frame_ms(&mut self, ms: u32) {
        self.frame_ms = ms;
    }

    /// Replace the (uncursed) line, cursing every word afresh.
    pub fn set_line(&mut self, line: &str) {
        let level = self.level;
        let rng = &mut self.rng;
        self.words = line
            .split(' ')
            .filter(|w| !w.is_empty())
            .map(|w| Word {
                raw: w.to_string(),
                text: curse(w, level, rng),
                hot_until: 0,
                flare_until: 0,
                dim: false,
            })
            .collect();
    }

    fn pick(&mut self) -> Option<usize> {
        (!self.words.is_empty()).then(|| self.rng.usize(..self.words.len()))
    }

    pub fn tick(&mut self, working: u32) {
        self.clock_ms += u64::from(self.frame_ms);
        if self.words.is_empty() {
            return;
        }
        if self.rng.f32() < 0.35
            && let Some(i) = self.pick()
        {
            let w = &mut self.words[i];
            w.text = curse(&w.raw, self.level, &mut self.rng);
        }
        if self.rng.f32() < 0.08 + 0.02 * working as f32
            && let Some(i) = self.pick()
        {
            self.words[i].flare_until = self.clock_ms + 220 + u64::from(self.rng.u32(0..300));
        }
        if self.rng.f32() < 0.15
            && let Some(i) = self.pick()
        {
            self.words[i].dim ^= true;
        }
        if self.rng.f32() < 0.2
            && let Some(i) = self.pick()
        {
            self.words[i].hot_until = self.clock_ms + 400;
        }
    }

    fn colour(&self, w: &Word) -> [u8; 3] {
        if w.flare_until > self.clock_ms {
            CYAN
        } else if w.hot_until > self.clock_ms {
            BRIGHT
        } else if w.dim {
            DIP
        } else {
            GOLD
        }
    }

    /// Frame body: clear, then the line centred on row `rows/2`, never wider than `cols`.
    pub fn render_line(&self, cols: usize, rows: usize) -> String {
        let mut out = String::from("\x1b[2J");
        if cols == 0 || rows == 0 {
            return out;
        }
        let total: usize = self
            .words
            .iter()
            .map(|w| display_width(&w.raw))
            .sum::<usize>()
            + self.words.len().saturating_sub(1);
        let width = total.min(cols);
        let _ = write!(
            out,
            "\x1b[{};{}H\x1b[49m",
            rows / 2 + 1,
            (cols - width) / 2 + 1
        );
        let mut used = 0;
        for (n, w) in self.words.iter().enumerate() {
            if used >= width {
                break;
            }
            if n > 0 {
                out.push(' ');
                used += 1;
                if used >= width {
                    break;
                }
            }
            let [r, g, b] = self.colour(w);
            let _ = write!(out, "\x1b[38;2;{r};{g};{b}m");
            for c in w.text.chars() {
                let cw = display_width(c.encode_utf8(&mut [0; 4]));
                if used + cw > width {
                    break;
                }
                out.push(c);
                used += cw;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grimoire::curse::display_width;

    fn visible(s: &str) -> usize {
        let mut out = String::new();
        let mut it = s.chars().peekable();
        while let Some(c) = it.next() {
            if c == '\x1b' {
                for d in it.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        display_width(&out)
    }

    #[test]
    fn render_line_centred() {
        let v = GrimView::new("abcde fghi", 0, 1);
        let s = v.render_line(30, 3);
        assert!(s.starts_with("\x1b[2J\x1b[2;11H"), "{s:?}");
        assert!(!s.contains('\n'));
        assert!(s.contains("\x1b[49m"));
        assert!(s.contains("38;2;245;197;66"));
    }

    #[test]
    fn render_line_empty_dims() {
        let v = GrimView::new("abc", 4, 1);
        assert_eq!(v.render_line(0, 3), "\x1b[2J");
        assert_eq!(v.render_line(10, 0), "\x1b[2J");
    }

    #[test]
    fn render_line_tiny_widths() {
        let v = GrimView::new("Behold the Pentacle", 4, 5);
        for cols in 0..=3 {
            let s = v.render_line(cols, 1);
            assert!(visible(&s) <= cols, "cols {cols}");
        }
    }

    #[test]
    fn tick_is_deterministic_and_uses_palette() {
        let mut a = GrimView::new("one two three four five", 4, 9);
        let mut b = GrimView::new("one two three four five", 4, 9);
        let mut seen = String::new();
        for _ in 0..300 {
            a.tick(5);
            b.tick(5);
            let s = a.render_line(80, 3);
            assert_eq!(s, b.render_line(80, 3));
            seen.push_str(&s);
        }
        for rgb in ["245;197;66", "255;232;150", "79;240;255", "176;127;224"] {
            assert!(seen.contains(rgb), "missing {rgb}");
        }
    }

    #[test]
    fn hot_expires_on_internal_clock() {
        let mut v = GrimView::new("one two", 0, 3);
        v.set_frame_ms(1000);
        for _ in 0..50 {
            v.tick(0);
        }
        // With 1 s frames every 400 ms hot flag lapses before the next tick,
        // so at most the word hot set this very tick can be bright.
        let s = v.render_line(40, 1);
        assert!(s.matches("255;232;150").count() <= 1);
    }
}
