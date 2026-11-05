//! The crossterm-powered [`Backend`] implementation.
//!
//! The renderer streams [`DiffRun`]s through a **persistent raster state
//! machine** (the notcurses technique): current cursor, SGR attributes and
//! colors are memoized *across frames*, so every emission is a delta against
//! what the terminal actually shows — not against a per-draw reset.
//!
//! Three byte-savers drive M1:
//!
//! 1. **SGR persistence** — no `ResetColor` at the end of a draw; styles
//!    survive between frames and are only invalidated by
//!    [`Backend::clear`] (resize, screen wipe).
//! 2. **Cursor economy** — same-row jumps emit column addressing (CHA,
//!    one parameter) instead of full cursor moves; continuing runs emit
//!    nothing.
//! 3. **EL elision** — runs marked `elide_rest` by the diff engine erase
//!    the row tail with one `EL` control instead of writing N blank cells.

use std::io::Write;

use crossterm::cursor::{Hide, MoveTo, MoveToColumn, Show};
use crossterm::queue;
use crossterm::style::{
    Attribute, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    SetUnderlineColor,
};
use crossterm::terminal::{Clear, ClearType};
use tuvix_core::backend::{Backend, ClearRegion};
use tuvix_core::buffer::{Cell, DiffRun};
use tuvix_core::clipboard::{ClipboardSelection, encode_osc52};
use tuvix_core::geometry::{Position, Size};
use tuvix_core::style::{Color, Hyperlink, Modifier};

use crate::event::to_crossterm_color;

/// Map of tuvix modifier bits to their SGR on/off attributes.
const MODIFIER_ATTRIBUTES: &[(Modifier, Attribute, Attribute)] = &[
    (Modifier::BOLD, Attribute::Bold, Attribute::NormalIntensity),
    (Modifier::DIM, Attribute::Dim, Attribute::NormalIntensity),
    (Modifier::ITALIC, Attribute::Italic, Attribute::NoItalic),
    (
        Modifier::UNDERLINED,
        Attribute::Underlined,
        Attribute::NoUnderline,
    ),
    (
        Modifier::SLOW_BLINK,
        Attribute::SlowBlink,
        Attribute::NoBlink,
    ),
    (
        Modifier::RAPID_BLINK,
        Attribute::RapidBlink,
        Attribute::NoBlink,
    ),
    (Modifier::REVERSED, Attribute::Reverse, Attribute::NoReverse),
    (Modifier::HIDDEN, Attribute::Hidden, Attribute::NoHidden),
    (
        Modifier::CROSSED_OUT,
        Attribute::CrossedOut,
        Attribute::NotCrossedOut,
    ),
];

/// Opens an OSC 8 hyperlink: every glyph written until the close sequence
/// is clickable as `uri`.
fn hyperlink_open(uri: &Hyperlink) -> String {
    format!("\x1b]8;;{}\x1b\\", uri.as_str())
}

/// Closes the open OSC 8 hyperlink.
const HYPERLINK_CLOSE: &str = "\x1b]8;;\x1b\\";

/// A [`Backend`] writing ANSI escape sequences through crossterm to any
/// [`Write`] target.
///
/// # Examples
///
/// ```
/// use tuvix_backend_crossterm::CrosstermBackend;
///
/// // Any `Write` target works; a `Vec<u8>` captures the ANSI output for tests.
/// let backend = CrosstermBackend::new(Vec::<u8>::new());
/// ```
#[derive(Debug)]
pub struct CrosstermBackend<W: Write> {
    writer: W,
    last_fg: Option<Color>,
    last_bg: Option<Color>,
    last_underline: Option<Color>,
    last_modifier: Modifier,
    current_position: Option<Position>,
    /// The hyperlink the terminal's next written glyph will carry, memoized
    /// across frames (OSC 8 state is sticky until closed).
    current_hyperlink: Option<Hyperlink>,
    /// Whether the memoized SGR state is believed to match the terminal.
    /// `false` after a clear (and initially): the next styled cell must
    /// re-anchor the state before deltas can be trusted.
    raster_valid: bool,
}

