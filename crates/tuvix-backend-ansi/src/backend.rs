//! The raw-ANSI [`Backend`]: the persistent raster-state machine writing
//! straight to a [`Write`] target.
//!
//! The emission rules are the M1 byte-savers, unchanged from the crossterm
//! reference implementation:
//!
//! 1. **SGR persistence** — colors and attributes are memoized *across
//!    frames*; no trailing reset after a draw. [`Backend::clear`]
//!    invalidates the memo so the next draw re-anchors with one SGR reset.
//! 2. **Cursor economy** — same-row jumps emit column addressing (CHA,
//!    one parameter); continuing runs emit nothing.
//! 3. **EL elision** — runs marked `elide_rest` erase the row tail with
//!    one `EL` control instead of writing blank cells.
//! 4. **OSC 8 hyperlinks** — the open/close transition of a cell's
//!    hyperlink is emitted only when the link state actually changes; EL
//!    tails close an open link first.

use std::io::Write;

use tuvix_core::backend::{Backend, ClearRegion};
use tuvix_core::buffer::{Cell, DiffRun};
use tuvix_core::clipboard::{ClipboardSelection, encode_osc52};
use tuvix_core::geometry::{Position, Size};
use tuvix_core::style::{Color, Hyperlink, Modifier};

/// SGR parameters for a color role: `role` 0 = foreground (39 base),
/// 1 = background (49), 2 = underline (59).
///
/// Named colors are valid SGR codes only for fg/bg (30-37, 90-107); the
/// underline role selects them through the 256-palette extended selector
/// (`58;5;0-15`), matching crossterm's `SetUnderlineColor`.
fn color_params(color: Color, role: u8) -> String {
    let base_default = 39 + u16::from(role) * 10;
    let base = 30 + u16::from(role) * 10;
    let bright = 90 + u16::from(role) * 10;
    // The extended-color selector per role: 38 (fg), 48 (bg), 58 (underline).
    let set = match role {
        2 => 58,
        1 => 48,
        _ => 38,
    };
    match color {
        Color::Reset => base_default.to_string(),
        Color::Indexed(index) => format!("{set};5;{index}"),
        Color::Rgb { r, g, b } => format!("{set};2;{r};{g};{b}"),
        Color::Black => named(base, bright, set, role, 0),
        Color::Red => named(base, bright, set, role, 1),
        Color::Green => named(base, bright, set, role, 2),
        Color::Yellow => named(base, bright, set, role, 3),
        Color::Blue => named(base, bright, set, role, 4),
        Color::Magenta => named(base, bright, set, role, 5),
        Color::Cyan => named(base, bright, set, role, 6),
        Color::Gray => named(base, bright, set, role, 7),
        Color::DarkGray => named(base, bright, set, role, 8),
        Color::LightRed => named(base, bright, set, role, 9),
        Color::LightGreen => named(base, bright, set, role, 10),
        Color::LightYellow => named(base, bright, set, role, 11),
        Color::LightBlue => named(base, bright, set, role, 12),
        Color::LightMagenta => named(base, bright, set, role, 13),
        Color::LightCyan => named(base, bright, set, role, 14),
        Color::White => named(base, bright, set, role, 15),
    }
}

/// SGR parameters for one of the 16 named colors: the classic 30-37 /
/// 90-107 codes for fg/bg, `58;5;<palette index>` for underline.
fn named(base: u16, bright: u16, set: u16, role: u8, index: u16) -> String {
    match role {
        2 => format!("{set};5;{index}"),
        _ if index < 8 => (base + index).to_string(),
        _ => (bright + index - 8).to_string(),
    }
}

/// Map of tuvix modifier bits to their SGR on/off parameters.
const MODIFIER_PARAMS: &[(Modifier, &str, &str)] = &[
    (Modifier::BOLD, "1", "22"),
    (Modifier::DIM, "2", "22"),
    (Modifier::ITALIC, "3", "23"),
    (Modifier::UNDERLINED, "4", "24"),
    (Modifier::SLOW_BLINK, "5", "25"),
    (Modifier::RAPID_BLINK, "6", "25"),
    (Modifier::REVERSED, "7", "27"),
    (Modifier::HIDDEN, "8", "28"),
    (Modifier::CROSSED_OUT, "9", "29"),
];

/// The ANSI control prefix: cursor position (`ESC[<row>;<col>H`).
fn cup(position: Position) -> String {
    format!("\x1b[{};{}H", position.y + 1, position.x + 1)
}

/// Same-row cursor addressing: column only (`ESC[<col>G`).
fn cha(column: u16) -> String {
    format!("\x1b[{}G", column + 1)
}

