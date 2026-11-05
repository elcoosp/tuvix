//! The cell grid, the double buffer, and the zero-allocation diff engine.
//!
//! # Rendering model
//!
//! Widgets paint into a [`Buffer`]. The [`crate::terminal::Terminal`] keeps
//! two buffers (front = what the terminal shows, back = what is being built)
//! and produces the minimal set of updates between them with
//! [`Buffer::diff_runs`]. The diff yields *runs* of contiguous changed cells
//! on the same row — one cursor move plus one slice write per run instead of
//! per-cell dispatch — without allocating.
//!
//! # Wide characters
//!
//! `set_string` measures graphemes with `unicode-width`. A width-2 grapheme
//! writes its leading cell and marks the trailing cell as `skip`, so the
//! pair is always redrawn atomically: whenever a leading cell is emitted by
//! the diff, its trailing cell is forced into the same run, and cells that
//! used to be covered by a wide glyph are compared against an empty cell so
//! stale content cannot survive an overwrite.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr as _;

use core::hash::{Hash, Hasher};

use crate::damage::Band;
use crate::geometry::{Position, Rect, Size};
use crate::journal::FrameJournal;
use crate::style::{Hyperlink, Style};

/// Inline capacity of a [`Symbol`]: graphemes up to this many UTF-8 bytes
/// are stored in the cell itself with no heap involvement.
const INLINE_CAP: usize = 14;

/// Capacity of the thin-boxed heap tier of a [`Symbol`].
///
/// Graphemes above [`INLINE_CAP`] bytes are boxed into a fixed 62-byte
/// buffer behind a thin pointer (a `Box<[u8; 62]>` is a sized pointee, so
/// the whole [`Symbol`] stays 16 bytes). Pathological graphemes longer than
/// that fall through to a double box; only they pay the second indirection.
const HEAP_CAP: usize = 62;

/// The glyph displayed in a cell: 16 bytes total, no pointer chase on the
/// hot path.
///
/// This is the M1 replacement for the M0 per-cell `String` and the single
/// biggest diff-walk win: comparing two inline symbols is a 15-byte stack
/// compare, not a pointer dereference. Terminal realities informed the
/// tiering: ASCII and CJK are 1-3 bytes, all BMP emoji are 4, the widest
/// common sequences (flags, VS16, skin tones) stay ≤ 8, and ZWJ family
/// emoji (~25 bytes) land in the boxed tier.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Symbol;
///
/// let inline = Symbol::new("汉");
/// assert!(inline.is_inline());
/// assert_eq!(inline.as_str(), "汉");
///
/// // A ZWJ family emoji exceeds the inline capacity but is preserved
/// // byte-identically through the boxed tier.
/// let family = Symbol::new("👨‍👩‍👧‍👦");
/// assert!(!family.is_inline());
/// assert_eq!(family.as_str(), "👨‍👩‍👧‍👦");
/// ```
#[derive(Debug, Clone)]
pub struct Symbol(Repr);

#[derive(Debug, Clone)]
enum Repr {
    /// Up to [`INLINE_CAP`] UTF-8 bytes, stored inline; `bytes` beyond
    /// `len` is always zeroed so equality can compare the whole array.
    Inline { len: u8, bytes: [u8; INLINE_CAP] },
    /// Up to [`HEAP_CAP`] bytes behind a thin pointer.
    Heap { len: u8, bytes: Box<[u8; HEAP_CAP]> },
    /// Anything longer; the double box keeps the thin-pointer size win and
    /// the cost is irrelevant at this frequency.
    Huge(Box<Box<str>>),
}

impl Symbol {
    /// Builds a symbol from `glyph`.
    pub fn new<T: AsRef<str>>(glyph: T) -> Self {
        let glyph = glyph.as_ref();
        let bytes = glyph.as_bytes();
        let repr = if bytes.len() <= INLINE_CAP {
            let mut inline = [0_u8; INLINE_CAP];
            inline[..bytes.len()].copy_from_slice(bytes);
            Repr::Inline {
                len: bytes.len() as u8,
                bytes: inline,
            }
        } else if bytes.len() <= HEAP_CAP {
            let mut boxed = Box::new([0_u8; HEAP_CAP]);
            boxed[..bytes.len()].copy_from_slice(bytes);
            Repr::Heap {
                len: bytes.len() as u8,
                bytes: boxed,
            }
        } else {
            Repr::Huge(Box::new(Box::from(glyph)))
        };
        Self(repr)
    }

    /// The empty symbol.
    pub const fn empty() -> Self {
        Self(Repr::Inline {
            len: 0,
            bytes: [0; INLINE_CAP],
        })
    }

    /// The glyph as a string slice.
    pub fn as_str(&self) -> &str {
        match &self.0 {
            Repr::Inline { len, bytes } => str::from_utf8(&bytes[..*len as usize]).unwrap_or(""),
            Repr::Heap { len, bytes } => str::from_utf8(&bytes[..*len as usize]).unwrap_or(""),
            Repr::Huge(boxed) => boxed,
        }
    }

    /// Returns `true` when the glyph is stored inline (no heap tier).
    pub fn is_inline(&self) -> bool {
        matches!(self.0, Repr::Inline { .. })
    }

    /// Returns `true` when the symbol displays nothing.
    pub fn is_empty(&self) -> bool {
        matches!(self.0, Repr::Inline { len: 0, bytes: _ })
    }

    /// Display width of the glyph in terminal columns.
    ///
    /// ASCII graphics take a one-byte fast path; everything else resolves
    /// through `unicode-width` exactly like the M0 implementation.
    pub fn width(&self) -> usize {
        match &self.0 {
            Repr::Inline {
                len: 1,
                bytes: [byte, ..],
            } if byte.is_ascii_graphic() || *byte == b' ' => 1,
            _ => self.as_str().width(),
        }
    }
}

impl PartialEq for Symbol {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            // Fast path: both inline, both zero-padded — a flat array compare.
            (
                Repr::Inline { len, bytes },
                Repr::Inline {
                    len: other_len,
                    bytes: other_bytes,
                },
            ) => len == other_len && bytes == other_bytes,
            _ => self.as_str() == other.as_str(),
        }
    }
}

impl Eq for Symbol {}

impl PartialEq<&str> for Symbol {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for Symbol {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<Symbol> for &str {
    fn eq(&self, other: &Symbol) -> bool {
        other.as_str() == *self
    }
}

impl Hash for Symbol {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Hash::hash(&self.as_str(), state);
    }
}

impl Default for Symbol {
    fn default() -> Self {
        Self::empty()
    }
}

impl core::fmt::Display for Symbol {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single terminal cell: one grapheme plus its style, hyperlink and
/// coverage flag.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct Cell {
    /// The glyph displayed in this cell; empty when the cell is blank.
    pub symbol: Symbol,
    /// The style of the symbol.
    pub style: Style,
    /// The OSC 8 hyperlink the cell participates in, if any.
    pub hyperlink: Option<Hyperlink>,
    /// `true` when this cell is the trailing half of a wide glyph and is
    /// covered by it — covered cells are never emitted by the diff.
    pub skip: bool,
}

impl Cell {
    /// A blank cell: empty symbol, default style, no hyperlink.
    pub fn empty() -> Self {
        Self {
            symbol: Symbol::empty(),
            style: Style::new(),
            hyperlink: None,
            skip: false,
        }
    }

