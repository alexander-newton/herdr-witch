use std::time::{Duration, Instant};

/// How often a pane in a hidden tab checks whether it is visible again.
pub const HIDDEN_POLL: Duration = Duration::from_millis(250);

/// Stops a pane drawing while its tab is hidden: the frame already on
/// screen is the final one, until the pane is resized or shown again.
#[derive(Debug, Default)]
pub struct HiddenGate {
    /// The size of the last frame drawn.
    drawn: Option<(usize, usize)>,
}

impl HiddenGate {
    /// Whether to draw a frame now; `size` is the pane's (cols, rows).
    pub fn should_draw(&mut self, hidden: bool, size: (usize, usize)) -> bool {
        if hidden && self.drawn == Some(size) {
            return false;
        }
        self.drawn = Some(size);
        true
    }
}

/// Deadline-based frame pacing: sleeps to the next frame boundary and
/// resynchronises after an overrun instead of bursting to catch up.
pub struct Pacer {
    period: Duration,
    next: Instant,
}

impl Pacer {
    pub fn new(fps: u8) -> Self {
        Pacer {
            period: Duration::from_secs_f64(1.0 / f64::from(fps.max(1))),
            next: Instant::now(),
        }
    }

    /// Call once per frame, after drawing.
    pub fn wait(&mut self) {
        self.next += self.period;
        let now = Instant::now();
        if self.next > now {
            std::thread::sleep(self.next - now);
        } else {
            self.next = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_gate_skips_after_final_frame() {
        let mut g = HiddenGate::default();
        assert!(g.should_draw(false, (10, 5)));
        assert!(g.should_draw(false, (10, 5)));
        // Hidden: the frame already on screen is the final one.
        assert!(!g.should_draw(true, (10, 5)));
        assert!(!g.should_draw(true, (10, 5)));
        // Resized while hidden: one frame at the new size, then nothing.
        assert!(g.should_draw(true, (12, 5)));
        assert!(!g.should_draw(true, (12, 5)));
        assert!(g.should_draw(false, (12, 5)), "visible again: full rate");
        // Hidden before anything was drawn: draw once.
        let mut g = HiddenGate::default();
        assert!(g.should_draw(true, (10, 5)));
        assert!(!g.should_draw(true, (10, 5)));
    }

    #[test]
    fn paces_to_period() {
        let mut p = Pacer::new(20);
        let t = Instant::now();
        p.wait();
        p.wait();
        assert!(t.elapsed() >= Duration::from_millis(95));
    }

    #[test]
    fn overrun_does_not_burst() {
        let mut p = Pacer::new(50);
        std::thread::sleep(Duration::from_millis(100));
        let t = Instant::now();
        p.wait();
        p.wait();
        // First wait resyncs without sleeping; the second sleeps one period (20 ms).
        assert!(t.elapsed() < Duration::from_millis(60), "{:?}", t.elapsed());
    }
}