impl<W: Write> CrosstermBackend<W> {
    /// Creates a backend writing to `writer`.
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            last_fg: None,
            last_bg: None,
            last_underline: None,
            last_modifier: Modifier::empty(),
            current_position: None,
            current_hyperlink: None,
            raster_valid: false,
        }
    }

    /// Returns the wrapped writer, consuming the backend.
    pub fn into_writer(self) -> W {
        self.writer
    }

    /// Emits SGR sequences to transition from the memoized state to `cell`'s.
    fn update_style(&mut self, cell: &Cell) -> std::io::Result<()> {
        if cell.style.fg != self.last_fg {
            self.last_fg = cell.style.fg;
            queue!(
                self.writer,
                SetForegroundColor(to_crossterm_color(self.last_fg.unwrap_or(Color::Reset)))
            )?;
        }
        if cell.style.bg != self.last_bg {
            self.last_bg = cell.style.bg;
            queue!(
                self.writer,
                SetBackgroundColor(to_crossterm_color(self.last_bg.unwrap_or(Color::Reset)))
            )?;
        }
        if cell.style.underline_color != self.last_underline {
            self.last_underline = cell.style.underline_color;
            queue!(
                self.writer,
                SetUnderlineColor(to_crossterm_color(
                    self.last_underline.unwrap_or(Color::Reset)
                ))
            )?;
        }
        let modifier = cell
            .style
            .add_modifier
            .difference(cell.style.remove_modifier);
        if modifier != self.last_modifier {
            for (bit, on, off) in MODIFIER_ATTRIBUTES {
                let wanted = modifier.contains(*bit);
                let applied = self.last_modifier.contains(*bit);
                if wanted != applied {
                    queue!(self.writer, SetAttribute(if wanted { *on } else { *off }))?;
                }
            }
            self.last_modifier = modifier;
        }
        Ok(())
    }

    /// Emits OSC 8 sequences to transition from the memoized hyperlink to
    /// `cell`'s. OSC 8 state is sticky: it survives between cells and
    /// frames and is only re-synchronized when it actually changes.
    fn update_hyperlink(&mut self, cell: &Cell) -> std::io::Result<()> {
        if cell.hyperlink != self.current_hyperlink {
            if self.current_hyperlink.is_some() {
                queue!(self.writer, Print(HYPERLINK_CLOSE))?;
            }
            if let Some(uri) = &cell.hyperlink {
                queue!(self.writer, Print(hyperlink_open(uri)))?;
            }
            self.current_hyperlink.clone_from(&cell.hyperlink);
        }
        Ok(())
    }
}

impl<W: Write> Backend for CrosstermBackend<W> {
    type Error = std::io::Error;