    /// Replaces the symbol, keeping style and flags.
    pub fn set_symbol<T: AsRef<str>>(&mut self, symbol: T) -> &mut Self {
        self.symbol = Symbol::new(symbol.as_ref());
        self
    }

    /// Layers `style` on top of the current cell style.
    pub fn set_style(&mut self, style: Style) -> &mut Self {
        self.style = self.style.patch(style);
        self
    }

    /// Sets (or clears) the cell's OSC 8 hyperlink.
    pub fn set_hyperlink(&mut self, hyperlink: Option<Hyperlink>) -> &mut Self {
        self.hyperlink = hyperlink;
        self
    }

    /// Resets the cell to blank.
    pub fn reset(&mut self) {
        self.symbol = Symbol::empty();
        self.style = Style::new();
        self.hyperlink = None;
        self.skip = false;
    }

    /// Returns `true` when the cell displays nothing.
    pub fn is_empty(&self) -> bool {
        self.symbol.is_empty()
    }

    /// Display width of the symbol in terminal columns (1 for normal glyphs,
    /// 2 for wide glyphs, 0 for zero-width glyphs).
    pub fn symbol_width(&self) -> usize {
        self.symbol.width()
    }

    /// Returns `true` when the symbol occupies two columns and therefore
    /// covers its trailing neighbor.
    pub fn is_wide(&self) -> bool {
        self.symbol.width() == 2
    }

    /// Returns `true` when the cell carries no visible content difference:
    /// blank symbol, default style, no hyperlink. A hyperlink-only cell
    /// changes what a click on that position opens, so it is content.
    fn is_blank(&self) -> bool {
        self.symbol.is_empty() && self.style == Style::new() && self.hyperlink.is_none()
    }
}

/// A contiguous run of changed cells on a single row, borrowed from the new
/// buffer.
///
/// Runs are the unit of I/O in tuvix: a backend performs exactly one cursor
/// move per run and writes the cells back to back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffRun<'a> {
    /// Row index of the run.
    pub row: u16,
    /// Column index of the first cell of the run.
    pub start: u16,
    /// The changed cells, `cells.len()` columns starting at `start`.
    pub cells: &'a [Cell],
    /// Erase-to-end-of-line elision: after writing `cells`, every cell
    /// from `start + cells.len()` to the end of the row is a blank that
    /// must become blank — one `EL` control instead of N space writes.
    /// Backends that cannot erase must write the blanks themselves.
    pub elide_rest: bool,
}

/// Minimum number of trailing blank cells the elision optimization must
/// save to bother with an `EL` control sequence.
const ELIDE_MIN_CELLS: usize = 2;

impl DiffRun<'_> {
    /// Number of cells in the run.
    pub fn len(&self) -> u16 {
        self.cells.len() as u16
    }

    /// Returns `true` when the run is empty (never produced by the diff).
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

/// A lazy iterator over all [`DiffRun`]s between two buffers.
///
/// The iterator borrows both buffers and allocates nothing. When constructed
/// through [`Buffer::diff_runs_banded`], run *starts* are restricted to the
/// given journal bands, so untouched rows cost nothing; run *extension* and
/// the wide-character pair rules stay exactly as in the full walk, which is
/// what keeps banded output byte-identical to full-diff output.
#[derive(Debug)]
pub struct DiffRuns<'a> {
    previous: &'a Buffer,
    next: &'a Buffer,
    row: u16,
    col: u16,
    bands: Option<&'a [Band]>,
    band_index: usize,
}

impl<'a> Iterator for DiffRuns<'a> {
    type Item = DiffRun<'a>;
    #[allow(clippy::too_many_lines)] // one walk, four rules; splitting hides the invariants
    fn next(&mut self) -> Option<Self::Item> {
        if self.previous.area != self.next.area {
            return None;
        }
        let width = self.next.area.width;
        loop {
            if self.bands.is_none() {
                if self.row >= self.next.area.height {
                    return None;
                }
            } else if !self.advance_to_band() {
                return None;
            }
            let scan_end = match self.bands {
                Some(bands) => bands[self.band_index].end.min(width),
                None => width,
            };
            // Skip unchanged leading cells (within the scan domain).
            while self.col < scan_end && !self.should_emit(self.row, self.col) {
                self.col += 1;
            }
            if self.col >= scan_end {
                if self.bands.is_some() {
                    // Band exhausted; the next band repositions us.
                    self.band_index += 1;
                    continue;
                }
                self.row += 1;
                self.col = 0;
                continue;
            }
            let start = self.col;
            // Extend the run: a cell continues the run when it changed, or
            // when it is the forced trailing half of a wide leading glyph
            // (keeps the pair atomic on terminals with quirky repair logic).
            let mut end = self.col;
            while end < width {
                let continue_run = end + 1 < width
                    && (self.should_emit(self.row, end + 1)
                        || self.next_cell(self.row, end).is_wide());
                if continue_run {
                    end += 1;
                } else {
                    break;
                }
            }

            // Elision attempt: strip trailing blank cells from the run and
            // let the backend erase them with one EL instead of writing
            // spaces. Only worth it from ELIDE_MIN_CELLS saved writes.
            //
            // Invariant: `last` is the last column that would be written.
            // It walks down from `end` while the cell there is a plain
            // blank (empty symbol, default style, not a covered cell).
            let mut last = end;
            let mut stripped = 0usize;
            loop {
                let cell = self.next_cell(self.row, last);
                // Never strip a covered cell: it is the forced trailing
                // half of a wide pair and must be written atomically.
                // A hyperlink is content too: erasing the tail with EL
                // would not remove it from the cells before it, and the
                // cell must be written to carry (or close) the link.
                if cell.skip
                    || !cell.symbol.is_empty()
                    || cell.style != Style::new()
                    || cell.hyperlink.is_some()
                {
                    break;
                }
                stripped += 1;
                if last == start {
                    last = u16::MAX; // sentinel: the whole run was blank
                    break;
                }
                last -= 1;
            }
            let whole_run_stripped = last == u16::MAX;
            if stripped >= ELIDE_MIN_CELLS {
                let first = if whole_run_stripped { start } else { last + 1 };
                if self.tail_is_blank(self.row, first, width) {
                    let row_offset = self.row as usize * width as usize;
                    let cells = if whole_run_stripped {
                        &self.next.content[row_offset..row_offset]
                    } else {
                        &self.next.content[row_offset + start as usize..=row_offset + last as usize]
                    };
                    // The EL covers everything to the end of the row; no
                    // other run can start on it. In banded mode the row
                    // advance makes the band skip-drop every remaining
                    // band on this row.
                    let row = self.row;
                    self.row += 1;
                    self.col = 0;
                    return Some(DiffRun {
                        row,
                        start,
                        cells,
                        elide_rest: true,
                    });
                }
            }

            let row_offset = self.row as usize * width as usize;
            let cells = &self.next.content[row_offset + start as usize..=row_offset + end as usize];
            self.col = end + 1;
            return Some(DiffRun {
                row: self.row,
                start,
                cells,
                elide_rest: false,
            });
        }
    }
}

