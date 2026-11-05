//! The backend abstraction: pluggable terminal renderers, plus the
//! [`TestBackend`] used to drive the whole stack in tests without a real
//! terminal.

use core::convert::Infallible;

use crate::buffer::{Buffer, DiffRun};
use crate::clipboard::ClipboardSelection;
use crate::geometry::{Position, Rect, Size};

/// A region of the screen to clear.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClearRegion {
    /// The whole screen.
    #[default]
    All,
    /// Everything after the cursor (exclusive of the cursor cell).
    AfterCursor,
    /// Everything before the cursor.
    BeforeCursor,
    /// The line the cursor is on.
    CurrentLine,
    /// From the cursor to the end of its line.
    UntilNewLine,
}

/// A pluggable terminal renderer.
///
/// Backends receive [`DiffRun`]s — contiguous slices of changed cells — and
/// translate them into terminal I/O. Implement this trait to support a new
/// terminal library or protocol; `tuvix-backend-crossterm` is the reference
/// implementation.
///
/// Errors are backend-defined through the [`Backend::Error`] associated type
/// so that in-memory backends can be infallible.
pub trait Backend {
    /// The error type produced by backend operations.
    type Error: core::error::Error;

    /// Writes the given changed-cell runs to the terminal.
    fn draw<'a>(
        &mut self,
        content: impl IntoIterator<Item = DiffRun<'a>>,
    ) -> Result<(), Self::Error>;

    /// Flushes queued output to the terminal.
    fn flush(&mut self) -> Result<(), Self::Error>;

    /// Current terminal size in cells.
    fn size(&self) -> Result<Size, Self::Error>;

    /// Current cursor position.
    fn get_cursor_position(&self) -> Result<Position, Self::Error>;

    /// Moves the cursor.
    fn set_cursor_position(&mut self, position: Position) -> Result<(), Self::Error>;

    /// Hides the cursor.
    fn hide_cursor(&mut self) -> Result<(), Self::Error>;

    /// Shows the cursor.
    fn show_cursor(&mut self) -> Result<(), Self::Error>;

    /// Clears `region`.
    fn clear(&mut self, region: ClearRegion) -> Result<(), Self::Error>;

    /// Writes a raw terminal control sequence, such as a kitty keyboard
    /// protocol push (`ESC [ > flags u`) or a DEC private mode toggle.
    ///
    /// The default implementation is a no-op for backends that cannot
    /// forward raw control sequences; the reference implementations
    /// (`CrosstermBackend` writes them, `TestBackend` records them) override
    /// it.
    fn write_control(&mut self, sequence: &str) -> Result<(), Self::Error> {
        let _ = sequence;
        Ok(())
    }

    /// Asks the terminal to put `contents` on the system clipboard (OSC
    /// 52). The terminal may ignore the request or prompt the user; there
    /// is deliberately no readback — clipboard *queries* are the variant
    /// terminals gate behind prompts, so the framework only writes.
    ///
    /// Empty contents ask the terminal to clear the clipboard. The
    /// payload is base64 (escape-free by construction), capped at
    /// [`OSC52_MAX_PAYLOAD`](crate::clipboard::OSC52_MAX_PAYLOAD) —
    /// oversized requests fail rather than truncate.
    ///
    /// The default implementation is a no-op for backends that cannot
    /// reach the clipboard; the reference implementations override it
    /// (`AnsiBackend`/`CrosstermBackend` write the sequence, `TestBackend`
    /// records it in the control log).
    fn set_clipboard(&mut self, contents: &str) -> Result<(), Self::Error> {
        let _ = contents;
        Ok(())
    }
}

