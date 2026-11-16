use std::io::{self, Stdout, Write};

/// Owns the terminal for the sigil pane: hidden cursor, no autowrap, and
/// whole frames written atomically inside synchronized output.
pub struct FrameWriter<W: Write = Stdout> {
    out: W,
    /// The frame being written, kept across frames to reuse its allocation.
    scratch: String,
}

impl FrameWriter {
    pub fn new() -> Self {
        Self::with_writer(std::io::stdout())
    }
}

impl<W: Write> FrameWriter<W> {
    pub fn with_writer(out: W) -> Self {
        FrameWriter {
            out,
            scratch: String::new(),
        }
    }

    /// Hide the cursor, disable autowrap and clear the screen.
    pub fn begin(&mut self) -> io::Result<()> {
        self.out.write_all(b"\x1b[?25l\x1b[?7l\x1b[2J")?;
        self.out.flush()
    }

    /// Write one frame as a single `write_all` in synchronized output.
    pub fn frame(&mut self, body: &str) -> io::Result<()> {
        let buf = &mut self.scratch;
        buf.clear();
        buf.push_str("\x1b[?2026h\x1b[H");
        buf.push_str(body);
        buf.push_str("\x1b[0m\x1b[?2026l");
        self.out.write_all(buf.as_bytes())?;
        self.out.flush()
    }
}

impl Default for FrameWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl<W: Write> Drop for FrameWriter<W> {
    fn drop(&mut self) {
        let _ = self.out.write_all(b"\x1b[?25h\x1b[?7h\x1b[0m");
        let _ = self.out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_wraps_body_and_reuses_scratch() {
        let mut w = FrameWriter::with_writer(Vec::new());
        w.frame("AB").unwrap();
        let ptr = w.scratch.as_ptr();
        w.frame("CD").unwrap();
        assert_eq!(w.scratch.as_ptr(), ptr, "scratch reused across frames");
        let want = "\x1b[?2026h\x1b[HAB\x1b[0m\x1b[?2026l\x1b[?2026h\x1b[HCD\x1b[0m\x1b[?2026l";
        assert_eq!(String::from_utf8(w.out.clone()).unwrap(), want);
    }
}