impl DiffRuns<'_> {
    /// Advances the banded walk to the next band that still has work:
    /// bands fully consumed by row progress or by a previous run that
    /// extended past them are skipped. Positions `row`/`col` at the band's
    /// scan domain and returns `false` when no band remains.
    fn advance_to_band(&mut self) -> bool {
        let bands = self.bands.unwrap_or(&[]);
        loop {
            if self.band_index >= bands.len() {
                return false;
            }
            let band = bands[self.band_index];
            let consumed = band.row < self.row || (band.row == self.row && band.end <= self.col);
            if !consumed {
                // Position the scan at the band's domain.
                if self.row != band.row {
                    self.row = band.row;
                    self.col = band.start;
                } else if self.col < band.start {
                    self.col = band.start;
                }
                return true;
            }
            self.band_index += 1;
        }
    }

    /// Returns `true` when the cell at `position` must be emitted: it is not
    /// covered by a wide glyph and differs from what the terminal currently
    /// shows there.
    ///
    /// Cells whose left neighbor is a wide glyph in the previous buffer are
    /// compared against an empty cell: overwriting the wide glyph clears the
    /// whole pair on the terminal, so the covered cell must be repainted from
    /// scratch.
    fn should_emit(&self, row: u16, col: u16) -> bool {
        let next = self.next_cell(row, col);
        if next.skip {
            return false;
        }
        let previous = self.previous_cell(row, col);
        let covered_by_wide = col > 0 && self.previous_cell(row, col - 1).is_wide();
        if covered_by_wide {
            !next.is_blank()
        } else {
            next.symbol != previous.symbol
                || next.style != previous.style
                || next.hyperlink != previous.hyperlink
        }
    }

    fn previous_cell(&self, row: u16, col: u16) -> &Cell {
        &self.previous.content[row as usize * self.previous.area.width as usize + col as usize]
    }

    fn next_cell(&self, row: u16, col: u16) -> &Cell {
        &self.next.content[row as usize * self.next.area.width as usize + col as usize]
    }

    /// Returns `true` when every cell from `first` (inclusive) to the row
    /// end displays blank with the default style and no hyperlink. Those
    /// are exactly the cells an `EL` control issued at `first` would erase
    /// to.
    fn tail_is_blank(&self, row: u16, first: u16, width: u16) -> bool {
        (first..width).all(|col| {
            let cell = self.next_cell(row, col);
            cell.symbol.is_empty() && cell.style == Style::new() && cell.hyperlink.is_none()
        })
    }
}

/// A rectangular grid of [`Cell`]s: the canvas every widget paints into.
///
/// Every mutating method records the touched region into the buffer's
/// optional [`FrameJournal`] (see [`Buffer::enable_journal`]), which is how
/// the persistent-DOM [`crate::terminal::Terminal`] mode reaches the O(1)
/// no-op frame decision. Without a journal installed the mutators behave
/// exactly as they always have.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::style::{Color, Style};
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
/// buffer.set_string(0, 0, "hi", Style::new().fg(Color::Red));
/// assert_eq!(buffer.get(0, 0).unwrap().symbol, "h");
/// assert_eq!(buffer.get(1, 0).unwrap().style.fg, Some(Color::Red));
/// ```
#[derive(Debug, Default, Clone, Eq)]
pub struct Buffer {
    /// The rectangular region this buffer covers.
    pub area: Rect,
    /// The cells, row-major: `content[y * area.width + x]`.
    pub content: Vec<Cell>,
    /// The optional paint journal; bookkeeping only, never compared.
    journal: Option<FrameJournal>,
}

// The journal is paint bookkeeping, not content: two buffers are equal when
// they show the same cells over the same area, regardless of what any
// journal happens to have recorded.
impl PartialEq for Buffer {
    fn eq(&self, other: &Self) -> bool {
        self.area == other.area && self.content == other.content
    }
}

impl Buffer {
    /// Creates a buffer full of blank cells with the given area.
    pub fn empty(area: Rect) -> Self {
        let size = area.area() as usize;
        Self {
            area,
            content: vec![Cell::empty(); size],
            journal: None,
        }
    }

    /// Creates a buffer where every cell shows `symbol` with `style`.
    pub fn filled<T: AsRef<str>>(area: Rect, symbol: T, style: Style) -> Self {
        let mut buffer = Self::empty(area);
        for cell in &mut buffer.content {
            cell.set_symbol(&symbol);
            cell.style = style;
        }
        buffer
    }

    /// Installs a paint journal for the buffer's current area.
    ///
    /// Every subsequent mutation records the touched region until
    /// [`Buffer::clear_journal`] resets it. Used by the persistent-DOM
    /// [`crate::terminal::Terminal`] mode; plain buffers render identically
    /// without one.
    pub fn enable_journal(&mut self) {
        self.journal = Some(FrameJournal::viewport(self.area.width, self.area.height));
    }

    /// Whether a paint journal is installed.
    pub fn has_journal(&self) -> bool {
        self.journal.is_some()
    }

    /// The recorded paint bands, if a journal is installed.
    pub fn journal_bands(&self) -> &[Band] {
        self.journal.as_ref().map_or(&[], FrameJournal::bands)
    }

    /// Whether the journal recorded no paint since it was last cleared (or
    /// `true` when no journal is installed — the caller decides what an
    /// unjournaled buffer means).
    pub fn journal_is_empty(&self) -> bool {
        self.journal.as_ref().is_none_or(FrameJournal::is_empty)
    }

    /// Whether the journal decided the whole viewport changed (or `false`
    /// when no journal is installed).
    pub fn journal_is_full(&self) -> bool {
        self.journal.as_ref().is_some_and(FrameJournal::is_full)
    }

    /// Marks the whole viewport as painted (no-op without a journal).
    ///
    /// Call this after writing [`Buffer::content`] directly: slice writes
    /// bypass the mutation hooks, so the journal would otherwise miss them.
    pub fn mark_fully_painted(&mut self) {
        if let Some(journal) = self.journal.as_mut() {
            journal.mark_full();
        }
    }