/// An in-memory backend for deterministic tests of the whole rendering stack.
///
/// It applies the diff runs it receives onto a plain [`Buffer`], exactly like
/// a real terminal would apply them to the screen, so tests can assert on the
/// visible state after any sequence of frames.
///
/// Raw control sequences ([`Backend::write_control`]) are recorded in a
/// [`control_log`](Self::control_log) instead of being applied, which makes
/// the input stack and runner testable end to end.
///
/// # Examples
///
/// ```
/// use tuvix_core::backend::TestBackend;
/// use tuvix_core::geometry::Size;
/// use tuvix_core::style::{Color, Style};
/// use tuvix_core::terminal::Terminal;
///
/// let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 2))).unwrap();
/// terminal
///     .draw(|frame| {
///         frame.buffer.set_string(0, 0, "hi", Style::new().fg(Color::Red));
///     })
///     .unwrap();
/// assert_eq!(terminal.backend().buffer().to_string(), "hi        \n          ");
/// ```
#[derive(Debug, Clone)]
pub struct TestBackend {
    buffer: Buffer,
    cursor: Position,
    hidden_cursor: bool,
    control_log: Vec<String>,
}

impl TestBackend {
    /// Creates a test backend with the given cell grid.
    pub fn new(size: Size) -> Self {
        Self {
            buffer: Buffer::empty(Rect::from_size(size)),
            cursor: Position::new(0, 0),
            hidden_cursor: false,
            control_log: Vec::new(),
        }
    }

    /// The visible state: what a real terminal would show.
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// The current cursor position.
    pub fn cursor_position(&self) -> Position {
        self.cursor
    }

    /// Whether the cursor is hidden.
    pub fn is_cursor_hidden(&self) -> bool {
        self.hidden_cursor
    }

    /// Simulates a terminal resize (test utility).
    pub fn resize(&mut self, size: Size) {
        self.buffer.resize(Rect::from_size(size));
    }

    /// The raw control sequences recorded so far, in write order.
    pub fn control_log(&self) -> &[String] {
        &self.control_log
    }
}

impl Backend for TestBackend {
    type Error = Infallible;