/// Erase to end of line (`ESC[K`).
const EL: &str = "\x1b[K";

/// SGR reset (`ESC[0m`): re-anchors the memoized raster state.
const SGR_RESET: &str = "\x1b[0m";

/// Opens an OSC 8 hyperlink: every glyph written until the close sequence
/// is clickable as `uri`.
fn hyperlink_open(uri: &Hyperlink) -> String {
    format!("\x1b]8;;{}\x1b\\", uri.as_str())
}

/// Closes the open OSC 8 hyperlink.
const HYPERLINK_CLOSE: &str = "\x1b]8;;\x1b\\";

/// The clear-region ANSI controls, matching ECMA-48 / DEC private modes.
fn clear_sequence(region: ClearRegion) -> &'static str {
    match region {
        ClearRegion::All => "\x1b[2J",
        ClearRegion::AfterCursor => "\x1b[J",
        ClearRegion::BeforeCursor => "\x1b[1J",
        ClearRegion::CurrentLine => "\x1b[2K",
        ClearRegion::UntilNewLine => "\x1b[K",
    }
}

/// A [`Backend`] emitting raw ANSI escape sequences to any [`Write`]
/// target — no terminal library required.
///
/// This is the reference "simple" backend: byte-for-byte the same raster
/// discipline as [`CrosstermBackend`](https://docs.rs/tuvix-backend-crossterm)
/// (persistent SGR, cursor economy, EL elision, OSC 8 links), but
/// self-contained. It does not touch terminal modes and cannot query the
/// terminal: the caller owns the size (update it with
/// [`AnsiBackend::resize`]) and cursor positions are virtual.
///
/// # Examples
///
/// ```
/// use tuvix_backend_ansi::AnsiBackend;
/// use tuvix_core::backend::Backend;
/// use tuvix_core::geometry::Size;
///
/// let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(10, 1));
/// backend.hide_cursor().unwrap();
/// assert_eq!(backend.into_writer(), b"\x1b[?25l".as_slice());
/// ```
#[derive(Debug)]
pub struct AnsiBackend<W: Write> {
    writer: W,
    size: Size,
    hidden_cursor: bool,
    last_fg: Option<Color>,
    last_bg: Option<Color>,
    last_underline: Option<Color>,
    last_modifier: Modifier,
    current_position: Option<Position>,
    /// The hyperlink the terminal's next written glyph will carry,
    /// memoized across frames (OSC 8 state is sticky until closed).
    current_hyperlink: Option<Hyperlink>,
    /// Whether the memoized SGR state is believed to match the terminal.
    raster_valid: bool,
}