    /// Clears the recorded paint (no-op without a journal).
    pub fn clear_journal(&mut self) {
        if let Some(journal) = self.journal.as_mut() {
            journal.clear();
        }
    }

    /// Records `rect` into the journal (no-op without a journal).
    fn journal_mark_rect(&mut self, rect: Rect) {
        if let Some(journal) = self.journal.as_mut() {
            journal.mark_rect(rect);
        }
    }

    /// Resets every cell to blank.
    pub fn reset(&mut self) {
        for cell in &mut self.content {
            cell.reset();
        }
        self.journal_mark_rect(self.area);
    }

    /// Clears every cell in `area`, keeping wide-glyph pairs consistent.
    ///
    /// Unlike resetting cells individually through [`Buffer::get_mut`], this
    /// repairs pair coverage so no orphaned covered cells remain.
    pub fn clear(&mut self, area: Rect) {
        let Some(area) = self.area.intersection(area) else {
            return;
        };
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                self.clear_with_pair_repair(x, y);
            }
        }
        self.journal_mark_rect(area);
    }

    /// Resizes the buffer, losing content when the area changes.
    pub fn resize(&mut self, area: Rect) {
        if self.area != area {
            self.area = area;
            self.content = vec![Cell::empty(); area.area() as usize];
            // Content was lost: the next frame must repaint from scratch.
            if let Some(journal) = self.journal.as_mut() {
                *journal = FrameJournal::viewport(area.width, area.height);
                journal.mark_full();
            }
        }
    }

    /// The size of the buffer.
    pub fn size(&self) -> Size {
        self.area.size()
    }

    /// Flattened index of `position`, or `None` when it lies outside the area.
    pub fn index_of(&self, position: Position) -> Option<usize> {
        if !self.area.contains(position) {
            return None;
        }
        Some(position.y as usize * self.area.width as usize + position.x as usize)
    }

    /// Position of a flattened `index`, saturating at the area edges.
    pub fn position_of(&self, index: usize) -> Position {
        let width = self.area.width.max(1) as usize;
        Position::new((index % width) as u16, (index / width) as u16)
    }

    /// Returns a shared reference to the cell at `(x, y)`, if inside.
    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        self.content.get(self.index_of(Position::new(x, y))?)
    }

    /// Returns an exclusive reference to the cell at `(x, y)`, if inside.
    ///
    /// The cell is recorded in the journal before it is handed out: callers
    /// may mutate through the reference freely, and the persistent-DOM
    /// terminal still sees the region as painted.
    pub fn get_mut(&mut self, x: u16, y: u16) -> Option<&mut Cell> {
        let index = self.index_of(Position::new(x, y))?;
        if let Some(journal) = self.journal.as_mut() {
            journal.mark_cell(x, y);
        }
        self.content.get_mut(index)
    }

    /// Layers `style` over every cell in `area`.
    pub fn set_style(&mut self, area: Rect, style: Style) {
        if style == Style::new() {
            return;
        }
        let Some(area) = self.area.intersection(area) else {
            return;
        };
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                if let Some(cell) = self.get_mut(x, y) {
                    cell.set_style(style);
                }
            }
        }
    }

    /// Writes `string` at `(x, y)` with `style`, clipped to the buffer.
    ///
    /// Returns the position right after the last written cell.
    pub fn set_string<T: AsRef<str>>(
        &mut self,
        x: u16,
        y: u16,
        string: T,
        style: Style,
    ) -> (u16, u16) {
        self.set_stringn(x, y, string, self.area.right().saturating_sub(x), style)
    }

    /// Writes `string` at `(x, y)` with `style` and a hyperlink on every
    /// written cell, clipped to the buffer.
    ///
    /// The hyperlink is an [`OSC 8`](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda)
    /// attribute: terminals that support it make each written position
    /// clickable. The covered half of a wide glyph carries the link too so
    /// the attribute survives pair repair.
    ///
    /// Returns the position right after the last written cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuvix_core::buffer::Buffer;
    /// use tuvix_core::geometry::Rect;
    /// use tuvix_core::style::{Hyperlink, Style};
    ///
    /// let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 1));
    /// buffer.set_string_with_hyperlink(
    ///     0,
    ///     0,
    ///     "docs",
    ///     Style::new(),
    ///     Hyperlink::new("https://tuvix.dev/docs"),
    /// );
    /// let cell = buffer.get(0, 0).unwrap();
    /// assert_eq!(cell.hyperlink.as_ref().map(Hyperlink::as_str),
    ///     Some("https://tuvix.dev/docs"));
    /// ```
    pub fn set_string_with_hyperlink<T: AsRef<str>>(
        &mut self,
        x: u16,
        y: u16,
        string: T,
        style: Style,
        hyperlink: impl Into<Hyperlink>,
    ) -> (u16, u16) {
        self.set_stringn_with_hyperlink(
            x,
            y,
            string,
            self.area.right().saturating_sub(x),
            style,
            Some(&hyperlink.into()),
        )
    }

    /// Sets (or clears with `None`) the hyperlink of the cell at `(x, y)`.
    ///
    /// Returns the position of the affected cell, or `None` when it is
    /// outside the buffer.
    pub fn set_hyperlink(
        &mut self,
        x: u16,
        y: u16,
        hyperlink: Option<Hyperlink>,
    ) -> Option<Position> {
        let cell = self.get_mut(x, y)?;
        cell.set_hyperlink(hyperlink);
        Some(Position::new(x, y))
    }

    /// Writes at most `max_width` columns of `string` at `(x, y)` with
    /// `style`, clipped to the buffer.
    ///
    /// Wide glyphs are never split: a glyph that would cross the limit or the
    /// buffer edge stops the write. Returns the position right after the last
    /// written cell.
    pub fn set_stringn<T: AsRef<str>>(
        &mut self,
        x: u16,
        y: u16,
        string: T,
        max_width: u16,
        style: Style,
    ) -> (u16, u16) {
        self.set_stringn_with_hyperlink(x, y, string, max_width, style, None)
    }

    /// Shared write path for [`Buffer::set_string`],
    /// [`Buffer::set_stringn`] and [`Buffer::set_string_with_hyperlink`].
    fn set_stringn_with_hyperlink<T: AsRef<str>>(
        &mut self,
        x: u16,
        y: u16,
        string: T,
        max_width: u16,
        style: Style,
        hyperlink: Option<&Hyperlink>,
    ) -> (u16, u16) {
        let mut cursor_x = x;
        let mut remaining = max_width;
        for grapheme in string.as_ref().graphemes(true) {
            let width = grapheme.width();
            if grapheme.chars().any(char::is_control) || width == 0 {
                continue;
            }
            if width > remaining as usize {
                break;
            }
            // Terminal pair repair: overwriting either half of a wide glyph
            // clears the whole pair on a real terminal, so the logical buffer
            // mirrors that to stay consistent with what is shown.
            let was_wide = self.get(cursor_x, y).is_some_and(Cell::is_wide);
            let was_skip = self.get(cursor_x, y).is_some_and(|cell| cell.skip);
            if was_wide {
                self.clear_with_pair_repair(cursor_x.saturating_add(1), y);
            } else if was_skip && let Some(previous) = cursor_x.checked_sub(1) {
                self.clear_with_pair_repair(previous, y);
            }
            let Some(cell) = self.get_mut(cursor_x, y) else {
                break;
            };
            cell.set_symbol(grapheme);
            cell.set_style(style);
            cell.set_hyperlink(hyperlink.cloned());
            if width == 2 {
                cursor_x += 1;
                remaining -= 2;
                // The cell about to become covered may itself be the leading
                // half of another wide pair — repair before covering it.
                self.clear_with_pair_repair(cursor_x, y);
                if let Some(trailing) = self.get_mut(cursor_x, y) {
                    trailing.reset();
                    trailing.set_hyperlink(hyperlink.cloned());
                    trailing.skip = true;
                }
            } else {
                remaining -= 1;
            }
            cursor_x += 1;
        }
        (cursor_x, y)
    }

    /// Resets the cell at `(x, y)`, repairing wide-glyph pairs: clearing a
    /// wide leading also clears its trailing, and clearing a covered trailing
    /// also clears its leading — mirroring what a real terminal does.
    pub fn clear_with_pair_repair(&mut self, x: u16, y: u16) {
        let Some(cell) = self.get(x, y) else { return };
        if cell.is_wide() {
            if let Some(trailing) = self.get_mut(x.saturating_add(1), y) {
                trailing.reset();
            }
            if let Some(leading) = self.get_mut(x, y) {
                leading.reset();
            }
        } else if cell.skip {
            if let Some(previous) = x.checked_sub(1)
                && let Some(leading) = self.get_mut(previous, y)
            {
                leading.reset();
            }
            if let Some(covered) = self.get_mut(x, y) {
                covered.reset();
            }
        } else if let Some(cell) = self.get_mut(x, y) {
            cell.reset();
        }
    }

    /// Computes the minimal set of changed runs between `self` (what the
    /// terminal currently shows) and `other` (the newly rendered frame).
    ///
    /// The iterator is lazy and allocation-free; each [`DiffRun`] is
    /// contiguous on a single row. See the [module docs](self) for the
    /// wide-character repaint rules.
    pub fn diff_runs<'a>(&'a self, other: &'a Buffer) -> DiffRuns<'a> {
        debug_assert_eq!(
            self.area, other.area,
            "diffing buffers with different areas is a bug"
        );
        DiffRuns {
            previous: self,
            next: other,
            row: 0,
            col: 0,
            bands: None,
            band_index: 0,
        }
    }

    /// The banded variant of [`Buffer::diff_runs`]: run starts are
    /// restricted to `bands` (the row bands a [`FrameJournal`] recorded),
    /// so rows without bands cost nothing. Everything else — run extension,
    /// elision, the wide-character pair rules — behaves exactly as in the
    /// full walk, so banded output over the same paint sequence is
    /// byte-identical to full-diff output.
    ///
    /// Soundness contract: `bands` must cover every cell mutated since
    /// `self` was last synced with `other` (the frame journal guarantees
    /// this for buffer-method mutations; direct [`Buffer::content`] slice
    /// writes must call [`Buffer::mark_fully_painted`]).
    pub fn diff_runs_banded<'a>(&'a self, other: &'a Buffer, bands: &'a [Band]) -> DiffRuns<'a> {
        debug_assert_eq!(
            self.area, other.area,
            "diffing buffers with different areas is a bug"
        );
        DiffRuns {
            previous: self,
            next: other,
            row: 0,
            col: 0,
            bands: Some(bands),
            band_index: 0,
        }
    }

    /// Flat per-cell variant of [`Buffer::diff_runs`], yielding
    /// `(x, y, &cell)` tuples; useful for simple backends and tests.
    pub fn diff<'a>(&'a self, other: &'a Buffer) -> impl Iterator<Item = (u16, u16, &'a Cell)> {
        self.diff_runs(other).flat_map(|run| {
            run.cells
                .iter()
                .enumerate()
                .map(move |(i, cell)| (run.start + i as u16, run.row, cell))
        })
    }
}

