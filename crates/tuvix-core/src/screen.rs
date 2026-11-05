//! Screen reconstruction: parse a canonical ANSI snapshot back into a
//! cell grid (M9, architecture §5.8).
//!
//! [`serialize_ansi`] snapshots are the
//! language every tuvix frame speaks — preview taps, the M8
//! [`HistorySink`](crate::preview::HistorySink) ring and the M9
//! [`SessionRecorder`](crate::session::SessionRecorder) all store them.
//! [`Screen::from_ansi`] parses that canonical format back into cells,
//! which turns a recorded frame into a first-class object again:
//! reconstruct a [`Buffer`] with [`Screen::to_buffer`], diff two frames
//! with [`Screen::diff`], or overlay a historical frame on a live app
//! (the `FrameInspector` widget).
//!
//! # The accepted vocabulary
//!
//! The parser accepts exactly what `serialize_ansi` emits — the
//! canonical format, nothing more — and rejects everything else with a
//! byte-offset [`ParseError`]:
//!
//! - cursor addressing `ESC[<row>;<col>H` (1-based);
//! - SGR runs with the standard parameters (named, indexed and RGB
//!   colors, the nine modifier bits, and the off codes `22`-`29`);
//! - OSC 8 hyperlinks `ESC]8;;URI ST` (empty URI closes);
//! - erase-to-end-of-line `ESC[K` — everything after it in the row is
//!   blank, as in a real terminal;
//! - printable text, segmented per grapheme cluster; a glyph whose
//!   display width exceeds one column covers the trailing cells, exactly
//!   like a live buffer.
//!
//! The screen's dimensions are *discovered*: the height is the highest
//! row addressed, the width the longest written row. A snapshot does not
//! encode the viewport width (a blank tail is one `ESC[K`), so
//! [`Screen::to_buffer`] pastes the grid into whatever area the caller
//! provides, clipping as needed. One documented lossiness: a *blank*
//! cell that carries a style (e.g. a colored background with no glyph)
//! produces no output in the canonical format — a real terminal paints
//! nothing there either — and therefore reconstructs as a default cell.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::buffer::Buffer;
//! use tuvix_core::geometry::Rect;
//! use tuvix_core::preview::serialize_ansi;
//! use tuvix_core::screen::Screen;
//! use tuvix_core::style::{Color, Modifier, Style};
//!
//! let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
//! buffer.set_string(0, 0, "hi", Style::new().fg(Color::Red).add_modifier(Modifier::BOLD));
//! let ansi = serialize_ansi(&buffer);
//!
//! let screen = Screen::from_ansi(&ansi).unwrap();
//! // Dimensions are discovered from the written content.
//! assert_eq!(screen.width(), 2);
//! assert_eq!(screen.height(), 1);
//! let rebuilt = screen.to_buffer(Rect::new(0, 0, 6, 1));
//! assert_eq!(rebuilt, buffer);
//! ```

use std::fmt;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::buffer::{Buffer, Cell, Symbol};
use crate::geometry::Rect;
use crate::preview::serialize_ansi;
use crate::style::{Color, Hyperlink, Modifier, Style};

/// Why a snapshot failed to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseErrorKind {
    /// An escape sequence the canonical format never produces (or a
    /// malformed one). Carries the sequence text for the error message.
    UnknownSequence {
        /// The rejected sequence, control characters escaped.
        sequence: String,
    },
    /// An SGR parameter outside the canonical vocabulary.
    UnknownSgr {
        /// The rejected parameter.
        parameter: u16,
    },
    /// An OSC or CSI sequence cut off by the end of the input.
    Unterminated,
    /// A raw C0 control character in the text stream (the canonical
    /// format only ever contains printable text and escape sequences).
    ControlCharacter {
        /// The rejected character.
        character: char,
    },
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSequence { sequence } => write!(f, "unknown escape sequence {sequence:?}"),
            Self::UnknownSgr { parameter } => write!(f, "unknown SGR parameter {parameter}"),
            Self::Unterminated => f.write_str("unterminated escape sequence"),
            Self::ControlCharacter { character } => {
                write!(f, "control character {character:?} in the text stream")
            }
        }
    }
}

/// A snapshot parse failure: the kind plus the byte offset it was
/// detected at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset into the input where the problem was detected.
    pub at: usize,
    /// What the problem is.
    pub kind: ParseErrorKind,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ANSI snapshot parse error at byte {}: {}",
            self.at, self.kind
        )
    }
}