impl<W: Write> AnsiBackend<W> {
    /// Creates a backend writing to `writer`, believing the terminal is
    /// `size` cells large.
    pub fn new(writer: W, size: Size) -> Self {
        Self {
            writer,
            size,
            hidden_cursor: false,
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

    /// Tells the backend the terminal is now `size` cells large. The host
    /// calls this on resize (there is no TTY to ask).
    pub fn resize(&mut self, size: Size) {
        self.size = size;
    }

    /// Emits SGR sequences to transition from the memoized state to
    /// `cell`'s.
    fn update_style(&mut self, cell: &Cell) -> std::io::Result<()> {
        if cell.style.fg != self.last_fg {
            self.last_fg = cell.style.fg;
            self.sgr(&color_params(self.last_fg.unwrap_or(Color::Reset), 0))?;
        }
        if cell.style.bg != self.last_bg {
            self.last_bg = cell.style.bg;
            self.sgr(&color_params(self.last_bg.unwrap_or(Color::Reset), 1))?;
        }
        if cell.style.underline_color != self.last_underline {
            self.last_underline = cell.style.underline_color;
            self.sgr(&color_params(
                self.last_underline.unwrap_or(Color::Reset),
                2,
            ))?;
        }
        let modifier = cell
            .style
            .add_modifier
            .difference(cell.style.remove_modifier);
        if modifier != self.last_modifier {
            for (bit, on, off) in MODIFIER_PARAMS {
                let wanted = modifier.contains(*bit);
                let applied = self.last_modifier.contains(*bit);
                if wanted != applied {
                    self.sgr(if wanted { on } else { off })?;
                }
            }
            self.last_modifier = modifier;
        }
        Ok(())
    }

    /// Emits OSC 8 sequences to transition from the memoized hyperlink to
    /// `cell`'s.
    fn update_hyperlink(&mut self, cell: &Cell) -> std::io::Result<()> {
        if cell.hyperlink != self.current_hyperlink {
            if self.current_hyperlink.is_some() {
                self.write(HYPERLINK_CLOSE)?;
            }
            if let Some(uri) = &cell.hyperlink {
                self.write(&hyperlink_open(uri))?;
            }
            self.current_hyperlink.clone_from(&cell.hyperlink);
        }
        Ok(())
    }

    /// Writes one SGR sequence (`ESC[<params>m`).
    fn sgr(&mut self, params: &str) -> std::io::Result<()> {
        self.write("\x1b[")
            .and_then(|()| self.write(params))
            .and_then(|()| self.write("m"))
    }

    /// Writes a string to the terminal.
    fn write(&mut self, text: &str) -> std::io::Result<()> {
        self.writer.write_all(text.as_bytes())
    }
}

impl<W: Write> Backend for AnsiBackend<W> {
    type Error = std::io::Error;

    fn draw<'a>(
        &mut self,
        content: impl IntoIterator<Item = DiffRun<'a>>,
    ) -> Result<(), Self::Error> {
        for run in content {
            for (index, cell) in run.cells.iter().enumerate() {
                let position = Position::new(run.start + index as u16, run.row);
                // Covered cells are invisible; just keep the virtual cursor
                // in sync so no extra move is needed.
                if cell.skip {
                    self.current_position = Some(Position::new(position.x + 1, position.y));
                    continue;
                }
                if self.current_position != Some(position) {
                    // Cursor economy: same-row jumps address the column only.
                    let same_row = self
                        .current_position
                        .is_some_and(|current| current.y == position.y);
                    let sequence = if same_row {
                        cha(position.x)
                    } else {
                        cup(position)
                    };
                    self.write(&sequence)?;
                    self.current_position = Some(position);
                }
                if !self.raster_valid {
                    // Re-anchor the memoized state after an invalidation;
                    // the delta logic below then emits only real changes.
                    self.write(SGR_RESET)?;
                    self.last_fg = None;
                    self.last_bg = None;
                    self.last_underline = None;
                    self.last_modifier = Modifier::empty();
                    self.current_hyperlink = None;
                    self.raster_valid = true;
                }
                self.update_style(cell)?;
                self.update_hyperlink(cell)?;
                self.write(cell.symbol.as_str())?;
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
                    self.write(SGR_RESET)?;
                    self.last_fg = None;
                    self.last_bg = None;
                    self.last_underline = None;
                    self.last_modifier = Modifier::empty();
                }
                // The erased tail must not carry the previous cell's link.
                if self.current_hyperlink.is_some() {
                    self.write(HYPERLINK_CLOSE)?;
                    self.current_hyperlink = None;
                }
                self.write(EL)?;
                let written = run.start + u16::try_from(run.cells.len()).unwrap_or(u16::MAX);
                self.current_position = Some(Position::new(written, run.row));
            }
        }
        // Raster state persists across frames by design: no trailing
        // SGR reset. `Backend::clear` invalidates it.
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.writer.flush()
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(self.size)
    }

    fn get_cursor_position(&self) -> Result<Position, Self::Error> {
        Ok(self.current_position.unwrap_or(Position::new(0, 0)))
    }

    fn set_cursor_position(&mut self, position: Position) -> Result<(), Self::Error> {
        self.write(&cup(position))?;
        self.current_position = Some(position);
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.write("\x1b[?25l")?;
        self.hidden_cursor = true;
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.write("\x1b[?25h")?;
        self.hidden_cursor = false;
        Ok(())
    }

    fn clear(&mut self, region: ClearRegion) -> Result<(), Self::Error> {
        self.write(clear_sequence(region))?;
        self.current_position = None;
        self.current_hyperlink = None;
        self.raster_valid = false;
        Ok(())
    }

    fn write_control(&mut self, sequence: &str) -> Result<(), Self::Error> {
        // Mode toggles are written verbatim; dropping the memoized position
        // makes the next draw re-anchor defensively.
        self.write(sequence)?;
        self.current_position = None;
        Ok(())
    }