    fn draw<'a>(
        &mut self,
        content: impl IntoIterator<Item = DiffRun<'a>>,
    ) -> Result<(), Self::Error> {
        for run in content {
            for (index, cell) in run.cells.iter().enumerate() {
                let position = Position::new(run.start + index as u16, run.row);
                // Covered cells are invisible; just keep the virtual cursor in
                // sync so no extra move is needed.
                if cell.skip {
                    self.current_position = Some(Position::new(position.x + 1, position.y));
                    continue;
                }
                if self.current_position != Some(position) {
                    // Cursor economy: same-row jumps address the column only.
                    let same_row = self
                        .current_position
                        .is_some_and(|current| current.y == position.y);
                    if same_row {
                        queue!(self.writer, MoveToColumn(position.x))?;
                    } else {
                        queue!(self.writer, MoveTo(position.x, position.y))?;
                    }
                    self.current_position = Some(position);
                }
                if !self.raster_valid {
                    // Re-anchor the memoized state after an invalidation;
                    // the delta logic below then emits only real changes.
                    queue!(self.writer, ResetColor)?;
                    self.last_fg = None;
                    self.last_bg = None;
                    self.last_underline = None;
                    self.last_modifier = Modifier::empty();
                    self.current_hyperlink = None;
                    self.raster_valid = true;
                }
                self.update_style(cell)?;
                self.update_hyperlink(cell)?;
                queue!(self.writer, Print(&cell.symbol))?;
                // The terminal advanced the cursor by the glyph width.
                self.current_position = Some(Position::new(
                    position.x + cell.symbol_width() as u16,
                    position.y,
                ));
            }
            if run.elide_rest {
                // EL erases with the current background color; reset
                // attributes first so the erased tail is default blank.
                if self.raster_valid {
                    queue!(self.writer, ResetColor)?;
                    self.last_fg = None;
                    self.last_bg = None;
                    self.last_underline = None;
                    self.last_modifier = Modifier::empty();
                }
                // The erased tail must not carry the previous cell's link.
                if self.current_hyperlink.is_some() {
                    queue!(self.writer, Print(HYPERLINK_CLOSE))?;
                    self.current_hyperlink = None;
                }
                queue!(self.writer, Clear(ClearType::UntilNewLine))?;
                let written = run.start + u16::try_from(run.cells.len()).unwrap_or(u16::MAX);
                self.current_position = Some(Position::new(written, run.row));
            }
        }
        // Raster state persists across frames by design: no trailing
        // ResetColor. `Backend::clear` invalidates it.
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.writer.flush()
    }

    fn size(&self) -> Result<Size, Self::Error> {
        let (width, height) = crossterm::terminal::size()?;
        Ok(Size::new(width, height))
    }

    fn get_cursor_position(&self) -> Result<Position, Self::Error> {
        let (column, row) = crossterm::cursor::position()?;
        Ok(Position::new(column, row))
    }

    fn set_cursor_position(&mut self, position: Position) -> Result<(), Self::Error> {
        queue!(self.writer, MoveTo(position.x, position.y))?;
        self.current_position = Some(position);
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        queue!(self.writer, Hide)?;
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        queue!(self.writer, Show)?;
        Ok(())
    }

    fn clear(&mut self, region: ClearRegion) -> Result<(), Self::Error> {
        let clear_type = match region {
            ClearRegion::All => ClearType::All,
            ClearRegion::AfterCursor => ClearType::FromCursorDown,
            ClearRegion::BeforeCursor => ClearType::FromCursorUp,
            ClearRegion::CurrentLine => ClearType::CurrentLine,
            ClearRegion::UntilNewLine => ClearType::UntilNewLine,
        };
        queue!(self.writer, Clear(clear_type))?;
        self.current_position = None;
        self.current_hyperlink = None;
        self.raster_valid = false;
        Ok(())
    }

    fn write_control(&mut self, sequence: &str) -> Result<(), Self::Error> {
        // Mode toggles (kitty push/pop, DEC private modes) do not move the
        // cursor and do not affect SGR state, but they are written verbatim;
        // dropping the memoized position makes the next draw re-anchor
        // defensively in case a terminal interpreted them differently.
        queue!(self.writer, Print(sequence))?;
        self.current_position = None;
        Ok(())
    }
    fn set_clipboard(&mut self, contents: &str) -> Result<(), Self::Error> {
        // The same raw OSC 52 sequence the ANSI backend writes; no
        // terminal moves the cursor for it, so the memoized state holds.
        let sequence = encode_osc52(ClipboardSelection::System, contents).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
        })?;
        queue!(self.writer, Print(sequence))?;
        Ok(())
    }
}