impl core::fmt::Display for Buffer {
    /// Renders the buffer as a character grid: blank cells become spaces and
    /// covered (skip) cells become `·`.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (index, cell) in self.content.iter().enumerate() {
            if index > 0 && (index as u16).is_multiple_of(self.area.width.max(1)) {
                writeln!(f)?;
            }
            if cell.skip {
                write!(f, "·")?;
            } else if cell.is_empty() {
                write!(f, " ")?;
            } else {
                write!(f, "{}", cell.symbol)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Color, Modifier};

    fn buffer_10x3() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 10, 3))
    }

    mod symbol {
        use super::*;

        /// The M1 perf contract: a symbol is 16 bytes, half of the M0
        /// `String` cell footprint, with the inline tier dominant.
        #[test]
        fn size_contract_is_pinned() {
            assert_eq!(core::mem::size_of::<Symbol>(), 16);
        }

        #[test]
        fn ascii_and_cjk_stay_inline() {
            assert!(Symbol::new("a").is_inline());
            assert!(Symbol::new("汉").is_inline());
            assert!(Symbol::new("❤️").is_inline()); // 7 bytes
            assert!(Symbol::new("🏳️‍🌈").is_inline()); // 14 bytes, exactly at the cap
            assert!(Symbol::empty().is_inline());
        }

        #[test]
        fn zwj_families_fall_to_the_boxed_tier() {
            let family = Symbol::new("👨‍👩‍👧‍👦");
            assert!(!family.is_inline());
            assert_eq!(family.as_str(), "👨‍👩‍👧‍👦");
            assert_eq!(family.width(), 2);
        }

        #[test]
        fn pathological_graphemes_survive_every_tier() {
            // 200 combining accents over one base char: far beyond every
            // fixed tier; the double-box fallback must preserve it exactly.
            let mut long = String::from("a");
            for _ in 0..200 {
                long.push('\u{301}');
            }
            let symbol = Symbol::new(&long);
            assert_eq!(symbol.as_str(), long);
            assert_eq!(symbol.width(), 1);
        }

        #[test]
        fn equality_is_byte_semantic_across_tiers() {
            assert_eq!(Symbol::new("hi"), Symbol::new("hi"));
            assert_ne!(Symbol::new("hi"), Symbol::new("ho"));
            assert_eq!(Symbol::new("汉"), "汉");
            assert_eq!("汉", Symbol::new("汉"));
            assert_eq!(Symbol::new("汉"), String::from("汉"));
            // Same glyph through the boxed tier vs a hypothetical inline
            // encoding must still compare equal via as_str.
            assert_eq!(Symbol::new("👨‍👩‍👧‍👦"), Symbol::new("👨‍👩‍👧‍👦"));
        }

        #[test]
        fn empty_symbol_is_blank_with_zero_width() {
            let empty = Symbol::empty();
            assert!(empty.is_empty());
            assert_eq!(empty.width(), 0);
            assert_eq!(empty.as_str(), "");
            assert_eq!(empty, Symbol::default());
        }

        #[test]
        fn width_fast_paths_ascii_and_still_counts_cjk() {
            assert_eq!(Symbol::new("x").width(), 1);
            assert_eq!(Symbol::new(" ").width(), 1);
            assert_eq!(Symbol::new("汉").width(), 2);
            assert_eq!(Symbol::new("🏳️‍🌈").width(), 2);
        }

        #[test]
        fn clone_is_value_semantics_in_every_tier() {
            let inline = Symbol::new("汉");
            let boxed = Symbol::new("👨‍👩‍👧‍👦");
            let mut long = String::from("a");
            for _ in 0..200 {
                long.push('\u{301}');
            }
            let huge = Symbol::new(&long);
            assert_eq!(inline.clone(), inline);
            assert_eq!(boxed.clone(), boxed);
            assert_eq!(huge.clone(), huge);
        }

        #[test]
        fn display_is_the_glyph() {
            assert_eq!(Symbol::new("汉").to_string(), "汉");
            assert_eq!(Symbol::empty().to_string(), "");
        }

        #[test]
        fn hash_matches_equality_across_tiers() {
            use std::collections::hash_map::DefaultHasher;
            let hash_of = |symbol: &Symbol| {
                let mut hasher = DefaultHasher::new();
                symbol.hash(&mut hasher);
                hasher.finish()
            };
            assert_eq!(hash_of(&Symbol::new("hi")), hash_of(&Symbol::new("hi")));
            assert_ne!(hash_of(&Symbol::new("hi")), hash_of(&Symbol::new("ho")));
        }
    }

    mod cell {
        use super::*;

        #[test]
        fn empty_is_blank() {
            let cell = Cell::empty();
            assert!(cell.is_empty());
            assert!(!cell.skip);
            assert_eq!(cell.style, Style::new());
        }

        #[test]
        fn set_symbol_replaces_content() {
            let mut cell = Cell::empty();
            cell.set_symbol("汉");
            cell.set_style(Style::new().fg(Color::Red));
            cell.set_symbol("a");
            assert_eq!(cell.symbol, "a");
            assert_eq!(cell.style.fg, Some(Color::Red));
        }

        #[test]
        fn reset_clears_everything() {
            let mut cell = Cell::empty();
            cell.set_symbol("汉");
            cell.set_style(Style::new().fg(Color::Red));
            cell.skip = true;
            cell.reset();
            assert!(cell.is_empty());
            assert!(!cell.skip);
            assert_eq!(cell.style, Style::new());
        }

        #[test]
        fn wide_detection() {
            let mut cell = Cell::empty();
            cell.set_symbol("汉");
            assert!(cell.is_wide());
            assert_eq!(cell.symbol_width(), 2);
            cell.set_symbol("a");
            assert!(!cell.is_wide());
        }
    }

    mod buffer_basics {
        use super::*;

        #[test]
        fn empty_buffer_is_all_blank() {
            let buffer = buffer_10x3();
            assert_eq!(buffer.content.len(), 30);
            assert!(buffer.content.iter().all(Cell::is_empty));
        }

        #[test]
        fn filled_buffer_shows_symbol_everywhere() {
            let buffer = Buffer::filled(Rect::new(0, 0, 4, 2), "x", Style::new().fg(Color::Red));
            assert!(
                buffer
                    .content
                    .iter()
                    .all(|c| c.symbol == "x" && c.style.fg == Some(Color::Red))
            );
        }

        #[test]
        fn reset_clears_all_cells() {
            let mut buffer = buffer_10x3();
            buffer.set_string(0, 0, "hello", Style::new());
            buffer.reset();
            assert!(buffer.content.iter().all(Cell::is_empty));
        }

        #[test]
        fn resize_rebuilds_content() {
            let mut buffer = buffer_10x3();
            buffer.set_string(0, 0, "hello", Style::new());
            buffer.resize(Rect::new(0, 0, 4, 2));
            assert_eq!(buffer.size(), Size::new(4, 2));
            assert_eq!(buffer.content.len(), 8);
            assert!(buffer.content.iter().all(Cell::is_empty));
        }

        #[test]
        fn resize_with_same_area_keeps_content() {
            let mut buffer = buffer_10x3();
            buffer.set_string(0, 0, "hello", Style::new());
            buffer.resize(Rect::new(0, 0, 10, 3));
            assert_eq!(buffer.get(0, 0).unwrap().symbol, "h");
        }

        #[test]
        fn index_of_roundtrip() {
            let buffer = buffer_10x3();
            let index = buffer.index_of(Position::new(3, 2)).unwrap();
            assert_eq!(buffer.position_of(index), Position::new(3, 2));
        }

        #[test]
        fn index_of_outside_is_none() {
            let buffer = buffer_10x3();
            assert_eq!(buffer.index_of(Position::new(10, 0)), None);
            assert_eq!(buffer.index_of(Position::new(0, 3)), None);
        }

        #[test]
        fn get_outside_is_none() {
            let mut buffer = buffer_10x3();
            assert!(buffer.get(10, 0).is_none());
            assert!(buffer.get_mut(0, 3).is_none());
        }

        #[test]
        fn display_renders_grid_with_skip_marker() {
            let mut buffer = buffer_10x3();
            buffer.set_string(0, 1, "a汉", Style::new());
            let rendered = buffer.to_string();
            let rows: Vec<&str> = rendered.lines().collect();
            assert_eq!(rows.len(), 3);
            assert_eq!(rows[1], "a汉·       ");
        }
    }

    mod set_string {
        use super::*;

        #[test]
        fn writes_ascii_with_style() {
            let mut buffer = buffer_10x3();
            let style = Style::new().fg(Color::Red);
            let (x, y) = buffer.set_string(2, 1, "hi", style);
            assert_eq!((x, y), (4, 1));
            assert_eq!(buffer.get(2, 1).unwrap().symbol, "h");
            assert_eq!(buffer.get(3, 1).unwrap().style.fg, Some(Color::Red));
        }

        #[test]
        fn clips_at_buffer_edge() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
            let (x, _) = buffer.set_string(2, 0, "hello", Style::new());
            assert_eq!(x, 4);
            assert_eq!(buffer.get(2, 0).unwrap().symbol, "h");
            assert_eq!(buffer.get(3, 0).unwrap().symbol, "e");
        }

        #[test]
        fn respects_max_width() {
            let mut buffer = buffer_10x3();
            let (x, _) = buffer.set_stringn(0, 0, "hello", 3, Style::new());
            assert_eq!(x, 3);
            assert_eq!(buffer.get(0, 0).unwrap().symbol, "h");
            assert!(buffer.get(3, 0).unwrap().is_empty());
        }

        #[test]
        fn wide_glyph_marks_trailing_cell_as_skip() {
            let mut buffer = buffer_10x3();
            buffer.set_string(0, 0, "汉a", Style::new());
            let leading = buffer.get(0, 0).unwrap();
            let trailing = buffer.get(1, 0).unwrap();
            assert_eq!(leading.symbol, "汉");
            assert!(trailing.skip);
            assert!(trailing.is_empty());
            assert_eq!(buffer.get(2, 0).unwrap().symbol, "a");
        }

        #[test]
        fn wide_glyph_never_splits_at_limit() {
            let mut buffer = buffer_10x3();
            let (x, _) = buffer.set_stringn(0, 0, "a汉", 2, Style::new());
            // 'a' fits; 汉 needs two of the one remaining column -> stop.
            assert_eq!(x, 1);
            assert!(buffer.get(1, 0).unwrap().is_empty());
        }

        #[test]
        fn skips_control_and_zero_width_characters() {
            let mut buffer = buffer_10x3();
            buffer.set_string(0, 0, "a\u{7}b\u{200b}c", Style::new());
            assert_eq!(buffer.get(0, 0).unwrap().symbol, "a");
            assert_eq!(buffer.get(1, 0).unwrap().symbol, "b");
            assert_eq!(buffer.get(2, 0).unwrap().symbol, "c");
        }

        #[test]
        fn set_style_layers_over_existing() {
            let mut buffer = buffer_10x3();
            buffer.set_string(0, 0, "abc", Style::new().fg(Color::Red));
            buffer.set_style(
                Rect::new(0, 0, 10, 1),
                Style::new().add_modifier(Modifier::BOLD),
            );
            let cell = buffer.get(0, 0).unwrap();
            assert_eq!(cell.style.fg, Some(Color::Red));
            assert!(cell.style.add_modifier.contains(Modifier::BOLD));
        }

        #[test]
        fn set_style_ignores_areas_outside() {
            let mut buffer = buffer_10x3();
            buffer.set_style(Rect::new(20, 20, 5, 5), Style::new().fg(Color::Red));
            assert!(buffer.content.iter().all(|c| c.style.fg.is_none()));
        }
    }

    mod diff {
        use super::*;

        fn assert_runs(previous: &Buffer, next: &Buffer, expected: &[(u16, u16, u16, &str, bool)]) {
            let runs: Vec<DiffRun<'_>> = previous.diff_runs(next).collect();
            let actual: Vec<(u16, u16, u16, String, bool)> = runs
                .iter()
                .map(|run| {
                    (
                        run.start,
                        run.row,
                        run.len(),
                        run.cells
                            .iter()
                            .map(|c| c.symbol.as_str())
                            .collect::<String>(),
                        run.elide_rest,
                    )
                })
                .collect();
            let expected: Vec<(u16, u16, u16, String, bool)> = expected
                .iter()
                .map(|(start, row, len, symbols, elide)| {
                    (*start, *row, *len, (*symbols).to_string(), *elide)
                })
                .collect();
            assert_eq!(actual, expected);
        }

        #[test]
        fn identical_buffers_produce_no_runs() {
            let a = buffer_10x3();
            let b = buffer_10x3();
            assert_eq!(a.diff_runs(&b).count(), 0);
        }

        #[test]
        fn single_change_is_single_run() {
            let a = buffer_10x3();
            let mut b = buffer_10x3();
            b.set_string(3, 1, "x", Style::new());
            assert_runs(&a, &b, &[(3, 1, 1, "x", false)]);
        }

        #[test]
        fn contiguous_changes_merge_into_one_run() {
            let a = buffer_10x3();
            let mut b = buffer_10x3();
            b.set_string(2, 0, "abc", Style::new());
            assert_runs(&a, &b, &[(2, 0, 3, "abc", false)]);
        }

        #[test]
        fn gap_splits_runs_on_same_row() {
            let a = buffer_10x3();
            let mut b = buffer_10x3();
            b.set_string(0, 0, "ab", Style::new());
            b.set_string(5, 0, "cd", Style::new());
            assert_runs(&a, &b, &[(0, 0, 2, "ab", false), (5, 0, 2, "cd", false)]);
        }

        #[test]
        fn style_only_change_is_emitted() {
            let mut a = buffer_10x3();
            let mut b = buffer_10x3();
            b.set_string(0, 0, "x", Style::new().fg(Color::Red));
            a.set_string(0, 0, "x", Style::new());
            assert_runs(&a, &b, &[(0, 0, 1, "x", false)]);
        }

        #[test]
        fn clearing_to_blank_elides_with_erase_line() {
            let mut a = buffer_10x3();
            let b = buffer_10x3();
            a.set_string(0, 0, "old", Style::new());
            // The whole tail is blank: one zero-length elided run instead
            // of three space writes.
            assert_runs(&a, &b, &[(0, 0, 0, "", true)]);
        }

        #[test]
        fn changed_wide_glyph_repaints_both_halves() {
            let mut a = buffer_10x3();
            let mut b = buffer_10x3();
            a.set_string(0, 0, "汉", Style::new());
            b.set_string(0, 0, "字", Style::new());
            // The leading cell changed and forces the (unchanged) trailing
            // cell into the same run: 2 cells whose joined symbols are 字.
            assert_runs(&a, &b, &[(0, 0, 2, "字", false)]);
        }

        #[test]
        fn narrow_over_wide_repaints_covered_cell() {
            let mut a = buffer_10x3();
            let mut b = buffer_10x3();
            a.set_string(0, 0, "汉", Style::new());
            b.set_string(0, 0, "ab", Style::new());
            assert_runs(&a, &b, &[(0, 0, 2, "ab", false)]);
        }

        #[test]
        fn wide_over_narrow_forces_trailing_too() {
            let mut a = buffer_10x3();
            let mut b = buffer_10x3();
            a.set_string(0, 0, "ab", Style::new());
            b.set_string(0, 0, "汉", Style::new());
            // Leading changed; the trailing half is forced into the run so the
            // wide pair is always written atomically (trailing is skip/empty).
            assert_runs(&a, &b, &[(0, 0, 2, "汉", false)]);
        }

        #[test]
        fn unchanged_trailing_not_emitted_beyond_pair() {
            let mut a = buffer_10x3();
            let mut b = buffer_10x3();
            a.set_string(0, 0, "汉a", Style::new());
            b.set_string(0, 0, "汉b", Style::new());
            // Stable wide pair stays untouched; only the cell after it is
            // emitted. This proves skip cells are not emitted standalone.
            assert_runs(&a, &b, &[(2, 0, 1, "b", false)]);
        }

        #[test]
        fn changed_leading_also_emits_trailing() {
            let mut a = buffer_10x3();
            let mut b = buffer_10x3();
            a.set_string(0, 0, "汉", Style::new());
            b.set_string(0, 0, "汉", Style::new().fg(Color::Red));
            // Only the leading cell changed in style; the trailing cell is
            // emitted purely as part of the forced pair (trailing = skip/empty).
            assert_runs(&a, &b, &[(0, 0, 2, "汉", false)]);
        }

        #[test]
        fn runs_across_rows_yield_in_order() {
            let a = buffer_10x3();
            let mut b = buffer_10x3();
            b.set_string(0, 0, "row0", Style::new());
            b.set_string(0, 2, "row2", Style::new());
            let runs: Vec<DiffRun<'_>> = a.diff_runs(&b).collect();
            assert_eq!(runs.len(), 2);
            assert_eq!((runs[0].row, runs[0].start, runs[0].len()), (0, 0, 4));
            assert_eq!((runs[1].row, runs[1].start, runs[1].len()), (2, 0, 4));
        }

        #[test]
        fn diff_flat_variant_matches_runs() {
            let a = buffer_10x3();
            let mut b = buffer_10x3();
            b.set_string(1, 1, "hey", Style::new());
            let flat: Vec<(u16, u16)> = a.diff(&b).map(|(x, y, _)| (x, y)).collect();
            assert_eq!(flat, vec![(1, 1), (2, 1), (3, 1)]);
        }
    }

    mod hyperlink {
        use super::*;
        use crate::style::Hyperlink;

        #[allow(clippy::unnecessary_wraps)] // mirrors the Option<Hyperlink> field
        fn link(uri: &str) -> Option<Hyperlink> {
            Some(Hyperlink::new(uri))
        }

        #[test]
        fn empty_cell_has_no_hyperlink() {
            assert_eq!(Cell::empty().hyperlink, None);
        }

        #[test]
        fn set_string_with_hyperlink_stamps_every_written_cell() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
            buffer.set_string_with_hyperlink(
                1,
                0,
                "docs",
                Style::new(),
                Hyperlink::new("https://tuvix.dev"),
            );
            for x in 1..5 {
                let cell = buffer.get(x, 0).unwrap();
                assert_eq!(cell.hyperlink, link("https://tuvix.dev"));
            }
            // Cells before and after the write stay clean.
            assert_eq!(buffer.get(0, 0).unwrap().hyperlink, None);
            assert_eq!(buffer.get(5, 0).unwrap().hyperlink, None);
        }

        #[test]
        fn wide_glyph_pair_carries_the_hyperlink() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
            buffer.set_string_with_hyperlink(
                0,
                0,
                "汉字",
                Style::new(),
                Hyperlink::new("https://tuvix.dev"),
            );
            for x in 0..4 {
                let cell = buffer.get(x, 0).unwrap();
                assert_eq!(cell.hyperlink, link("https://tuvix.dev"));
            }
        }

        #[test]
        fn set_hyperlink_updates_one_cell_and_reports_its_position() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
            assert_eq!(
                buffer.set_hyperlink(2, 1, link("https://tuvix.dev")),
                Some(Position::new(2, 1))
            );
            assert_eq!(
                buffer.get(2, 1).unwrap().hyperlink,
                link("https://tuvix.dev")
            );
            // Outside the buffer: nothing happens, `None` comes back.
            assert_eq!(buffer.set_hyperlink(9, 9, link("https://tuvix.dev")), None);
            // Clearing works too.
            assert_eq!(buffer.set_hyperlink(2, 1, None), Some(Position::new(2, 1)));
            assert_eq!(buffer.get(2, 1).unwrap().hyperlink, None);
        }

        #[test]
        fn hyperlink_is_content_for_the_diff() {
            let a = Buffer::empty(Rect::new(0, 0, 4, 1));
            let mut b = Buffer::empty(Rect::new(0, 0, 4, 1));
            // Same blank grid, but the hyperlink changed: the diff must
            // emit the cell so the terminal's link state follows.
            b.set_hyperlink(2, 0, link("https://tuvix.dev")).unwrap();
            let runs: Vec<DiffRun<'_>> = a.diff_runs(&b).collect();
            assert_eq!(runs.len(), 1);
            assert_eq!((runs[0].row, runs[0].start, runs[0].len()), (0, 2, 1));

            // And the reverse direction emits too (link closed).
            let runs: Vec<DiffRun<'_>> = b.diff_runs(&a).collect();
            assert_eq!(runs.len(), 1);
            assert_eq!((runs[0].row, runs[0].start, runs[0].len()), (0, 2, 1));
        }

        #[test]
        fn different_hyperlinks_emit_a_run() {
            let mut a = Buffer::empty(Rect::new(0, 0, 4, 1));
            let mut b = Buffer::empty(Rect::new(0, 0, 4, 1));
            a.set_hyperlink(0, 0, link("https://a.dev")).unwrap();
            b.set_hyperlink(0, 0, link("https://b.dev")).unwrap();
            assert_eq!(a.diff_runs(&b).count(), 1);
        }

        #[test]
        fn same_hyperlink_does_not_emit() {
            let mut a = Buffer::empty(Rect::new(0, 0, 4, 1));
            let mut b = Buffer::empty(Rect::new(0, 0, 4, 1));
            a.set_hyperlink(0, 0, link("https://a.dev")).unwrap();
            b.set_hyperlink(0, 0, link("https://a.dev")).unwrap();
            assert_eq!(a.diff_runs(&b).count(), 0);
        }

        #[test]
        fn hyperlink_tails_are_never_elided() {
            // "ab" + a hyperlink-only tail: two runs — the glyphs, then the
            // linked blank cell. The linked cell has to be written so the
            // terminal's link state is established, and no EL may swallow it.
            let a = Buffer::empty(Rect::new(0, 0, 8, 1));
            let mut b = Buffer::empty(Rect::new(0, 0, 8, 1));
            b.set_string(0, 0, "ab", Style::new());
            b.set_hyperlink(4, 0, link("https://tuvix.dev")).unwrap();
            let runs: Vec<DiffRun<'_>> = a.diff_runs(&b).collect();
            assert_eq!(runs.len(), 2);
            assert_eq!((runs[0].start, runs[0].len()), (0, 2));
            assert!(!runs[0].elide_rest);
            assert_eq!((runs[1].start, runs[1].len()), (4, 1));
            assert!(!runs[1].elide_rest);
        }

        #[test]
        fn hyperlink_cells_survive_reset_only_through_reset() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
            buffer.set_string_with_hyperlink(
                0,
                0,
                "x",
                Style::new(),
                Hyperlink::new("https://tuvix.dev"),
            );
            buffer.get_mut(0, 0).unwrap().reset();
            assert_eq!(buffer.get(0, 0).unwrap().hyperlink, None);
        }
    }
}
