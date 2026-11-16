use super::curse::display_width;
use fastrand::Rng;
use std::collections::HashSet;

/// The grimoire lines plus a shuffled hand-out order.
pub struct LinePool {
    pub lines: Vec<String>,
    pub order: Vec<usize>,
    pub next: usize,
    rng: Rng,
}

impl LinePool {
    /// Parse the asset text (skipping blanks and `#` comments) and shuffle.
    pub fn from_text(text: &str, seed: u64) -> Self {
        let lines: Vec<String> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect();
        let mut pool = LinePool {
            order: Vec::new(),
            next: 0,
            rng: Rng::with_seed(seed),
            lines,
        };
        pool.reshuffle();
        pool
    }

    fn reshuffle(&mut self) {
        self.order = (0..self.lines.len()).collect();
        self.rng.shuffle(&mut self.order);
        self.next = 0;
    }

    fn take_free(&mut self, live: &HashSet<usize>) -> Option<usize> {
        let pos = (self.next..self.order.len()).find(|&i| !live.contains(&self.order[i]))?;
        self.order.swap(self.next, pos);
        self.next += 1;
        Some(self.order[self.next - 1])
    }

    /// Next line index (into `lines`) in shuffled order that no tab shows yet.
    /// Reshuffles once the pool is used up; if every line is live, repeats.
    pub fn assign(&mut self, live: &HashSet<usize>) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        if let Some(i) = self.take_free(live) {
            return i;
        }
        self.reshuffle();
        self.take_free(live).unwrap_or(self.order[0])
    }

    /// The line cut to at most `width` columns: at the last space that fits,
    /// or hard-cut when even the first word is too wide.
    pub fn fit_line(&self, idx: usize, width: usize) -> String {
        let Some(line) = self.lines.get(idx) else {
            return String::new();
        };
        if display_width(line) <= width {
            return line.clone();
        }
        let mut end = 0;
        let mut w = 0;
        let mut last_space = None;
        for (i, c) in line.char_indices() {
            w += display_width(c.encode_utf8(&mut [0; 4]));
            if w > width {
                break;
            }
            end = i + c.len_utf8();
            if c == ' ' {
                last_space = Some(i);
            }
        }
        // A space exactly at the cut also means the previous word is whole.
        let next_is_space = line[end..].starts_with(' ');
        match last_space {
            _ if next_is_space => line[..end].trim_end().to_string(),
            Some(i) => line[..i].trim_end().to_string(),
            None => line[..end].to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(n: usize) -> LinePool {
        let text: String = (0..n).map(|i| format!("line number {i}\n")).collect();
        LinePool::from_text(&format!("# header\n\n{text}"), 7)
    }

    #[test]
    fn skips_blanks_and_comments() {
        assert_eq!(pool(5).lines.len(), 5);
    }

    #[test]
    fn pool_no_live_duplicates() {
        let mut p = pool(5);
        let free = p.order[3];
        let live: HashSet<usize> = (0..5).filter(|&i| i != free).collect();
        assert_eq!(p.assign(&live), free);
    }

    #[test]
    fn pool_no_repeat_until_exhausted() {
        let mut p = pool(5);
        let mut got: Vec<usize> = (0..5).map(|_| p.assign(&HashSet::new())).collect();
        got.sort_unstable();
        assert_eq!(got, vec![0, 1, 2, 3, 4]);
        assert!(p.assign(&HashSet::new()) < 5);
    }

    #[test]
    fn assign_is_deterministic_per_seed() {
        let (mut a, mut b) = (pool(9), pool(9));
        let none = HashSet::new();
        for _ in 0..12 {
            assert_eq!(a.assign(&none), b.assign(&none));
        }
    }

    #[test]
    fn fits_and_truncates_at_word() {
        let p = LinePool::from_text("Behold the Pentacle of Solomon\n", 1);
        assert_eq!(p.fit_line(0, 20), "Behold the Pentacle");
        assert_eq!(p.fit_line(0, 30), "Behold the Pentacle of Solomon");
        assert_eq!(p.fit_line(0, 3), "Beh");
        assert_eq!(p.fit_line(0, 0), "");
    }

    #[test]
    fn grimoire_txt_lengths() {
        let p = LinePool::from_text(include_str!("../../assets/grimoire.txt"), 1);
        assert!(p.lines.len() >= 60, "{} lines", p.lines.len());
        for l in &p.lines {
            let n = l.chars().count();
            assert!((30..=70).contains(&n), "{n}: {l}");
        }
    }
}