impl std::error::Error for ParseError {}

/// One reconstructed frame: a discovered-dimensions grid of cells,
/// identical in shape and semantics to a live [`Buffer`]'s content.
///
/// Cells carry the same fields a live cell does (symbol, style,
/// hyperlink, coverage flag), so [`Screen::to_buffer`] output
/// participates in [`Buffer::diff`] and the ordinary rendering pipeline
/// unchanged.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Screen {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
}

impl Screen {
    /// Parses a canonical ANSI snapshot (`serialize_ansi` output) into a
    /// screen.
    ///
    /// # Errors
    ///
    /// Returns a [`ParseError`] when `ansi` contains anything outside
    /// the canonical vocabulary.
    pub fn from_ansi(ansi: &str) -> Result<Self, ParseError> {
        let mut parser = Parser::new(ansi);
        parser.run()?;
        Ok(parser.finish())
    }

    /// An empty screen (zero by zero cells).
    pub fn empty() -> Self {
        Self::default()
    }

    /// The discovered width: the longest written row.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// The discovered height: the highest row addressed.
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Whether nothing was recorded at all (no row was ever addressed).
    pub fn is_empty(&self) -> bool {
        self.height == 0
    }

    /// Whether every cell is blank (default style, no glyph, no link).
    pub fn is_blank(&self) -> bool {
        self.cells.iter().all(|cell| {
            cell.symbol.as_str().is_empty()
                && cell.style == Style::new()
                && cell.hyperlink.is_none()
        })
    }

    /// The cell at `(x, y)`, or `None` when out of bounds.
    pub fn cell(&self, x: u16, y: u16) -> Option<&Cell> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.cells
            .get(y as usize * self.width as usize + x as usize)
    }

    /// Reconstructs the frame as a [`Buffer`] over `area`: the grid is
    /// pasted at the area origin and clipped to it. Cells beyond the
    /// recorded grid stay blank — a snapshot does not encode the
    /// viewport width, only what was written.
    pub fn to_buffer(&self, area: Rect) -> Buffer {
        let mut buffer = Buffer::empty(area);
        let copy_width = self.width.min(area.width);
        let copy_height = self.height.min(area.height);
        for row in 0..copy_height {
            let source = row as usize * self.width as usize;
            let target = row as usize * area.width as usize;
            buffer.content[target..target + copy_width as usize]
                .clone_from_slice(&self.cells[source..source + copy_width as usize]);
        }
        buffer
    }

    /// Re-serializes the screen into the canonical ANSI snapshot of its
    /// own discovered dimensions. For any canonical snapshot this is the
    /// identity (pinned by tests), which makes the format round-trip
    /// safe for golden sessions.
    pub fn to_ansi(&self) -> String {
        serialize_ansi(&self.to_buffer(Rect::new(0, 0, self.width, self.height)))
    }

    /// The cells of `other` that differ from `self`, as `(x, y, cell)`
    /// triples over the union of both screens' dimensions (screens of
    /// different sizes compare as blank-padded to the union). Owned
    /// copies: the result outlives neither screen borrow.
    pub fn diff(&self, other: &Screen) -> Vec<(u16, u16, Cell)> {
        let width = self.width.max(other.width);
        let height = self.height.max(other.height);
        let mut changed = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let blank = Cell::empty();
                let before = self.cell(x, y).unwrap_or(&blank);
                let after = other.cell(x, y).unwrap_or(&blank);
                if before != after {
                    changed.push((x, y, after.clone()));
                }
            }
        }
        changed
    }
}