/// Public re-exports of the color mapping helpers for backend consumers.
pub use crate::event::{
    to_crossterm_color as map_color_to_crossterm, to_tuvix_color as map_color_to_tuvix,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::to_tuvix_color;
    use crossterm::style::Colored;

    /// Force color output even when NO_COLOR is set in the environment
    /// (crossterm honors no-color.org semantics globally).
    fn force_ansi_colors() {
        Colored::set_ansi_color_disabled(false);
    }

    /// Renders through the backend and returns the captured ANSI output.
    fn ansi(backend: CrosstermBackend<Vec<u8>>) -> String {
        String::from_utf8(backend.into_writer()).unwrap()
    }

    fn cell(symbol: &str) -> Cell {
        let mut cell = Cell::empty();
        cell.set_symbol(symbol);
        cell
    }

    fn run_of(cells: &[Cell], start: u16, row: u16) -> String {
        let mut backend = CrosstermBackend::new(Vec::<u8>::new());
        let run = DiffRun {
            row,
            start,
            cells,
            elide_rest: false,
        };
        backend.draw([run]).unwrap();
        ansi(backend)
    }

    #[test]
    fn draw_moves_cursor_and_prints_symbols() {
        let output = run_of(&[cell("a"), cell("b")], 0, 0);
        assert!(output.contains("\x1b[1;1H"), "move missing: {output:?}");
        assert!(output.contains('a'), "unexpected: {output:?}");
        assert!(output.contains('b'), "unexpected: {output:?}");
        // First draw re-anchors the raster state with a reset.
        assert!(output.contains("\x1b[0m"), "re-anchor missing: {output:?}");
    }

    #[test]
    fn contiguous_cells_do_not_repeat_move_to() {
        let output = run_of(&[cell("a"), cell("b"), cell("c")], 0, 2);
        assert!(
            output
                .matches("\x1b[")
                .filter(|seq| seq.starts_with("\x1b["))
                .count()
                >= 1
        );
        assert_eq!(
            output.matches('H').count(),
            1,
            "one MoveTo per run, got: {output:?}"
        );
    }

    #[test]
    fn styled_cells_emit_sgr_sequences() {
        force_ansi_colors();
        let mut styled = cell("x");
        styled.style = styled.style.fg(Color::Red).add_modifier(Modifier::BOLD);
        let output = run_of(&[styled], 0, 0);
        assert!(output.contains("\x1b[38;5;1m"), "unexpected: {output:?}");
        assert!(output.contains("\x1b[1m"), "unexpected: {output:?}");
    }

    #[test]
    fn same_style_does_not_repeat_sgr() {
        force_ansi_colors();
        let mut styled = cell("x");
        styled.style = styled.style.fg(Color::Red);
        let output = run_of(&[styled.clone(), styled], 0, 0);
        assert_eq!(output.matches("\x1b[38;5;1m").count(), 1, "got: {output:?}");
    }

    #[test]
    fn skip_cells_advance_without_printing() {
        force_ansi_colors();
        let leading = cell("汉");
        let mut trailing = Cell::empty();
        trailing.skip = true;
        let mut after = cell("z");
        after.style = after.style.fg(Color::Red);
        let output = run_of(&[leading, trailing, after], 0, 0);
        // No MoveTo between 汉 and z: the virtual cursor advanced over the
        // covered cell.
        assert_eq!(output.matches('H').count(), 1, "got: {output:?}");
        assert!(output.contains('z'), "got: {output:?}");
    }

    #[test]
    fn color_mapping_round_trips_through_both_directions() {
        for color in [
            Color::Reset,
            Color::Black,
            Color::Gray,
            Color::DarkGray,
            Color::Red,
            Color::LightRed,
            Color::Rgb { r: 1, g: 2, b: 3 },
            Color::Indexed(42),
        ] {
            let mapped = to_crossterm_color(color);
            assert_eq!(
                to_tuvix_color(mapped),
                color,
                "round trip failed for {color:?}"
            );
        }
    }

    #[test]
    fn cursor_commands_queue_ansi() {
        let mut backend = CrosstermBackend::new(Vec::<u8>::new());
        backend.hide_cursor().unwrap();
        backend.set_cursor_position(Position::new(3, 4)).unwrap();
        backend.show_cursor().unwrap();
        let output = ansi(backend);
        assert!(output.contains("\x1b[?25l"), "got: {output:?}");
        assert!(output.contains("\x1b[5;4H"), "got: {output:?}");
        assert!(output.contains("\x1b[?25h"), "got: {output:?}");
    }

    #[test]
    fn clear_region_maps_to_ansi() {
        let mut backend = CrosstermBackend::new(Vec::<u8>::new());
        backend.clear(ClearRegion::All).unwrap();
        backend.clear(ClearRegion::UntilNewLine).unwrap();
        let output = ansi(backend);
        assert!(output.contains("\x1b[2J"), "got: {output:?}");
        assert!(output.contains("\x1b[K"), "got: {output:?}");
    }

    #[test]
    fn write_control_writes_raw_sequences_verbatim() {
        let mut backend = CrosstermBackend::new(Vec::<u8>::new());
        backend.write_control("\x1b[>3u").unwrap();
        backend.write_control("\x1b[?2004h").unwrap();
        let output = ansi(backend);
        assert!(output.contains("\x1b[>3u"), "got: {output:?}");
        assert!(output.contains("\x1b[?2004h"), "got: {output:?}");
    }

    #[test]
    fn write_control_forces_the_next_draw_to_reanchor() {
        let mut backend = CrosstermBackend::new(Vec::<u8>::new());
        backend
            .draw([DiffRun {
                row: 0,
                start: 0,
                cells: &[cell("a")],
                elide_rest: false,
            }])
            .unwrap();
        backend.write_control("\x1b[>3u").unwrap();
        backend
            .draw([DiffRun {
                row: 0,
                start: 1,
                cells: &[cell("b")],
                elide_rest: false,
            }])
            .unwrap();
        let output = ansi(backend);
        // The second draw must not assume the cursor position survived the
        // control sequence: it re-anchors with a full MoveTo.
        assert_eq!(output.matches('H').count(), 2, "got: {output:?}");
    }

    #[test]
    fn hyperlink_cells_emit_osc8_open_and_close() {
        let mut linked = cell("x");
        linked.set_hyperlink(Some(Hyperlink::new("https://tuvix.dev")));
        let mut plain = cell("y");
        plain.set_hyperlink(None);
        let output = run_of(&[linked, plain], 0, 0);
        assert!(
            output.contains("\x1b]8;;https://tuvix.dev\x1b\\"),
            "open missing: {output:?}"
        );
        assert!(
            output.matches("\x1b]8;;\x1b\\").count() >= 1,
            "close missing: {output:?}"
        );
    }

    #[test]
    fn contiguous_same_hyperlink_emits_one_open() {
        let mut a = cell("a");
        a.set_hyperlink(Some(Hyperlink::new("https://tuvix.dev")));
        let b = {
            let mut cell = cell("b");
            cell.set_hyperlink(Some(Hyperlink::new("https://tuvix.dev")));
            cell
        };
        let output = run_of(&[a, b], 0, 0);
        assert_eq!(
            output.matches("\x1b]8;;https://tuvix.dev\x1b\\").count(),
            1,
            "one open per contiguous segment, got: {output:?}"
        );
        // No close until a different link state arrives.
        assert_eq!(
            output.matches("\x1b]8;;\x1b\\").count(),
            0,
            "got: {output:?}"
        );
    }

    #[test]
    fn changing_hyperlink_closes_then_reopens() {
        let mut a = cell("a");
        a.set_hyperlink(Some(Hyperlink::new("https://a.dev")));
        let mut b = cell("b");
        b.set_hyperlink(Some(Hyperlink::new("https://b.dev")));
        let output = run_of(&[a, b], 0, 0);
        let open_a = "\x1b]8;;https://a.dev\x1b\\";
        let close = "\x1b]8;;\x1b\\";
        let open_b = "\x1b]8;;https://b.dev\x1b\\";
        let (Some(at_a), Some(at_close), Some(at_b)) =
            (output.find(open_a), output.find(close), output.find(open_b))
        else {
            panic!("a transition sequence is missing: {output:?}");
        };
        assert!(at_a < at_close && at_close < at_b, "ordering: {output:?}");
    }

    #[test]
    fn elide_closes_an_open_hyperlink() {
        let mut backend = CrosstermBackend::new(Vec::<u8>::new());
        let mut linked = cell("a");
        linked.set_hyperlink(Some(Hyperlink::new("https://tuvix.dev")));
        backend
            .draw([DiffRun {
                row: 0,
                start: 0,
                cells: &[linked],
                elide_rest: true,
            }])
            .unwrap();
        let output = ansi(backend);
        let close = "\x1b]8;;\x1b\\";
        let el = "\x1b[K";
        assert!(
            output.contains(&format!("{close}{el}")),
            "close must precede EL: {output:?}"
        );
    }

    #[test]
    fn clear_invalidates_the_memoized_hyperlink() {
        let mut backend = CrosstermBackend::new(Vec::<u8>::new());
        let mut linked = cell("a");
        linked.set_hyperlink(Some(Hyperlink::new("https://tuvix.dev")));
        backend
            .draw([DiffRun {
                row: 0,
                start: 0,
                cells: &[linked],
                elide_rest: false,
            }])
            .unwrap();
        backend.clear(ClearRegion::All).unwrap();
        // A fresh run with the same link must re-open it: the clear wiped
        // the terminal's link state along with the screen.
        let mut linked = cell("b");
        linked.set_hyperlink(Some(Hyperlink::new("https://tuvix.dev")));
        backend
            .draw([DiffRun {
                row: 0,
                start: 0,
                cells: &[linked],
                elide_rest: false,
            }])
            .unwrap();
        let output = ansi(backend);
        assert_eq!(
            output.matches("\x1b]8;;https://tuvix.dev\x1b\\").count(),
            2,
            "link must re-open after clear: {output:?}"
        );
    }
}