    fn set_clipboard(&mut self, contents: &str) -> Result<(), Self::Error> {
        // OSC 52 is a pure side effect: no terminal moves the cursor for
        // it, so the memoized raster state (position and SGR) stays valid.
        let sequence = encode_osc52(ClipboardSelection::System, contents).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
        })?;
        self.write(&sequence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders through a backend and returns the captured ANSI output.
    fn ansi(backend: AnsiBackend<Vec<u8>>) -> String {
        String::from_utf8(backend.into_writer()).unwrap()
    }

    fn cell(symbol: &str) -> Cell {
        let mut cell = Cell::empty();
        cell.set_symbol(symbol);
        cell
    }

    fn run_of(cells: &[Cell], start: u16, row: u16) -> String {
        let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(20, 4));
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
    fn new_backend_is_virtual_with_stored_size() {
        let backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(11, 3));
        assert_eq!(backend.size().unwrap(), Size::new(11, 3));
        assert_eq!(backend.get_cursor_position().unwrap(), Position::new(0, 0));
        assert!(!backend.hidden_cursor);
    }

    #[test]
    fn draw_moves_cursor_and_prints_symbols() {
        let output = run_of(&[cell("a"), cell("b")], 0, 0);
        assert_eq!(output, "\x1b[1;1H\x1b[0mab");
    }

    #[test]
    fn same_row_jumps_use_cha() {
        let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(20, 4));
        backend
            .draw([DiffRun {
                row: 0,
                start: 0,
                cells: &[cell("a")],
                elide_rest: false,
            }])
            .unwrap();
        backend
            .draw([DiffRun {
                row: 0,
                start: 5,
                cells: &[cell("b")],
                elide_rest: false,
            }])
            .unwrap();
        let output = ansi(backend);
        assert!(
            output.contains("\x1b[6G"),
            "expected CHA for the same-row jump: {output:?}"
        );
        assert_eq!(output.matches('H').count(), 1, "one CUP total");
    }

    #[test]
    fn row_changes_use_cup() {
        let output = run_of(&[cell("x")], 3, 2);
        assert!(output.starts_with("\x1b[3;4H"), "got: {output:?}");
    }

    #[test]
    fn styled_cells_emit_sgr_deltas() {
        let mut styled = cell("x");
        styled.style = styled.style.fg(Color::Red).add_modifier(Modifier::BOLD);
        let output = run_of(&[styled], 0, 0);
        assert!(output.contains("\x1b[31m"), "fg red: {output:?}");
        assert!(output.contains("\x1b[1m"), "bold: {output:?}");
    }

    #[test]
    fn indexed_rgb_and_underline_colors() {
        let mut a = cell("a");
        a.style = a.style.fg(Color::Indexed(196));
        let mut b = cell("b");
        b.style = b.style.bg(Color::Rgb { r: 1, g: 2, b: 3 });
        let mut c = cell("c");
        c.style = c.style.underline_color(Color::Indexed(9));
        let output = run_of(&[a, b, c], 0, 0);
        assert!(output.contains("\x1b[38;5;196m"), "{output:?}");
        assert!(output.contains("\x1b[48;2;1;2;3m"), "{output:?}");
        assert!(output.contains("\x1b[58;5;9m"), "{output:?}");
    }

    #[test]
    fn bright_colors_use_the_90_107_range() {
        let mut styled = cell("x");
        styled.style = styled.style.fg(Color::DarkGray).bg(Color::White);
        let output = run_of(&[styled], 0, 0);
        assert!(output.contains("\x1b[90m"), "{output:?}");
        assert!(output.contains("\x1b[107m"), "{output:?}");
    }

    #[test]
    fn sgr_state_persists_across_draws() {
        fn run(cells: &[Cell]) -> DiffRun<'_> {
            DiffRun {
                row: 0,
                start: 0,
                cells,
                elide_rest: false,
            }
        }
        let mut styled = cell("x");
        styled.style = styled.style.fg(Color::Red);
        let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(20, 4));
        backend.draw([run(&[styled.clone()])]).unwrap();
        backend.draw([run(&[styled])]).unwrap();
        let output = ansi(backend);
        assert_eq!(
            output.matches("\x1b[31m").count(),
            1,
            "SGR must be memoized across frames: {output:?}"
        );
    }

    #[test]
    fn first_draw_reanchors_with_sgr_reset() {
        let output = run_of(&[cell("a")], 0, 0);
        assert!(output.contains(SGR_RESET), "got: {output:?}");
    }

    #[test]
    fn clear_invalidates_the_raster_state() {
        let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(20, 4));
        backend
            .draw([DiffRun {
                row: 0,
                start: 0,
                cells: &[cell("a")],
                elide_rest: false,
            }])
            .unwrap();
        backend.clear(ClearRegion::All).unwrap();
        backend
            .draw([DiffRun {
                row: 0,
                start: 0,
                cells: &[cell("b")],
                elide_rest: false,
            }])
            .unwrap();
        let output = ansi(backend);
        // Two SGR resets: the initial re-anchor and the post-clear one.
        assert_eq!(output.matches(SGR_RESET).count(), 2, "got: {output:?}");
    }

    #[test]
    fn clear_region_maps_to_ansi() {
        let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(20, 4));
        backend.clear(ClearRegion::AfterCursor).unwrap();
        backend.clear(ClearRegion::BeforeCursor).unwrap();
        backend.clear(ClearRegion::CurrentLine).unwrap();
        backend.clear(ClearRegion::UntilNewLine).unwrap();
        let output = ansi(backend);
        assert!(output.contains("\x1b[J"), "{output:?}");
        assert!(output.contains("\x1b[1J"), "{output:?}");
        assert!(output.contains("\x1b[2K"), "{output:?}");
        assert!(output.contains("\x1b[K"), "{output:?}");
    }

    #[test]
    fn elide_rest_writes_one_el_after_a_reset() {
        let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(20, 4));
        backend
            .draw([DiffRun {
                row: 0,
                start: 0,
                cells: &[cell("a"), cell("b")],
                elide_rest: true,
            }])
            .unwrap();
        let output = ansi(backend);
        assert!(output.contains("ab\x1b[0m\x1b[K"), "got: {output:?}");
    }

    #[test]
    fn skip_cells_advance_without_printing() {
        let leading = cell("汉");
        let mut trailing = Cell::empty();
        trailing.skip = true;
        let output = run_of(&[leading, trailing], 0, 0);
        assert_eq!(output.matches("汉").count(), 1, "{output:?}");
        // No second cursor move between the pair.
        assert_eq!(output.matches('H').count(), 1, "{output:?}");
    }

    #[test]
    fn cursor_commands_write_the_expected_ansi() {
        let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(20, 4));
        backend.hide_cursor().unwrap();
        backend.set_cursor_position(Position::new(3, 4)).unwrap();
        assert_eq!(backend.get_cursor_position().unwrap(), Position::new(3, 4));
        backend.show_cursor().unwrap();
        let output = ansi(backend);
        assert!(output.contains("\x1b[?25l"), "{output:?}");
        assert!(output.contains("\x1b[5;4H"), "{output:?}");
        assert!(output.contains("\x1b[?25h"), "{output:?}");
    }

    #[test]
    fn resize_updates_the_stored_size() {
        let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(10, 2));
        backend.resize(Size::new(6, 1));
        assert_eq!(backend.size().unwrap(), Size::new(6, 1));
    }

    #[test]
    fn write_control_writes_verbatim_and_forces_reanchor() {
        let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(20, 4));
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
        assert!(output.contains("\x1b[>3u"), "{output:?}");
        // The second draw re-anchors the cursor (CUP, not CHA) because the
        // control sequence may have moved the real terminal cursor.
        assert_eq!(output.matches('H').count(), 2, "got: {output:?}");
    }

    #[test]
    fn hyperlink_cells_emit_osc8_transitions() {
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
            output.contains("\x1b]8;;\x1b\\"),
            "close missing: {output:?}"
        );
    }

    #[test]
    fn contiguous_same_hyperlink_emits_one_open() {
        let mut a = cell("a");
        a.set_hyperlink(Some(Hyperlink::new("https://tuvix.dev")));
        let mut b = cell("b");
        b.set_hyperlink(Some(Hyperlink::new("https://tuvix.dev")));
        let output = run_of(&[a, b], 0, 0);
        assert_eq!(
            output.matches("\x1b]8;;https://tuvix.dev\x1b\\").count(),
            1,
            "one open per contiguous segment: {output:?}"
        );
        assert_eq!(
            output.matches("\x1b]8;;\x1b\\").count(),
            0,
            "no close while the link continues: {output:?}"
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
        let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(20, 4));
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
        assert!(
            output.contains("\x1b]8;;\x1b\\\x1b[K"),
            "close must precede EL: {output:?}"
        );
    }

    #[test]
    fn clear_invalidates_the_memoized_hyperlink() {
        let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(20, 4));
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

    #[test]
    fn wide_symbols_advance_the_virtual_cursor_by_two() {
        let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(20, 4));
        backend
            .draw([DiffRun {
                row: 0,
                start: 0,
                cells: &[cell("汉")],
                elide_rest: false,
            }])
            .unwrap();
        backend
            .draw([DiffRun {
                row: 0,
                start: 2,
                cells: &[cell("b")],
                elide_rest: false,
            }])
            .unwrap();
        let output = ansi(backend);
        // The virtual cursor already sits at column 2 after the wide glyph:
        // the second run continues without any cursor addressing at all.
        assert_eq!(
            output.matches('H').count(),
            1,
            "one CUP for the whole frame: {output:?}"
        );
        assert!(
            !output.contains('G'),
            "no CHA between the wide pair and the next cell: {output:?}"
        );
    }
}