/// The incremental parser behind [`Screen::from_ansi`]: a cursor, the
/// active SGR state and the growing row grid.
struct Parser<'a> {
    input: &'a str,
    pos: usize,
    cursor_row: u16,
    cursor_col: u16,
    style: Style,
    link: Option<Hyperlink>,
    rows: Vec<Vec<Cell>>,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            cursor_row: 0,
            cursor_col: 0,
            style: Style::new(),
            link: None,
            rows: Vec::new(),
        }
    }

    fn run(&mut self) -> Result<(), ParseError> {
        while self.pos < self.input.len() {
            if self.peek() == Some('\x1b') {
                self.parse_escape()?;
            } else {
                self.parse_text()?;
            }
        }
        Ok(())
    }

    fn finish(self) -> Screen {
        let height = self.rows.len();
        let width = self.rows.iter().map(Vec::len).max().unwrap_or(0);
        let width = width.min(u16::MAX as usize) as u16;
        let mut cells = Vec::with_capacity(height * width as usize);
        for row in &self.rows {
            cells.extend_from_slice(row);
            cells.resize(cells.len() + width as usize - row.len(), Cell::empty());
        }
        Screen {
            width,
            height: height.min(u16::MAX as usize) as u16,
            cells,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn ensure_row(&mut self, row: u16) {
        while self.rows.len() <= row as usize {
            self.rows.push(Vec::new());
        }
    }

    /// Grows `row` to `col` with blank cells and returns the row slice.
    fn row_for_writing(&mut self, row: u16, col: u16) -> &mut Vec<Cell> {
        self.ensure_row(row);
        let row = &mut self.rows[row as usize];
        while row.len() <= col as usize {
            row.push(Cell::empty());
        }
        row
    }

    /// Writes one grapheme (and its covered tail cells) at the cursor.
    fn write_grapheme(&mut self, glyph: &str) {
        let columns = glyph.width().max(1) as u16;
        let (row, col) = (self.cursor_row, self.cursor_col);
        self.row_for_writing(row, col + columns.saturating_sub(1));
        let cells = &mut self.rows[row as usize];
        cells[col as usize] = Cell {
            symbol: Symbol::new(glyph),
            style: self.style,
            hyperlink: self.link.clone(),
            skip: false,
        };
        for covered in 1..columns {
            cells[(col + covered) as usize] = Cell {
                skip: true,
                ..Cell::empty()
            };
        }
        self.cursor_col = col.saturating_add(columns);
    }

    /// Parses a run of printable text into grapheme cells.
    fn parse_text(&mut self) -> Result<(), ParseError> {
        let start = self.pos;
        let run = self.input[start..]
            .char_indices()
            .take_while(|(_, character)| *character != '\x1b' && !character.is_ascii_control())
            .last()
            .map(|(offset, character)| offset + character.len_utf8());
        let Some(end) = run else {
            // The first character is a rejected control character.
            let character = self.peek().unwrap_or('\x00');
            return Err(ParseError {
                at: self.pos,
                kind: ParseErrorKind::ControlCharacter { character },
            });
        };
        let text = &self.input[start..start + end];
        for glyph in text.graphemes(true) {
            self.write_grapheme(glyph);
        }
        self.pos = start + end;
        Ok(())
    }

    /// Dispatches one escape sequence.
    fn parse_escape(&mut self) -> Result<(), ParseError> {
        let start = self.pos;
        // The ESC byte itself.
        self.advance_char();
        match self.peek() {
            Some('[') => {
                self.advance_char();
                self.parse_csi(start)
            }
            Some(']') => {
                self.advance_char();
                self.parse_osc(start)
            }
            other => Err(ParseError {
                at: start,
                kind: ParseErrorKind::UnknownSequence {
                    sequence: other.map(|c| c.to_string()).unwrap_or_default(),
                },
            }),
        }
    }

    /// Parses a CSI sequence: `ESC [ <params> <final>`. Only the
    /// canonical finals are accepted: `m` (SGR), `H` (cursor position)
    /// and `K` (erase to end of line).
    fn parse_csi(&mut self, start: usize) -> Result<(), ParseError> {
        let params_start = self.pos;
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_digit() || character == ';')
        {
            self.advance_char();
        }
        let params = &self.input[params_start..self.pos];
        let Some(final_byte) = self.peek() else {
            return Err(ParseError {
                at: start,
                kind: ParseErrorKind::Unterminated,
            });
        };
        self.advance_char();
        match final_byte {
            'm' => self.apply_sgr(params, start),
            'H' => self.apply_cup(params, start),
            'K' if params.is_empty() => {
                // Erase to end of line: the rest of the row is blank.
                // Nothing to record — blank cells are the default — and
                // the canonical form never writes past it.
                Ok(())
            }
            _ => Err(ParseError {
                at: start,
                kind: ParseErrorKind::UnknownSequence {
                    sequence: escape_debug_of(&self.input[start..self.pos]),
                },
            }),
        }
    }

    /// Applies an SGR run. `params` is `;`-separated; an empty run is a
    /// reset (per the standard).
    fn apply_sgr(&mut self, params: &str, start: usize) -> Result<(), ParseError> {
        let numbers: Vec<u16> = params
            .split(';')
            .map(|param| param.parse::<u16>().unwrap_or(0))
            .collect();
        let numbers = if params.is_empty() { vec![0] } else { numbers };
        let mut index = 0;
        while index < numbers.len() {
            let parameter = numbers[index];
            index += 1;
            match parameter {
                0 => {
                    self.style = Style::new();
                }
                1 => {
                    self.style = self.style.add_modifier(Modifier::BOLD);
                }
                2 => {
                    self.style = self.style.add_modifier(Modifier::DIM);
                }
                3 => {
                    self.style = self.style.add_modifier(Modifier::ITALIC);
                }
                4 => {
                    self.style = self.style.add_modifier(Modifier::UNDERLINED);
                }
                5 => {
                    self.style = self.style.add_modifier(Modifier::SLOW_BLINK);
                }
                6 => {
                    self.style = self.style.add_modifier(Modifier::RAPID_BLINK);
                }
                7 => {
                    self.style = self.style.add_modifier(Modifier::REVERSED);
                }
                8 => {
                    self.style = self.style.add_modifier(Modifier::HIDDEN);
                }
                9 => {
                    self.style = self.style.add_modifier(Modifier::CROSSED_OUT);
                }
                22 => {
                    self.style = self.style.remove_modifier(Modifier::BOLD | Modifier::DIM);
                }
                23 => {
                    self.style = self.style.remove_modifier(Modifier::ITALIC);
                }
                24 => {
                    self.style = self.style.remove_modifier(Modifier::UNDERLINED);
                }
                25 => {
                    self.style = self
                        .style
                        .remove_modifier(Modifier::SLOW_BLINK | Modifier::RAPID_BLINK);
                }
                27 => {
                    self.style = self.style.remove_modifier(Modifier::REVERSED);
                }
                28 => {
                    self.style = self.style.remove_modifier(Modifier::HIDDEN);
                }
                29 => {
                    self.style = self.style.remove_modifier(Modifier::CROSSED_OUT);
                }
                30..=37 => {
                    self.style.fg = Some(named_color(parameter - 30, false));
                }
                39 => self.style.fg = None,
                40..=47 => {
                    self.style.bg = Some(named_color(parameter - 40, false));
                }
                49 => self.style.bg = None,
                90..=97 => {
                    self.style.fg = Some(named_color(parameter - 90, true));
                }
                100..=107 => {
                    self.style.bg = Some(named_color(parameter - 100, true));
                }
                38 | 48 | 58 => {
                    let Some(color) = Self::extended_color(&numbers, &mut index, parameter) else {
                        return Err(ParseError {
                            at: start,
                            kind: ParseErrorKind::UnknownSgr { parameter },
                        });
                    };
                    match parameter {
                        38 => self.style.fg = Some(color),
                        48 => self.style.bg = Some(color),
                        _ => self.style.underline_color = Some(color),
                    }
                }
                other => {
                    return Err(ParseError {
                        at: start,
                        kind: ParseErrorKind::UnknownSgr { parameter: other },
                    });
                }
            }
        }
        Ok(())
    }

    /// Parses the payload of an extended color (`38/48/58;5;n` or
    /// `38/48/58;2;r;g;b`), advancing `index` past it. `role` is the
    /// selector (38 fg, 48 bg, 58 underline): only the underline role
    /// uses palette indices 0-15 for the named 16 (that is how the
    /// encoders emit named underline colors), so `58;5;n` with `n <= 15`
    /// reconstructs as a named color while `38;5;9` stays
    /// [`Color::Indexed`].
    fn extended_color(numbers: &[u16], index: &mut usize, role: u16) -> Option<Color> {
        let kind = *numbers.get(*index)?;
        *index += 1;
        match kind {
            5 => {
                let value = *numbers.get(*index)?;
                *index += 1;
                Some(if role == 58 && value <= 15 {
                    named_color(value % 8, value >= 8)
                } else {
                    Color::Indexed(u8::try_from(value).ok()?)
                })
            }
            2 => {
                let r = u8::try_from(*numbers.get(*index)?).ok()?;
                *index += 1;
                let g = u8::try_from(*numbers.get(*index)?).ok()?;
                *index += 1;
                let b = u8::try_from(*numbers.get(*index)?).ok()?;
                *index += 1;
                Some(Color::Rgb { r, g, b })
            }
            _ => None,
        }
    }

    /// Applies a cursor position (`ESC[<row>;<col>H`, 1-based).
    fn apply_cup(&mut self, params: &str, start: usize) -> Result<(), ParseError> {
        let mut parts = params.split(';');
        let (Some(row), Some(col)) = (parts.next(), parts.next()) else {
            return Err(ParseError {
                at: start,
                kind: ParseErrorKind::UnknownSequence {
                    sequence: escape_debug_of(&self.input[start..self.pos]),
                },
            });
        };
        if parts.next().is_some() {
            return Err(ParseError {
                at: start,
                kind: ParseErrorKind::UnknownSequence {
                    sequence: escape_debug_of(&self.input[start..self.pos]),
                },
            });
        }
        let (Ok(row), Ok(col)) = (row.parse::<u32>(), col.parse::<u32>()) else {
            return Err(ParseError {
                at: start,
                kind: ParseErrorKind::UnknownSequence {
                    sequence: escape_debug_of(&self.input[start..self.pos]),
                },
            });
        };
        if row == 0 || col == 0 || row > u32::from(u16::MAX) || col > u32::from(u16::MAX) {
            return Err(ParseError {
                at: start,
                kind: ParseErrorKind::UnknownSequence {
                    sequence: escape_debug_of(&self.input[start..self.pos]),
                },
            });
        }
        self.cursor_row = (row - 1) as u16;
        self.cursor_col = (col - 1) as u16;
        // Addressed rows count toward the discovered height even when
        // nothing is written on them (a blank CUP + EL row).
        self.ensure_row(self.cursor_row);
        Ok(())
    }

    /// Parses an OSC sequence: `ESC ] <body> ESC \`. Only OSC 8
    /// hyperlinks exist in the canonical format.
    fn parse_osc(&mut self, start: usize) -> Result<(), ParseError> {
        let body_start = self.pos;
        let body_end = self.input[body_start..]
            .find("\x1b\\")
            .map(|offset| body_start + offset);
        let Some(body_end) = body_end else {
            return Err(ParseError {
                at: start,
                kind: ParseErrorKind::Unterminated,
            });
        };
        let body = &self.input[body_start..body_end];
        self.pos = body_end + 2;
        let Some(uri) = body.strip_prefix("8;;") else {
            return Err(ParseError {
                at: start,
                kind: ParseErrorKind::UnknownSequence {
                    sequence: escape_debug_of(&self.input[start..self.pos]),
                },
            });
        };
        // An empty URI closes the link; anything else opens one.
        self.link = if uri.is_empty() {
            None
        } else {
            Some(Hyperlink::new(uri))
        };
        Ok(())
    }

    /// Advances one character (multibyte-safe).
    fn advance_char(&mut self) {
        if let Some(character) = self.peek() {
            self.pos += character.len_utf8();
        }
    }
}