    fn draw<'a>(
        &mut self,
        content: impl IntoIterator<Item = DiffRun<'a>>,
    ) -> Result<(), Self::Error> {
        for run in content {
            for (index, cell) in run.cells.iter().enumerate() {
                let x = run.start + index as u16;
                // Emulate real terminal behavior: overwriting either half of
                // a wide glyph clears the whole pair, even when the neighbor
                // cell is not part of this diff batch.
                if let Some(previous) = self.buffer.get(x, run.row) {
                    if previous.is_wide() {
                        self.buffer
                            .clear_with_pair_repair(x.saturating_add(1), run.row);
                    } else if previous.skip
                        && let Some(previous) = x.checked_sub(1)
                    {
                        self.buffer.clear_with_pair_repair(previous, run.row);
                    }
                }
                if let Some(target) = self.buffer.get_mut(x, run.row) {
                    *target = cell.clone();
                }
            }
            if run.elide_rest {
                // Emulate EL: everything after the written cells blanks out.
                let width = self.buffer.area.width;
                let from = run.start + u16::try_from(run.cells.len()).unwrap_or(width);
                for x in from..width {
                    if let Some(target) = self.buffer.get_mut(x, run.row) {
                        target.reset();
                    }
                }
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(self.buffer.size())
    }

    fn get_cursor_position(&self) -> Result<Position, Self::Error> {
        Ok(self.cursor)
    }

    fn set_cursor_position(&mut self, position: Position) -> Result<(), Self::Error> {
        self.cursor = position;
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.hidden_cursor = true;
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.hidden_cursor = false;
        Ok(())
    }

    fn clear(&mut self, region: ClearRegion) -> Result<(), Self::Error> {
        if region == ClearRegion::All {
            self.buffer.reset();
        }
        Ok(())
    }

    fn write_control(&mut self, sequence: &str) -> Result<(), Self::Error> {
        self.control_log.push(sequence.to_string());
        Ok(())
    }

    fn set_clipboard(&mut self, contents: &str) -> Result<(), Self::Error> {
        // The log cannot fail (the error type is Infallible), so an
        // oversized payload is skipped here; real backends return the
        // [`ClipboardError`](crate::clipboard::ClipboardError) instead.
        if let Ok(sequence) = crate::clipboard::encode_osc52(ClipboardSelection::System, contents) {
            self.control_log.push(sequence);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Symbol;
    use crate::style::Style;

    mod clear_region {
        use super::*;

        #[test]
        fn default_is_all() {
            assert_eq!(ClearRegion::default(), ClearRegion::All);
        }
    }

    mod test_backend {
        use super::*;
        use crate::buffer::Cell;

        /// Paints a horizontal run directly through the backend draw path.
        fn draw_row(backend: &mut TestBackend, start: u16, symbols: &[&str]) {
            let cells: Vec<Cell> = symbols
                .iter()
                .map(|s| Cell {
                    symbol: Symbol::new(*s),
                    style: Style::new(),
                    hyperlink: None,
                    skip: false,
                })
                .collect();
            let run = DiffRun {
                row: 0,
                start,
                cells: &cells,
                elide_rest: false,
            };
            backend.draw([run]).unwrap();
        }

        #[test]
        fn starts_blank_with_cursor_visible() {
            let backend = TestBackend::new(Size::new(4, 2));
            assert_eq!(backend.buffer().to_string(), "    \n    ");
            assert!(!backend.is_cursor_hidden());
            assert_eq!(backend.cursor_position(), Position::new(0, 0));
        }

        #[test]
        fn resize_rebuilds_grid() {
            let mut backend = TestBackend::new(Size::new(4, 2));
            backend.resize(Size::new(6, 1));
            assert_eq!(backend.buffer().size(), Size::new(6, 1));
        }

        #[test]
        fn cursor_roundtrip_and_visibility() {
            let mut backend = TestBackend::new(Size::new(4, 2));
            backend.set_cursor_position(Position::new(3, 1)).unwrap();
            assert_eq!(backend.get_cursor_position().unwrap(), Position::new(3, 1));
            backend.hide_cursor().unwrap();
            assert!(backend.is_cursor_hidden());
            backend.show_cursor().unwrap();
            assert!(!backend.is_cursor_hidden());
        }

        #[test]
        fn draw_applies_runs_to_visible_state() {
            let mut backend = TestBackend::new(Size::new(6, 1));
            draw_row(&mut backend, 0, &["h", "i"]);
            assert_eq!(backend.buffer().to_string(), "hi    ");
        }

        #[test]
        fn draw_outside_bounds_is_ignored() {
            let mut backend = TestBackend::new(Size::new(4, 1));
            draw_row(&mut backend, 8, &["x"]);
            assert_eq!(backend.buffer().to_string(), "    ");
        }

        #[test]
        fn clear_all_resets_grid() {
            let mut backend = TestBackend::new(Size::new(6, 1));
            draw_row(&mut backend, 0, &["h", "i"]);
            backend.clear(ClearRegion::All).unwrap();
            assert_eq!(backend.buffer().to_string(), "      ");
        }

        #[test]
        fn clear_other_regions_do_not_reset() {
            let mut backend = TestBackend::new(Size::new(6, 1));
            draw_row(&mut backend, 0, &["h", "i"]);
            backend.clear(ClearRegion::UntilNewLine).unwrap();
            assert_eq!(backend.buffer().to_string(), "hi    ");
        }

        #[test]
        fn write_control_records_sequences_in_order() {
            let mut backend = TestBackend::new(Size::new(4, 1));
            backend.write_control("\x1b[?2004h").unwrap();
            backend.write_control("\x1b[>1u").unwrap();
            assert_eq!(
                backend.control_log(),
                ["\x1b[?2004h", "\x1b[>1u"].map(String::from)
            );
        }

        #[test]
        fn write_control_does_not_touch_the_visible_grid() {
            let mut backend = TestBackend::new(Size::new(4, 1));
            draw_row(&mut backend, 0, &["h", "i"]);
            backend.write_control("\x1b[>1u").unwrap();
            assert_eq!(backend.buffer().to_string(), "hi  ");
        }
    }
}