/// Named-color helper: `bright` selects the 90-97/100-107 family
/// (`DarkGray`..`White`); the base family is `Black`..`Gray`.
fn named_color(index: u16, bright: bool) -> Color {
    let index = index.min(7) as usize;
    if bright {
        [
            Color::DarkGray,
            Color::LightRed,
            Color::LightGreen,
            Color::LightYellow,
            Color::LightBlue,
            Color::LightMagenta,
            Color::LightCyan,
            Color::White,
        ][index]
    } else {
        [
            Color::Black,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::Gray,
        ][index]
    }
}

/// The rejected sequence for error messages, with control characters
/// made visible.
fn escape_debug_of(sequence: &str) -> String {
    sequence.chars().flat_map(char::escape_debug).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::serialize_ansi;

    fn parse(ansi: &str) -> Screen {
        Screen::from_ansi(ansi).unwrap()
    }

    fn screen_of(buffer: &Buffer) -> Screen {
        Screen::from_ansi(&serialize_ansi(buffer)).unwrap()
    }

    #[test]
    fn plain_text_round_trips_through_parse() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 2));
        buffer.set_string(0, 0, "hi", Style::new());
        buffer.set_string(0, 1, "row two", Style::new());
        let screen = screen_of(&buffer);
        // Dimensions are discovered: the longest written row (7), the
        // highest addressed row (2).
        assert_eq!(screen.width(), 7);
        assert_eq!(screen.height(), 2);
        assert_eq!(screen.cell(0, 0).unwrap().symbol.as_str(), "h");
        assert_eq!(screen.cell(1, 0).unwrap().symbol.as_str(), "i");
        assert_eq!(screen.to_buffer(Rect::new(0, 0, 10, 2)), buffer);
    }

    #[test]
    fn styled_cells_reconstruct_sgr() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
        buffer.set_string(
            0,
            0,
            "red",
            Style::new()
                .fg(Color::Red)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD | Modifier::ITALIC),
        );
        let screen = screen_of(&buffer);
        let cell = screen.cell(2, 0).unwrap();
        assert_eq!(cell.style.fg, Some(Color::Red));
        assert_eq!(cell.style.bg, Some(Color::Blue));
        assert!(cell.style.add_modifier.contains(Modifier::BOLD));
        assert!(cell.style.add_modifier.contains(Modifier::ITALIC));
        assert_eq!(screen.to_buffer(Rect::new(0, 0, 4, 1)), buffer);
    }

    #[test]
    fn indexed_and_rgb_colors_reconstruct() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        buffer.set_string(0, 0, "a", Style::new().fg(Color::Indexed(196)));
        buffer.set_string(
            1,
            0,
            "b",
            Style::new()
                .fg(Color::Rgb { r: 1, g: 2, b: 3 })
                .bg(Color::Indexed(21)),
        );
        let screen = screen_of(&buffer);
        assert_eq!(
            screen.cell(0, 0).unwrap().style.fg,
            Some(Color::Indexed(196))
        );
        let cell = screen.cell(1, 0).unwrap();
        assert_eq!(cell.style.fg, Some(Color::Rgb { r: 1, g: 2, b: 3 }));
        assert_eq!(cell.style.bg, Some(Color::Indexed(21)));
        assert_eq!(screen.to_buffer(Rect::new(0, 0, 3, 1)), buffer);
    }

    #[test]
    fn bright_colors_reconstruct() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
        buffer.set_string(
            0,
            0,
            "x",
            Style::new()
                .fg(Color::DarkGray)
                .bg(Color::LightCyan)
                .add_modifier(Modifier::UNDERLINED)
                .underline_color(Color::Yellow),
        );
        let screen = screen_of(&buffer);
        let cell = screen.cell(0, 0).unwrap();
        assert_eq!(cell.style.fg, Some(Color::DarkGray));
        assert_eq!(cell.style.bg, Some(Color::LightCyan));
        assert_eq!(cell.style.underline_color, Some(Color::Yellow));
        assert_eq!(screen.to_buffer(Rect::new(0, 0, 2, 1)), buffer);
    }

    #[test]
    fn hyperlinks_reconstruct() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
        buffer.set_string_with_hyperlink(
            0,
            0,
            "link",
            Style::new(),
            Hyperlink::new("https://tuvix.dev"),
        );
        let screen = screen_of(&buffer);
        let cell = screen.cell(0, 0).unwrap();
        assert_eq!(
            cell.hyperlink.as_ref().map(Hyperlink::as_str),
            Some("https://tuvix.dev")
        );
        // After the link closes, later cells are unlinked again.
        assert!(
            screen
                .cell(4, 0)
                .is_none_or(|cell| cell.hyperlink.is_none())
        );
        assert_eq!(screen.to_buffer(Rect::new(0, 0, 8, 1)), buffer);
    }

    #[test]
    fn wide_glyphs_reconstruct_with_covered_tails() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 5, 1));
        buffer.set_string(0, 0, "汉字", Style::new());
        buffer.set_string(4, 0, "!", Style::new());
        let screen = screen_of(&buffer);
        assert_eq!(screen.cell(0, 0).unwrap().symbol.as_str(), "汉");
        assert!(screen.cell(1, 0).unwrap().skip);
        assert_eq!(screen.cell(2, 0).unwrap().symbol.as_str(), "字");
        assert!(screen.cell(3, 0).unwrap().skip);
        assert_eq!(screen.cell(4, 0).unwrap().symbol.as_str(), "!");
        assert_eq!(screen.to_buffer(Rect::new(0, 0, 5, 1)), buffer);
    }

    #[test]
    fn el_tails_reconstruct_as_blanks() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        buffer.set_string(0, 0, "hi", Style::new());
        let screen = screen_of(&buffer);
        // The EL erased columns 2..10: reconstruction pads them back.
        assert_eq!(screen.to_buffer(Rect::new(0, 0, 10, 1)), buffer);
        // Width is discovered as 2: nothing beyond the glyphs was
        // written, so those columns are simply not part of the screen.
        assert_eq!(screen.cell(5, 0), None);
    }

    #[test]
    fn blank_rows_still_count_toward_height() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 4));
        buffer.set_string(0, 0, "top", Style::new());
        let screen = screen_of(&buffer);
        // The three blank CUP + EL rows still count toward the height;
        // the width is the longest written row.
        assert_eq!(screen.height(), 4);
        assert_eq!(screen.width(), 3);
        assert_eq!(screen.to_buffer(Rect::new(0, 0, 10, 4)), buffer);
    }

    #[test]
    fn dimensions_are_discovered_from_content() {
        // Two CUP rows, content only up to column 3 on row 2.
        let ansi = "\x1b[1;1Htop\x1b[K\x1b[2;1Habc\x1b[K\x1b[3;1H\x1b[K";
        let screen = parse(ansi);
        assert_eq!(screen.height(), 3);
        assert_eq!(screen.width(), 3);
        assert_eq!(screen.cell(0, 1).unwrap().symbol.as_str(), "a");
    }

    #[test]
    fn empty_input_is_an_empty_screen() {
        let screen = parse("");
        assert!(screen.is_empty());
        assert!(screen.is_blank());
        assert_eq!(screen.width(), 0);
        assert_eq!(screen.height(), 0);
        assert_eq!(screen.cell(0, 0), None);
    }

    #[test]
    fn fully_blank_snapshot_is_blank_but_sized() {
        let screen = parse("\x1b[1;1H\x1b[K\x1b[2;1H\x1b[K");
        assert!(!screen.is_empty());
        assert!(screen.is_blank());
        assert_eq!(screen.height(), 2);
    }

    #[test]
    fn to_buffer_clips_and_pads() {
        let screen = parse("\x1b[1;1Habc\x1b[K\x1b[2;1Hdef\x1b[K");
        // Paste into a smaller area: clipped.
        let small = screen.to_buffer(Rect::new(0, 0, 2, 1));
        assert_eq!(small.get(0, 0).unwrap().symbol.as_str(), "a");
        assert_eq!(small.get(1, 0).unwrap().symbol.as_str(), "b");
        // Paste into a larger area: the rest stays blank.
        let large = screen.to_buffer(Rect::new(0, 0, 10, 5));
        assert_eq!(large.get(2, 0).unwrap().symbol.as_str(), "c");
        assert_eq!(large.get(9, 4), Some(&Cell::empty()));
    }

    #[test]
    fn diff_reports_changed_cells_over_the_union() {
        let before = parse("\x1b[1;1Habc\x1b[K");
        let after = parse("\x1b[1;1Haxc\x1b[K\x1b[2;1Hnew\x1b[K");
        let changed = before.diff(&after);
        assert_eq!(changed.len(), 4);
        let mut expected = Cell::empty();
        expected.set_symbol("x");
        assert_eq!(changed[0], (1, 0, expected));
        // The second row exists only in `after` (blank in `before`).
        assert_eq!(changed[1].0, 0);
        assert_eq!(changed[1].1, 1);
        assert_eq!(changed[3].2.symbol.as_str(), "w");
        // Identical screens diff to nothing.
        assert_eq!(before.diff(&before), Vec::new());
    }

    #[test]
    fn unknown_sequences_are_rejected() {
        // DECTCEM cursor hide: never in a canonical snapshot.
        let error = Screen::from_ansi("\x1b[?25l").unwrap_err();
        assert_eq!(error.at, 0);
        assert!(matches!(error.kind, ParseErrorKind::UnknownSequence { .. }));
        // Unknown CSI final.
        assert!(Screen::from_ansi("\x1b[2J").is_err());
        // Non-8 OSC.
        assert!(Screen::from_ansi("\x1b]0;title\x1b\\").is_err());
        // Lone ESC.
        assert!(Screen::from_ansi("\x1b").is_err());
    }

    #[test]
    fn unknown_sgr_parameters_are_rejected() {
        let error = Screen::from_ansi("\x1b[51m").unwrap_err();
        assert_eq!(error.kind, ParseErrorKind::UnknownSgr { parameter: 51 });
        // A truncated extended color.
        assert!(Screen::from_ansi("\x1b[38;5m").is_err());
    }

    #[test]
    fn unterminated_sequences_are_rejected() {
        assert_eq!(
            Screen::from_ansi("\x1b]8;;https://x").unwrap_err().kind,
            ParseErrorKind::Unterminated
        );
        assert_eq!(
            Screen::from_ansi("\x1b[31").unwrap_err().kind,
            ParseErrorKind::Unterminated
        );
    }

    #[test]
    fn control_characters_in_text_are_rejected() {
        let error = Screen::from_ansi("hi\x07there").unwrap_err();
        assert_eq!(error.at, 2);
        assert_eq!(
            error.kind,
            ParseErrorKind::ControlCharacter { character: '\x07' }
        );
        assert!(Screen::from_ansi("a\tb").is_err());
    }

    #[test]
    fn parse_error_displays_offset_and_kind() {
        let error = Screen::from_ansi("hi\x1b[?25l").unwrap_err();
        let message = error.to_string();
        assert!(message.contains('2'));
        assert!(message.contains("unknown escape sequence"));
    }

    #[test]
    fn sgr_off_codes_clear_modifiers() {
        // Robustness beyond the canonical emitter: on then off.
        let screen = parse("\x1b[1;4mbold underline\x1b[22;24mplain");
        assert!(
            screen
                .cell(0, 0)
                .unwrap()
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        let plain = screen.cell(14, 0).unwrap();
        assert_eq!(plain.style.add_modifier, Modifier::empty());
        assert_eq!(plain.symbol.as_str(), "p");
    }

    #[test]
    fn canonical_round_trip_is_the_identity() {
        // A rich frame: styles, bright colors, indexed + RGB, modifiers,
        // a hyperlink, wide glyphs, blank rows.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 4));
        buffer.set_string(
            0,
            0,
            "dashboard",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        );
        buffer.set_string(0, 1, "汉x!", Style::new().fg(Color::Indexed(214)));
        buffer.set_string_with_hyperlink(
            5,
            1,
            "docs",
            Style::new().fg(Color::Rgb {
                r: 10,
                g: 200,
                b: 30,
            }),
            Hyperlink::new("https://tuvix.dev/docs"),
        );
        buffer.set_string(
            0,
            3,
            "warn",
            Style::new()
                .fg(Color::LightYellow)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::REVERSED | Modifier::SLOW_BLINK),
        );
        let ansi = serialize_ansi(&buffer);
        let screen = Screen::from_ansi(&ansi).unwrap();
        // Cell-level reconstruction is exact, and re-serializing the
        // reconstruction at the original viewport is the byte identity.
        assert_eq!(screen.to_buffer(Rect::new(0, 0, 20, 4)), buffer);
        assert_eq!(
            serialize_ansi(&screen.to_buffer(Rect::new(0, 0, 20, 4))),
            ansi
        );
    }

    #[test]
    fn wide_emoji_corpus_round_trips() {
        // ZWJ sequences, variation selectors: one cluster per cell with
        // covered tails.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
        buffer.set_string(0, 0, "👨‍👩‍👧‍👦", Style::new());
        buffer.set_string(2, 0, "🙂", Style::new());
        let ansi = serialize_ansi(&buffer);
        let screen = Screen::from_ansi(&ansi).unwrap();
        assert_eq!(screen.to_buffer(Rect::new(0, 0, 8, 1)), buffer);
        assert_eq!(
            serialize_ansi(&screen.to_buffer(Rect::new(0, 0, 8, 1))),
            ansi
        );
    }

    #[test]
    fn interior_gaps_survive_the_round_trip() {
        // A blank cell between two runs writes no bytes; the snapshot
        // encoder re-addresses the next glyph so playback and parsing
        // both keep the gap.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 1));
        buffer.set_string(0, 0, "ab", Style::new());
        buffer.set_string(6, 0, "cd", Style::new().fg(Color::Red));
        let ansi = serialize_ansi(&buffer);
        assert!(ansi.contains("\x1b[1;7H")); // the mid-row re-address
        let screen = Screen::from_ansi(&ansi).unwrap();
        assert_eq!(screen.to_buffer(Rect::new(0, 0, 12, 1)), buffer);
        assert_eq!(
            serialize_ansi(&screen.to_buffer(Rect::new(0, 0, 12, 1))),
            ansi
        );
    }

    #[test]
    fn screens_are_send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<Screen>();
        assert_sync::<Screen>();
    }
}
