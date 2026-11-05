//! TextArea: a multi-line text editor widget with bounded undo/redo.
//!
//! The text, cursor, scroll window and the undo/redo stacks live in
//! [`TextAreaState`]; the [`TextArea`] widget is the paint configuration
//! (style, block). The editing model generalizes the M6 [`crate::input::Input`]
//! decisions to many lines:
//!
//! - the cursor is **char-indexed** — it can never split a grapheme —
//!   while the horizontal window syncs on display *columns*, so a 2-wide
//!   glyph near the edge scrolls like two cells;
//! - vertical motion keeps a **goal column**: moving through shorter
//!   lines clamps, but the next longer line restores the column until a
//!   horizontal motion or an edit resets it;
//! - every edit records an inverse operation, so [`TextAreaState::undo`]
//!   and [`TextAreaState::redo`] are exact; typing runs coalesce into one
//!   step (a whole word deletes with a single undo), and the stack is
//!   bounded by [`TextAreaState::set_undo_limit`].
//!
//! The terminal cursor stays under the application's control: the widget
//! reports where it belongs through [`TextArea::cursor_position`], and
//! the app feeds that to `Frame::set_cursor_position` while the area is
//! focused.

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Position, Rect};
use tuvix_core::style::Style;
use tuvix_core::widget::RenderWidget;

use unicode_width::UnicodeWidthChar as _;

use crate::block::Block;

/// The default bound on the undo stack (and therefore the redo stack):
/// 128 editing steps, enough for a session of typing without growing
/// with the document.
const DEFAULT_UNDO_LIMIT: usize = 128;

/// One inverse-editable operation on the undo stack.
#[derive(Debug, Clone, PartialEq, Eq)]
enum UndoOp {
    /// `text` (possibly multi-line: contains `\n`) was inserted at
    /// `(row, col)`. Undo removes it; redo re-inserts it.
    Insert {
        row: usize,
        col: usize,
        text: String,
    },
    /// `text` (one line only) was deleted at `(row, col)`. Undo
    /// re-inserts it; redo deletes it again.
    Delete {
        row: usize,
        col: usize,
        text: String,
    },
    /// The line break between `row` and `row + 1` was removed at column
    /// `col` of `row` (the lines were joined). Undo splits again; redo
    /// joins again.
    Join { row: usize, col: usize },
    /// The line `row` was split at column `col` (a new line `row + 1`
    /// starts with the tail). Undo joins again; redo splits again.
    Split { row: usize, col: usize },
    /// The line `row` was removed entirely (its `text` kept for the
    /// undo). Undo restores it; redo erases it again.
    EraseLine { row: usize, text: String },
}

/// One undo step: the operation, the cursor on both sides of it, and the
/// coalescing group it belongs to (steps of one group merge; undo/redo
/// open a new group so a fresh edit never merges with older history).
#[derive(Debug, Clone, PartialEq, Eq)]
struct UndoStep {
    group: u64,
    op: UndoOp,
    before: (usize, usize),
    after: (usize, usize),
}

/// The editing state of a [`TextArea`]: lines, cursor, scroll window and
/// the undo/redo history.
///
/// # Examples
///
/// ```
/// use tuvix_widgets::TextAreaState;
///
/// let mut state = TextAreaState::new();
/// state.insert_str("hello");
/// state.new_line();
/// state.insert_str("world");
/// assert_eq!(state.text(), "hello\nworld");
/// state.undo();
/// assert_eq!(state.text(), "hello\n");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextAreaState {
    /// The document lines; never empty (at least one, possibly empty).
    lines: Vec<String>,
    /// Cursor row (line index).
    row: usize,
    /// Cursor column in char index within the row.
    col: usize,
    /// Display-column goal kept across vertical motions.
    goal_col: Option<usize>,
    /// First visible row.
    row_offset: usize,
    /// First visible char (column window start) per row.
    col_offset: usize,
    /// Bounded undo stack (oldest first).
    undo: Vec<UndoStep>,
    /// Redo stack (popped by redo, cleared by any new edit).
    redo: Vec<UndoStep>,
    /// Maximum undo steps retained.
    undo_limit: usize,
    /// Coalescing group of the next edit (bumped by undo/redo).
    group: u64,
}

impl Default for TextAreaState {
    fn default() -> Self {
        Self::new()
    }
}

/// Byte offset of a char index within `line` (`line.len()` past the end).
fn byte_of(line: &str, char_index: usize) -> usize {
    line.char_indices()
        .nth(char_index)
        .map_or(line.len(), |(byte, _)| byte)
}

/// The display column where the `index`-th char of `line` starts (control
/// characters count zero columns, matching the [`crate::input::Input`] model).
fn display_column_of_char_index(line: &str, index: usize) -> usize {
    line.chars()
        .take(index)
        .map(|ch| ch.width().unwrap_or(0))
        .sum()
}

/// The char index the cursor lands on when moving to display column
/// `target` within `line`: the glyph covering the column (a wide glyph
/// takes the cursor when the target falls inside it), clamped to the
/// line end.
fn char_index_at_display_column(line: &str, target: usize) -> usize {
    let mut col = 0;
    for (index, ch) in line.chars().enumerate() {
        if col >= target {
            return index;
        }
        let width = ch.width().unwrap_or(0);
        if col + width > target {
            return index;
        }
        col += width;
    }
    line.chars().count()
}

impl TextAreaState {
    /// An empty one-line document with the cursor at the start.
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
            goal_col: None,
            row_offset: 0,
            col_offset: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            undo_limit: DEFAULT_UNDO_LIMIT,
            group: 0,
        }
    }

    /// The document lines. At least one line exists; an empty document is
    /// one empty line, not zero lines.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The document as one string, lines joined with `\n`.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// The cursor as `(row, col)`: the row index and the char index
    /// within that row.
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// The first visible row (vertical scroll).
    pub fn row_offset(&self) -> usize {
        self.row_offset
    }

    /// The first visible char of every row (horizontal scroll, char
    /// indexed like the cursor).
    pub fn col_offset(&self) -> usize {
        self.col_offset
    }

    /// The display column the cursor occupies within its row (the width
    /// of the chars before it; control characters count zero).
    pub fn visible_cursor_column(&self) -> usize {
        display_column_of_char_index(&self.lines[self.row], self.col)
    }

    /// Whether an undo step is available.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether a redo step is available.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Bounds the undo history to `limit` steps (oldest dropped first).
    /// A limit of 0 disables undo entirely. Bulk replaces (`set_text`,
    /// [`Self::clear`]) reset the history instead of recording.
    pub fn set_undo_limit(&mut self, limit: usize) {
        self.undo_limit = limit;
        if self.undo.len() > limit {
            let excess = self.undo.len() - limit;
            self.undo.drain(0..excess);
        }
        if limit == 0 {
            self.redo.clear();
        }
    }

    /// Replaces the whole document: `text` splits on `\n` (an empty
    /// string is one empty line). The cursor and the undo history reset.
    pub fn set_text(&mut self, text: &str) {
        self.lines = if text.is_empty() {
            vec![String::new()]
        } else {
            text.split('\n').map(str::to_owned).collect()
        };
        self.row = 0;
        self.col = 0;
        self.goal_col = None;
        self.row_offset = 0;
        self.col_offset = 0;
        self.undo.clear();
        self.redo.clear();
    }

    /// Clears the document and the undo history.
    pub fn clear(&mut self) {
        self.set_text("");
    }

    // --- edit primitives -------------------------------------------------

    /// Builds an undo step in the current coalescing group (`push_undo`
    /// stamps the actual group).
    fn step(op: UndoOp, before: (usize, usize), after: (usize, usize)) -> UndoStep {
        UndoStep {
            group: 0,
            op,
            before,
            after,
        }
    }

    /// Records `step` on the undo stack, coalescing typing runs (a char
    /// inserted right after a previous insert extends that step; single
    /// char deletes chain the same way), then clears the redo stack.
    fn push_undo(&mut self, mut step: UndoStep) {
        step.group = self.group;
        if let Some(top) = self.undo.last_mut()
            && top.group == self.group
        {
            match (&mut top.op, &step.op) {
                (
                    UndoOp::Insert {
                        row: r1,
                        col: c1,
                        text: t1,
                    },
                    UndoOp::Insert {
                        row: r2,
                        col: c2,
                        text: t2,
                    },
                ) => {
                    let contiguous = r1 == r2 && *c1 + t1.chars().count() == *c2;
                    let single_line = !t1.contains('\n') && !t2.contains('\n');
                    if contiguous && single_line {
                        t1.push_str(t2);
                        top.after = step.after;
                        return;
                    }
                }
                (
                    UndoOp::Delete {
                        row: r1,
                        col: c1,
                        text: t1,
                    },
                    UndoOp::Delete {
                        row: r2,
                        col: c2,
                        text: t2,
                    },
                ) if r1 == r2 && t2.chars().count() == 1 && !t1.contains('\n') => {
                    if c2 == c1 {
                        // forward chain: the runs concatenate in order
                        t1.push_str(t2);
                        top.after = step.after;
                        return;
                    }
                    if *c2 + 1 == *c1 {
                        // backward chain: the new char sits before the run
                        let merged = format!("{t2}{t1}");
                        *t1 = merged;
                        *c1 = *c2;
                        top.after = step.after;
                        return;
                    }
                }
                _ => {}
            }
        }
        self.undo.push(step);
        if self.undo.len() > self.undo_limit {
            let excess = self.undo.len() - self.undo_limit;
            self.undo.drain(0..excess);
        }
        self.redo.clear();
    }

    /// Inserts `text` at `(row, col)` without touching cursor or history
    /// (the undo engine's shared primitive).
    fn insert_text_at(&mut self, row: usize, col: usize, text: &str) {
        let line = std::mem::take(&mut self.lines[row]);
        let byte = byte_of(&line, col);
        let (head, tail) = (&line[..byte], &line[byte..]);
        let segments: Vec<&str> = text.split('\n').collect();
        if segments.len() == 1 {
            self.lines[row] = format!("{head}{text}{tail}");
        } else {
            self.lines[row] = format!("{head}{}", segments[0]);
            let mut new_rows: Vec<String> = segments[1..]
                .iter()
                .map(|segment| (*segment).to_owned())
                .collect();
            let last = new_rows.last_mut().expect("split always yields > 1 row");
            last.push_str(tail);
            for (offset, new_row) in new_rows.into_iter().enumerate() {
                self.lines.insert(row + 1 + offset, new_row);
            }
        }
    }

    /// Removes the `text` (possibly multi-line, previously inserted at
    /// `(row, col)`) again — the exact inverse of [`Self::insert_text_at`].
    fn remove_text_at(&mut self, row: usize, col: usize, text: &str) {
        let segments: Vec<&str> = text.split('\n').collect();
        let spans = segments.len();
        let line = std::mem::take(&mut self.lines[row]);
        let start = byte_of(&line, col);
        let end = byte_of(&line, col + segments[0].chars().count());
        let mut rebuilt = String::with_capacity(line.len());
        rebuilt.push_str(&line[..start]);
        rebuilt.push_str(&line[end..]);
        if spans > 1 {
            // the tail lives at the front of the last inserted row
            let tail_row = &self.lines[row + spans - 1];
            let tail_byte = byte_of(tail_row, segments[spans - 1].chars().count());
            rebuilt.push_str(&tail_row[tail_byte..]);
        }
        self.lines[row] = rebuilt;
        for offset in (1..spans).rev() {
            self.lines.remove(row + offset);
        }
    }

    /// Joins `row + 1` into `row` (removes the line break). Inverse of
    /// [`Self::split_at`].
    fn join_at(&mut self, row: usize) {
        let next = std::mem::take(&mut self.lines[row + 1]);
        self.lines[row].push_str(&next);
        self.lines.remove(row + 1);
    }

    /// Removes the line `row` entirely, or clears it when it is the only
    /// line (the document never drops below one line). The shared
    /// primitive of [`Self::delete_line`] and the redo of `EraseLine`.
    fn erase_row(&mut self, row: usize) {
        if self.lines.len() > 1 {
            let _ = std::mem::take(&mut self.lines[row]);
            self.lines.remove(row);
        } else {
            self.lines[0] = String::new();
        }
    }

    /// Splits `row` at column `col` (starts `row + 1` with the tail).
    /// Inverse of [`Self::join_at`].
    fn split_at(&mut self, row: usize, col: usize) {
        let line = std::mem::take(&mut self.lines[row]);
        let byte = byte_of(&line, col);
        line[..byte].clone_into(&mut self.lines[row]);
        self.lines.insert(row + 1, line[byte..].to_owned());
    }

    /// Inserts one char at the cursor.
    pub fn insert(&mut self, character: char) {
        self.insert_str(&character.to_string());
    }

    /// Inserts text at the cursor; `\n` characters split lines. The
    /// cursor ends after the inserted text. Consecutive inserts at the
    /// cursor coalesce into one undo step.
    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let (row, col) = (self.row, self.col);
        let before = self.cursor();
        self.insert_text_at(row, col, text);
        let segments = text.split('\n').count();
        if segments == 1 {
            self.col += text.chars().count();
        } else {
            self.row = row + segments - 1;
            self.col = text
                .rsplit('\n')
                .next()
                .map_or(0, |tail| tail.chars().count());
        }
        self.goal_col = None;
        let after = self.cursor();
        self.push_undo(Self::step(
            UndoOp::Insert {
                row,
                col,
                text: text.to_owned(),
            },
            before,
            after,
        ));
    }

    /// Deletes the char before the cursor (backspace). At the start of a
    /// line it removes the line break (joins with the previous line).
    pub fn delete_backward(&mut self) {
        let (row, col) = (self.row, self.col);
        let before = self.cursor();
        if col > 0 {
            let line = std::mem::take(&mut self.lines[row]);
            let start = byte_of(&line, col - 1);
            let end = byte_of(&line, col);
            let removed = line[start..end].to_owned();
            self.lines[row] = format!("{}{}", &line[..start], &line[end..]);
            self.col -= 1;
            self.goal_col = None;
            let after = self.cursor();
            self.push_undo(Self::step(
                UndoOp::Delete {
                    row,
                    col: col - 1,
                    text: removed,
                },
                before,
                after,
            ));
        } else if row > 0 {
            let previous_length = self.lines[row - 1].chars().count();
            let current = std::mem::take(&mut self.lines[row]);
            self.lines[row - 1].push_str(&current);
            self.lines.remove(row);
            self.row = row - 1;
            self.col = previous_length;
            self.goal_col = None;
            let after = self.cursor();
            self.push_undo(Self::step(
                UndoOp::Join {
                    row: row - 1,
                    col: previous_length,
                },
                before,
                after,
            ));
        }
    }

    /// Deletes the char at the cursor (delete key). At the end of a line
    /// it removes the line break (joins with the next line).
    pub fn delete_forward(&mut self) {
        let (row, col) = (self.row, self.col);
        let before = self.cursor();
        let length = self.lines[row].chars().count();
        if col < length {
            let line = std::mem::take(&mut self.lines[row]);
            let start = byte_of(&line, col);
            let end = byte_of(&line, col + 1);
            let removed = line[start..end].to_owned();
            self.lines[row] = format!("{}{}", &line[..start], &line[end..]);
            self.goal_col = None;
            let after = self.cursor();
            self.push_undo(Self::step(
                UndoOp::Delete {
                    row,
                    col,
                    text: removed,
                },
                before,
                after,
            ));
        } else if row + 1 < self.lines.len() {
            let next = std::mem::take(&mut self.lines[row + 1]);
            self.lines[row].push_str(&next);
            self.lines.remove(row + 1);
            self.goal_col = None;
            let after = self.cursor();
            self.push_undo(Self::step(UndoOp::Join { row, col }, before, after));
        }
    }

    /// Deletes the word left of the cursor (alphanumeric runs are words,
    /// separators skip back first, the [`crate::input::Input`] semantics). At the start
    /// of a line it removes the line break instead.
    pub fn delete_word_backward(&mut self) {
        let (row, col) = (self.row, self.col);
        if col == 0 {
            self.delete_backward();
            return;
        }
        let line = self.lines[row].clone();
        let chars: Vec<char> = line.chars().collect();
        let mut index = col.min(chars.len());
        while index > 0 && !chars[index - 1].is_alphanumeric() {
            index -= 1;
        }
        while index > 0 && chars[index - 1].is_alphanumeric() {
            index -= 1;
        }
        if index == col {
            return;
        }
        let before = self.cursor();
        let start = byte_of(&line, index);
        let end = byte_of(&line, col);
        let removed = line[start..end].to_owned();
        self.lines[row] = format!("{}{}", &line[..start], &line[end..]);
        self.col = index;
        self.goal_col = None;
        let after = self.cursor();
        self.push_undo(Self::step(
            UndoOp::Delete {
                row,
                col: index,
                text: removed,
            },
            before,
            after,
        ));
    }

    /// Deletes the word right of the cursor (the [`crate::input::Input`] semantics). At
    /// the end of a line it removes the line break instead.
    pub fn delete_word_forward(&mut self) {
        let (row, col) = (self.row, self.col);
        let length = self.lines[row].chars().count();
        if col >= length {
            self.delete_forward();
            return;
        }
        let line = self.lines[row].clone();
        let chars: Vec<char> = line.chars().collect();
        let mut index = col;
        while index < chars.len() && !chars[index].is_alphanumeric() {
            index += 1;
        }
        while index < chars.len() && chars[index].is_alphanumeric() {
            index += 1;
        }
        if index == col {
            return;
        }
        let before = self.cursor();
        let start = byte_of(&line, col);
        let end = byte_of(&line, index);
        let removed = line[start..end].to_owned();
        self.lines[row] = format!("{}{}", &line[..start], &line[end..]);
        self.goal_col = None;
        let after = self.cursor();
        self.push_undo(Self::step(
            UndoOp::Delete {
                row,
                col,
                text: removed,
            },
            before,
            after,
        ));
    }

    /// Splits the current line at the cursor (the Enter key).
    pub fn new_line(&mut self) {
        let (row, col) = (self.row, self.col);
        let before = self.cursor();
        self.split_at(row, col);
        self.row = row + 1;
        self.col = 0;
        self.goal_col = None;
        let after = self.cursor();
        self.push_undo(Self::step(UndoOp::Split { row, col }, before, after));
    }

    /// Removes the current line. On the only line it clears that line
    /// instead (the document never drops below one line).
    pub fn delete_line(&mut self) {
        let row = self.row;
        let text = self.lines[row].clone();
        if self.lines.len() == 1 {
            if text.is_empty() {
                return; // clearing an empty document is a no-op, not a step
            }
            // On the only line the erase is an in-line delete of the whole
            // content: the document never drops below one line, so the
            // undo must re-insert into that line, not add a new one.
            let before = self.cursor();
            self.lines[0] = String::new();
            self.col = 0;
            self.goal_col = None;
            let after = self.cursor();
            self.push_undo(Self::step(
                UndoOp::Delete {
                    row: 0,
                    col: 0,
                    text,
                },
                before,
                after,
            ));
            return;
        }
        let before = self.cursor();
        self.erase_row(row);
        self.row = self.row.min(self.lines.len() - 1);
        self.col = 0;
        self.goal_col = None;
        let after = self.cursor();
        self.push_undo(Self::step(UndoOp::EraseLine { row, text }, before, after));
    }

    // --- motions ---------------------------------------------------------

    /// Moves one char left; at a line start it wraps to the end of the
    /// previous line.
    pub fn move_left(&mut self) {
        self.goal_col = None;
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.lines[self.row].chars().count();
        }
    }

    /// Moves one char right; at a line end it wraps to the start of the
    /// next line.
    pub fn move_right(&mut self) {
        self.goal_col = None;
        if self.col < self.lines[self.row].chars().count() {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    /// Moves one row up, restoring the goal display column (clamped to
    /// the row's end). A no-op on the first row.
    pub fn move_up(&mut self) {
        if self.row == 0 {
            return;
        }
        let goal = self
            .goal_col
            .unwrap_or_else(|| display_column_of_char_index(&self.lines[self.row], self.col));
        self.row -= 1;
        self.col = char_index_at_display_column(&self.lines[self.row], goal);
        self.goal_col = Some(goal);
    }

    /// Moves one row down, restoring the goal display column (clamped to
    /// the row's end). A no-op on the last row.
    pub fn move_down(&mut self) {
        if self.row + 1 >= self.lines.len() {
            return;
        }
        let goal = self
            .goal_col
            .unwrap_or_else(|| display_column_of_char_index(&self.lines[self.row], self.col));
        self.row += 1;
        self.col = char_index_at_display_column(&self.lines[self.row], goal);
        self.goal_col = Some(goal);
    }

    /// Moves to the start of the current line.
    pub fn move_home(&mut self) {
        self.goal_col = None;
        self.col = 0;
    }

    /// Moves to the end of the current line.
    pub fn move_end(&mut self) {
        self.goal_col = None;
        self.col = self.lines[self.row].chars().count();
    }

    /// Moves to the very start of the document.
    pub fn move_doc_start(&mut self) {
        self.goal_col = None;
        self.row = 0;
        self.col = 0;
    }

    /// Moves to the very end of the document.
    pub fn move_doc_end(&mut self) {
        self.goal_col = None;
        self.row = self.lines.len() - 1;
        self.col = self.lines[self.row].chars().count();
    }

    /// Moves to the start of the word left of the cursor (alphanumeric
    /// runs are words); at a line start it wraps to the previous line.
    pub fn move_word_left(&mut self) {
        self.goal_col = None;
        if self.col == 0 {
            if self.row > 0 {
                self.row -= 1;
                self.col = self.lines[self.row].chars().count();
            }
            return;
        }
        let chars: Vec<char> = self.lines[self.row].chars().collect();
        let mut index = self.col.min(chars.len());
        while index > 0 && !chars[index - 1].is_alphanumeric() {
            index -= 1;
        }
        while index > 0 && chars[index - 1].is_alphanumeric() {
            index -= 1;
        }
        self.col = index;
    }

    /// Moves past the end of the word right of the cursor (alphanumeric
    /// runs are words); at a line end it wraps to the next line.
    pub fn move_word_right(&mut self) {
        self.goal_col = None;
        let chars: Vec<char> = self.lines[self.row].chars().collect();
        if self.col >= chars.len() {
            if self.row + 1 < self.lines.len() {
                self.row += 1;
                self.col = 0;
            }
            return;
        }
        let mut index = self.col;
        while index < chars.len() && !chars[index].is_alphanumeric() {
            index += 1;
        }
        while index < chars.len() && chars[index].is_alphanumeric() {
            index += 1;
        }
        self.col = index;
    }

    // --- undo / redo -------------------------------------------------------

    /// Reverts the newest edit. Returns whether a step was available.
    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else {
            return false;
        };
        match &step.op {
            UndoOp::Insert { row, col, text } => self.remove_text_at(*row, *col, text),
            UndoOp::Delete { row, col, text } => self.insert_text_at(*row, *col, text),
            UndoOp::Join { row, col } => self.split_at(*row, *col),
            UndoOp::Split { row, .. } => self.join_at(*row),
            UndoOp::EraseLine { row, text } => {
                self.lines.insert(*row, text.clone());
            }
        }
        self.group += 1;
        self.row = step.before.0;
        self.col = step.before.1;
        self.goal_col = None;
        self.row = self.row.min(self.lines.len() - 1);
        self.col = self.col.min(self.lines[self.row].chars().count());
        self.redo.push(step);
        true
    }

    /// Re-applies the newest undone edit. Returns whether a step was
    /// available.
    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        match &step.op {
            UndoOp::Insert { row, col, text } => self.insert_text_at(*row, *col, text),
            UndoOp::Delete { row, col, text } => self.remove_text_at(*row, *col, text),
            UndoOp::Join { row, col: _ } => self.join_at(*row),
            UndoOp::Split { row, col } => self.split_at(*row, *col),
            UndoOp::EraseLine { row, text: _ } => self.erase_row(*row),
        }
        self.group += 1;
        self.row = step.after.0;
        self.col = step.after.1;
        self.goal_col = None;
        self.row = self.row.min(self.lines.len() - 1);
        self.col = self.col.min(self.lines[self.row].chars().count());
        self.undo.push(step);
        true
    }

    // --- window sync -------------------------------------------------------

    /// Syncs the scroll window so the cursor is inside a window of
    /// `visible_width` columns and `visible_height` rows. Both offsets
    /// move minimally: scrolling only happens when the cursor would
    /// leave the window. Called by the renderer; exposed for tests and
    /// apps that compute layouts early.
    pub fn sync_display_offset(&mut self, visible_width: usize, visible_height: usize) {
        // vertical
        if visible_height == 0 {
            self.row_offset = 0;
        } else {
            if self.row < self.row_offset {
                self.row_offset = self.row;
            }
            while self.row.saturating_sub(self.row_offset) >= visible_height {
                self.row_offset += 1;
            }
        }
        // horizontal
        if visible_width == 0 {
            self.col_offset = 0;
            return;
        }
        if self.col < self.col_offset {
            self.col_offset = self.col;
        }
        // While the cursor column sits at or past the window edge, scroll
        // one char at a time; when the offset reaches the cursor the
        // relative column is zero, so the loop always terminates.
        loop {
            let line = &self.lines[self.row];
            let relative = display_column_of_char_index(line, self.col)
                - display_column_of_char_index(line, self.col_offset);
            if relative < visible_width || self.col_offset >= self.col {
                break;
            }
            self.col_offset += 1;
        }
    }
}

/// A multi-line text editor: the paint configuration over
/// [`TextAreaState`].
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_widgets::{TextArea, TextAreaState};
///
/// let mut state = TextAreaState::new();
/// state.insert_str("first");
/// state.new_line();
/// state.insert_str("second");
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 2));
/// TextArea::new().render(Rect::new(0, 0, 10, 2), &mut buffer, &mut state);
/// assert_eq!(buffer.to_string(), "first     \nsecond    ");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextArea {
    block: Option<Block>,
    style: Style,
}

impl Default for TextArea {
    fn default() -> Self {
        Self {
            block: None,
            style: Style::new(),
        }
    }
}

impl TextArea {
    /// A bare text area: default style, no block.
    pub fn new() -> Self {
        Self::default()
    }

    /// Wraps the area in a block; the text fills the block's inner area.
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// The style of the text.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Maps the theme: the `text` slot styles the content.
    pub fn theme(mut self, theme: &tuvix_core::theme::Theme) -> Self {
        self.style = theme.text;
        self
    }

    /// Paints the visible window of the document into `buffer` and syncs
    /// `state`'s scroll offsets to the inner area.
    pub fn render(&self, area: Rect, buffer: &mut Buffer, state: &mut TextAreaState) {
        if area.is_empty() {
            return;
        }
        if let Some(block) = &self.block {
            block.render(area, buffer);
        }
        let inner = self.block.as_ref().map_or(area, |block| block.inner(area));
        if inner.is_empty() {
            return;
        }
        state.sync_display_offset(usize::from(inner.width), usize::from(inner.height));
        for visible_row in 0..usize::from(inner.height) {
            let row = state.row_offset + visible_row;
            let Some(line) = state.lines.get(row) else {
                break;
            };
            let byte = byte_of(line, state.col_offset);
            let y = inner.y + visible_row as u16;
            buffer.set_stringn(inner.x, y, &line[byte..], inner.width, self.style);
        }
    }

    /// The terminal cell the cursor belongs to, given the same `area` the
    /// widget was rendered into. The application feeds this to
    /// `Frame::set_cursor_position` while the area is focused; the
    /// position is clamped to the inner area.
    pub fn cursor_position(&self, area: Rect, state: &TextAreaState) -> Position {
        let inner = self.block.as_ref().map_or(area, |block| block.inner(area));
        if inner.is_empty() {
            return Position::new(area.x, area.y);
        }
        let window_start = display_column_of_char_index(&state.lines[state.row], state.col_offset);
        let column =
            usize::from(inner.x) + state.visible_cursor_column().saturating_sub(window_start);
        let row = usize::from(inner.y) + state.row.saturating_sub(state.row_offset);
        let max_x = usize::from(inner.right().saturating_sub(1));
        let max_y = usize::from(inner.bottom().saturating_sub(1));
        Position::new(column.min(max_x) as u16, row.min(max_y) as u16)
    }
}

impl RenderWidget for TextArea {
    type State = TextAreaState;

    fn render(self, area: Rect, buffer: &mut Buffer, state: &mut Self::State) {
        TextArea::render(&self, area, buffer, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    /// Types a document through the public editing API.
    fn state_with(text: &str) -> TextAreaState {
        let mut state = TextAreaState::new();
        state.insert_str(text);
        state
    }

    mod editing {
        use super::*;

        #[test]
        fn insert_appends_at_the_cursor() {
            let mut state = TextAreaState::new();
            state.insert('a');
            state.insert('b');
            state.insert('c');
            assert_eq!(state.lines(), ["abc"]);
            assert_eq!(state.cursor(), (0, 3));
        }

        #[test]
        fn insert_str_with_newline_splits_lines() {
            let mut state = TextAreaState::new();
            state.insert_str("ab\ncd");
            assert_eq!(state.lines(), ["ab", "cd"]);
            assert_eq!(state.cursor(), (1, 2));
            state.insert_str("x\ny");
            assert_eq!(state.lines(), ["ab", "cdx", "y"]);
            assert_eq!(state.cursor(), (2, 1));
        }

        #[test]
        fn insert_in_the_middle_splices() {
            let mut state = state_with("one\ntwo");
            state.move_doc_start();
            state.move_end(); // end of "one"
            state.insert_str("-half");
            assert_eq!(state.lines(), ["one-half", "two"]);
            assert_eq!(state.cursor(), (0, 8));
        }

        #[test]
        fn delete_backward_removes_before_the_cursor() {
            let mut state = state_with("abc");
            state.delete_backward();
            assert_eq!(state.lines(), ["ab"]);
            assert_eq!(state.cursor(), (0, 2));
            state.move_home();
            state.delete_backward();
            assert_eq!(state.lines(), ["ab"], "no-op at the document start");
        }

        #[test]
        fn delete_backward_at_line_start_joins_the_previous_line() {
            let mut state = state_with("ab\ncd");
            state.move_down();
            state.move_home();
            state.delete_backward();
            assert_eq!(state.lines(), ["abcd"]);
            assert_eq!(state.cursor(), (0, 2), "cursor at the join point");
        }

        #[test]
        fn delete_forward_removes_at_the_cursor() {
            let mut state = state_with("abc");
            state.move_home();
            state.delete_forward();
            assert_eq!(state.lines(), ["bc"]);
            assert_eq!(state.cursor(), (0, 0));
            state.move_doc_end();
            state.delete_forward();
            assert_eq!(state.lines(), ["bc"], "no-op at the document end");
        }

        #[test]
        fn delete_forward_at_line_end_joins_the_next_line() {
            let mut state = state_with("ab\ncd");
            state.move_doc_start();
            state.move_end();
            state.delete_forward();
            assert_eq!(state.lines(), ["abcd"]);
            assert_eq!(state.cursor(), (0, 2), "cursor stays at the join point");
        }

        #[test]
        fn new_line_splits_at_the_cursor() {
            let mut state = state_with("abcd");
            state.move_left();
            state.move_left();
            state.new_line();
            assert_eq!(state.lines(), ["ab", "cd"]);
            assert_eq!(state.cursor(), (1, 0));
        }

        #[test]
        fn delete_line_removes_the_row_and_clamps() {
            let mut state = state_with("one\ntwo\nthree");
            state.move_doc_start();
            state.move_down();
            state.delete_line();
            assert_eq!(state.lines(), ["one", "three"]);
            assert_eq!(state.cursor(), (1, 0));
        }

        #[test]
        fn delete_line_on_the_only_line_clears_it() {
            let mut state = state_with("solo");
            state.delete_line();
            assert_eq!(state.lines(), [""]);
            assert_eq!(state.cursor(), (0, 0));
            state.delete_line();
            assert_eq!(state.lines(), [""]);
        }

        #[test]
        fn set_text_resets_cursor_and_history() {
            let mut state = state_with("one\ntwo");
            state.set_text("a\nb\nc");
            assert_eq!(state.lines(), ["a", "b", "c"]);
            assert_eq!(state.cursor(), (0, 0));
            assert!(!state.can_undo(), "bulk replace resets the history");
        }

        #[test]
        fn set_text_empty_is_one_empty_line() {
            let mut state = state_with("one\ntwo");
            state.set_text("");
            assert_eq!(state.lines(), [""]);
        }
    }

    mod word_motions {
        use super::*;

        #[test]
        fn word_left_walks_runs_and_separators_within_the_line() {
            let mut state = state_with("foo.bar baz");
            assert_eq!(state.cursor(), (0, 11));
            state.move_word_left();
            assert_eq!(state.cursor(), (0, 8));
            state.move_word_left();
            assert_eq!(state.cursor(), (0, 4));
            state.move_word_left();
            assert_eq!(state.cursor(), (0, 0));
            state.move_word_left();
            assert_eq!(state.cursor(), (0, 0), "clamped at the line start");
        }

        #[test]
        fn word_left_at_line_start_wraps_to_the_previous_line() {
            let mut state = state_with("ab\ncd");
            state.move_down();
            state.move_home();
            state.move_word_left();
            assert_eq!(state.cursor(), (0, 2));
        }

        #[test]
        fn word_right_symmetric_and_wraps() {
            let mut state = state_with("foo.bar");
            state.move_home();
            state.move_word_right();
            assert_eq!(state.cursor(), (0, 3));
            state.move_word_right();
            assert_eq!(state.cursor(), (0, 7));
            state.move_word_right();
            assert_eq!(state.cursor(), (0, 7), "clamped at the line end");
            state.new_line();
            state.move_word_right();
            assert_eq!(state.cursor(), (1, 0), "wraps to the next line");
        }

        #[test]
        fn delete_word_backward_removes_the_word_and_gaps() {
            let mut state = state_with("foo bar");
            state.delete_word_backward();
            assert_eq!(state.lines(), ["foo "]);
            state.delete_word_backward();
            assert_eq!(
                state.lines(),
                [""],
                "the next call takes the word and its gap"
            );
        }

        #[test]
        fn delete_word_backward_at_line_start_joins() {
            let mut state = state_with("ab\ncd");
            state.move_down();
            state.move_home();
            state.delete_word_backward();
            assert_eq!(state.lines(), ["abcd"]);
        }

        #[test]
        fn delete_word_forward_removes_the_run() {
            let mut state = state_with("foo.bar");
            state.move_home();
            state.delete_word_forward();
            assert_eq!(state.lines(), [".bar"]);
            state.delete_word_forward();
            assert_eq!(state.lines(), [""], "the '.' is skipped with the run");
        }
    }

    mod motions {
        use super::*;

        #[test]
        fn left_right_wrap_across_lines() {
            let mut state = state_with("ab\ncd");
            state.move_doc_start();
            state.move_left();
            assert_eq!(state.cursor(), (0, 0), "clamped at the document start");
            state.move_right();
            state.move_right();
            assert_eq!(state.cursor(), (0, 2));
            state.move_right();
            assert_eq!(state.cursor(), (1, 0), "wrapped to the next line");
            state.move_right();
            state.move_right();
            assert_eq!(state.cursor(), (1, 2));
            state.move_right();
            assert_eq!(state.cursor(), (1, 2), "clamped at the document end");
        }

        #[test]
        fn up_down_keep_the_goal_column_and_clamp() {
            let mut state = state_with("long line here\nhi\nlonger line again");
            state.move_doc_start();
            state.move_end(); // (0, 14)
            state.move_down(); // "hi" clamps to (1, 2)
            assert_eq!(state.cursor(), (1, 2));
            state.move_down(); // the goal column restores on the longer row
            assert_eq!(state.cursor(), (2, 14));
            state.move_up();
            assert_eq!(state.cursor(), (1, 2));
            state.move_up();
            assert_eq!(state.cursor(), (0, 14));
            state.move_up();
            assert_eq!(state.cursor(), (0, 14), "no-op on the first row");
        }

        #[test]
        fn horizontal_motion_resets_the_goal_column() {
            let mut state = state_with("abcdef\nxy\nlongrow");
            state.move_doc_start();
            state.move_doc_end();
            state.move_up();
            state.move_up(); // goal column 6 -> lands on "abcdef" at 6
            assert_eq!(state.cursor(), (0, 6));
            state.move_left(); // resets the goal
            state.move_down();
            assert_eq!(state.cursor(), (1, 2), "clamped to the short row");
            state.move_down();
            assert_eq!(state.cursor(), (2, 5), "the goal did not survive");
        }

        #[test]
        fn home_end_and_document_motions() {
            let mut state = state_with("ab\ncdef");
            state.move_doc_end();
            assert_eq!(state.cursor(), (1, 4));
            state.move_home();
            assert_eq!(state.cursor(), (1, 0));
            state.move_doc_start();
            assert_eq!(state.cursor(), (0, 0));
            state.move_end();
            assert_eq!(state.cursor(), (0, 2));
        }
    }

    mod undo_redo {
        use super::*;

        #[test]
        fn undo_reverts_an_insert_exactly() {
            let mut state = state_with("hello");
            assert!(state.undo());
            assert_eq!(state.lines(), [""]);
            assert_eq!(state.cursor(), (0, 0));
            assert!(!state.can_undo());
            assert!(state.can_redo());
        }

        #[test]
        fn a_typing_run_coalesces_into_one_undo_step() {
            let mut state = TextAreaState::new();
            for ch in "hello".chars() {
                state.insert(ch);
            }
            assert!(state.undo(), "the whole word is one step");
            assert_eq!(state.lines(), [""]);
            assert!(!state.can_undo());
        }

        #[test]
        fn a_backward_delete_chain_coalesces() {
            let mut state = state_with("hello");
            state.delete_backward();
            state.delete_backward();
            state.delete_backward();
            assert_eq!(state.lines(), ["he"]);
            state.undo();
            assert_eq!(state.lines(), ["hello"], "one undo restores all three");
        }

        #[test]
        fn a_forward_delete_chain_coalesces() {
            let mut state = state_with("hello");
            state.move_home();
            state.delete_forward();
            state.delete_forward();
            assert_eq!(state.lines(), ["llo"]);
            state.undo();
            assert_eq!(state.lines(), ["hello"]);
        }

        #[test]
        fn undo_restores_splits_and_joins() {
            let mut state = state_with("abcd");
            state.move_left();
            state.move_left();
            state.new_line();
            assert_eq!(state.lines(), ["ab", "cd"]);
            state.undo();
            assert_eq!(state.lines(), ["abcd"]);
            assert_eq!(state.cursor(), (0, 2), "the pre-split cursor");
            state.redo();
            assert_eq!(state.lines(), ["ab", "cd"]);
            assert_eq!(state.cursor(), (1, 0), "the post-split cursor");
        }

        #[test]
        fn undo_restores_line_deletion() {
            let mut state = state_with("one\ntwo\nthree");
            state.move_doc_start();
            state.move_down();
            state.delete_line();
            assert_eq!(state.lines(), ["one", "three"]);
            state.undo();
            assert_eq!(state.lines(), ["one", "two", "three"]);
            assert_eq!(state.cursor(), (1, 0));
            state.redo();
            assert_eq!(state.lines(), ["one", "three"]);
        }

        #[test]
        fn a_new_edit_clears_the_redo_stack() {
            let mut state = state_with("abc");
            state.delete_backward();
            state.undo();
            state.insert('x');
            assert!(!state.can_redo());
            state.undo();
            assert_eq!(state.lines(), ["abc"], "only the fresh edit is undone");
        }

        #[test]
        fn undo_limit_drops_the_oldest_steps() {
            let mut state = TextAreaState::new();
            state.set_undo_limit(2);
            state.insert('a'); // step 1
            state.new_line(); // step 2
            state.insert('b'); // step 3 pushes step 1 out
            assert!(state.undo());
            assert!(state.undo());
            assert!(!state.can_undo(), "the oldest step was dropped");
            assert_eq!(state.lines(), ["a"], "only the newest two steps undid");
        }

        #[test]
        fn undo_limit_zero_disables_undo() {
            let mut state = state_with("abc");
            state.set_undo_limit(0);
            state.delete_backward();
            assert!(!state.can_undo());
        }

        #[test]
        fn undo_restores_the_cursor_across_line_joins() {
            let mut state = state_with("ab\ncd\nef");
            state.move_doc_start();
            state.move_down();
            state.move_home();
            state.delete_backward(); // joins "cd" into "ab"
            assert_eq!(state.lines(), ["abcd", "ef"]);
            state.undo();
            assert_eq!(state.lines(), ["ab", "cd", "ef"]);
            assert_eq!(state.cursor(), (1, 0), "back where the join happened");
        }

        #[test]
        fn multi_line_insert_undoes_as_one_step() {
            let mut state = state_with("ab");
            state.insert_str("x\ny\nz");
            assert_eq!(state.lines(), ["abx", "y", "z"]);
            state.undo();
            assert_eq!(state.lines(), ["ab"]);
            assert_eq!(state.cursor(), (0, 2));
        }

        /// Randomized editing session: whatever the op sequence did,
        /// undoing every recorded step restores the pristine empty
        /// document (and redo moves forward again). The LCG generator is
        /// the corpus gate's.
        #[test]
        fn randomized_edits_survive_undo_all() {
            const ALPHABET: [char; 8] = ['a', 'b', ' ', '.', '汉', 'é', 'x', 'Z'];
            let mut seed: u64 = 0x5EED_10A0_C0FF_EE10;
            let mut next = move || {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                seed
            };
            let mut state = TextAreaState::new();
            state.set_undo_limit(usize::MAX);
            let mut effective_edits = 0;
            for _ in 0..600 {
                let before = state.text();
                match next() % 10 {
                    0..=4 => state.insert(ALPHABET[(next() % 8) as usize]),
                    5 => state.new_line(),
                    6..=7 => state.delete_backward(),
                    8 => state.delete_line(),
                    _ => state.insert_str("row\nrow"),
                }
                if state.text() != before {
                    effective_edits += 1;
                }
            }
            assert!(effective_edits > 200, "the fuzz must exercise real edits");
            while state.undo() {}
            assert_eq!(state.lines(), [""], "undo-all is the identity");
            assert_eq!(state.cursor(), (0, 0));
            assert!(!state.can_undo());
            assert!(state.can_redo());
        }

        /// Same property over the wide corpus: multi-line pastes, wide
        /// glyphs, flags — undo-all equals a fresh state byte-for-byte.
        #[test]
        fn undo_all_equals_a_fresh_state_over_the_wide_corpus() {
            const PIECES: [&str; 6] = ["汉", "字!", "row\nrow", "é", " ", "flag🇺🇸"];
            let mut seed: u64 = 0x00DD_B1A1_5EED_5EED;
            let mut next = move || {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                seed
            };
            let mut state = TextAreaState::new();
            state.set_undo_limit(usize::MAX);
            for _ in 0..400 {
                match next() % 8 {
                    0..=3 => state.insert_str(PIECES[(next() % 6) as usize]),
                    4 => state.new_line(),
                    5..=6 => state.delete_backward(),
                    _ => state.delete_line(),
                }
            }
            while state.undo() {}
            let fresh = TextAreaState::new();
            assert_eq!(state.lines(), fresh.lines());
            assert_eq!(state.cursor(), fresh.cursor());
        }
    }

    mod display_offset {
        use super::*;

        #[test]
        fn sync_keeps_the_cursor_inside_a_narrow_window() {
            let mut state = state_with("0123456789");
            state.move_doc_start();
            state.sync_display_offset(5, 1);
            assert_eq!(state.col_offset(), 0);
            state.move_doc_end();
            state.sync_display_offset(5, 1);
            assert_eq!(state.col_offset(), 6, "the window followed the cursor");
            assert_eq!(state.visible_cursor_column() - state.col_offset(), 4);
        }

        #[test]
        fn sync_pulls_back_when_the_cursor_moves_left() {
            let mut state = state_with("0123456789");
            state.move_doc_end();
            state.sync_display_offset(5, 1);
            assert_eq!(state.col_offset(), 6);
            for _ in 0..4 {
                state.move_left();
            }
            state.sync_display_offset(5, 1);
            assert_eq!(state.col_offset(), 6, "still inside, no scroll");
            state.move_left();
            state.sync_display_offset(5, 1);
            assert_eq!(state.col_offset(), 5, "the window pulled back");
        }

        #[test]
        fn sync_scrolls_by_whole_wide_glyphs() {
            let mut state = state_with("汉汉汉汉");
            state.move_doc_end();
            state.sync_display_offset(3, 1);
            // The cursor sits at display column 8. The offset is a char
            // index by construction — it can never split a glyph — and
            // the loop stops as soon as the cursor fits the window.
            let offset = state.col_offset();
            assert!(offset <= 4, "the offset stays within the line");
            let window_start = display_column_of_char_index("汉汉汉汉", offset);
            assert!(
                8 - window_start < 3,
                "the cursor fits the 3-column window from column {window_start}"
            );
        }

        #[test]
        fn sync_scrolls_rows() {
            let mut state = state_with("a\nb\nc\nd\ne");
            state.move_doc_start();
            state.sync_display_offset(10, 2);
            assert_eq!(state.row_offset(), 0);
            state.move_doc_end();
            state.sync_display_offset(10, 2);
            assert_eq!(state.row_offset(), 3, "the last two rows are visible");
            state.move_doc_start();
            state.sync_display_offset(10, 2);
            assert_eq!(state.row_offset(), 0);
        }

        #[test]
        fn sync_with_zero_visible_size_is_safe() {
            let mut state = state_with("abc\ndef");
            state.move_doc_end();
            state.sync_display_offset(0, 0);
            assert_eq!(state.col_offset(), 0);
            assert_eq!(state.row_offset(), 0);
        }
    }

    mod rendering {
        use super::*;
        use tuvix_core::buffer::Buffer;
        use tuvix_core::geometry::Rect;
        use tuvix_core::theme::Theme;

        fn buffer_at(width: u16, height: u16) -> Buffer {
            Buffer::empty(Rect::new(0, 0, width, height))
        }

        #[test]
        fn renders_every_visible_line() {
            let mut state = state_with("first\nsecond\nthird");
            let mut buffer = buffer_at(10, 3);
            TextArea::new().render(Rect::new(0, 0, 10, 3), &mut buffer, &mut state);
            assert_eq!(buffer.to_string(), "first     \nsecond    \nthird     ");
        }

        #[test]
        fn clips_to_the_visible_window() {
            let mut state = state_with("a\nb\nc\nd\ne\nf");
            state.move_doc_start();
            let mut buffer = buffer_at(4, 2);
            TextArea::new().render(Rect::new(0, 0, 4, 2), &mut buffer, &mut state);
            assert_eq!(buffer.to_string(), "a   \nb   ");
        }

        #[test]
        fn the_cursor_row_scrolls_into_view() {
            let mut state = state_with("a\nb\nc\nd\ne\nf");
            state.move_doc_end();
            let mut buffer = buffer_at(4, 2);
            TextArea::new().render(Rect::new(0, 0, 4, 2), &mut buffer, &mut state);
            assert_eq!(
                buffer.to_string(),
                "e   \nf   ",
                "the window followed the cursor"
            );
        }

        #[test]
        fn the_horizontal_window_follows_the_cursor() {
            let mut state = state_with("0123456789");
            state.move_doc_end();
            let mut buffer = buffer_at(5, 1);
            TextArea::new().render(Rect::new(0, 0, 5, 1), &mut buffer, &mut state);
            assert_eq!(buffer.to_string(), "6789 ");
        }

        #[test]
        fn block_inner_offsets_apply() {
            use crate::block::{Block, BorderType};
            let block = Block::bordered().border_type(BorderType::Plain);
            let mut state = state_with("hi");
            let mut buffer = buffer_at(6, 3);
            TextArea::new().block(block.clone()).render(
                Rect::new(0, 0, 6, 3),
                &mut buffer,
                &mut state,
            );
            let rendered = buffer.to_string();
            assert!(
                rendered.contains("hi"),
                "text inside the border: {rendered:?}"
            );
        }

        #[test]
        fn theme_maps_the_text_slot() {
            let theme = Theme::dark();
            let mut state = state_with("hi");
            let mut buffer = buffer_at(2, 1);
            TextArea::new()
                .theme(&theme)
                .render(Rect::new(0, 0, 2, 1), &mut buffer, &mut state);
            assert_eq!(buffer.get(0, 0).unwrap().style, theme.text);
        }

        #[test]
        fn custom_style_overrides() {
            let style = Style::new().fg(Color::Red);
            let mut state = state_with("hi");
            let mut buffer = buffer_at(2, 1);
            TextArea::new()
                .style(style)
                .render(Rect::new(0, 0, 2, 1), &mut buffer, &mut state);
            assert_eq!(buffer.get(0, 0).unwrap().style, style);
        }

        #[test]
        fn empty_areas_are_no_ops() {
            let mut state = state_with("hi");
            let mut buffer = buffer_at(4, 2);
            TextArea::new().render(Rect::new(0, 0, 0, 0), &mut buffer, &mut state);
            assert_eq!(buffer.to_string(), "    \n    ");
        }

        #[test]
        fn render_widget_contract_passes_through() {
            use tuvix_core::widget::RenderWidget as _;
            let mut state = state_with("one\ntwo");
            let mut buffer = buffer_at(5, 2);
            TextArea::new().render(Rect::new(0, 0, 5, 2), &mut buffer, &mut state);
            assert_eq!(buffer.to_string(), "one  \ntwo  ");
        }

        #[test]
        fn cursor_position_tracks_block_offsets_and_clamps() {
            use crate::block::Block;
            let area = Rect::new(2, 3, 10, 3);
            let widget = TextArea::new().block(Block::bordered());
            let mut state = state_with("hello");
            let mut buffer = buffer_at(20, 10);
            widget.clone().render(area, &mut buffer, &mut state);
            let position = widget.cursor_position(area, &state);
            assert_eq!(position, Position::new(2 + 1 + 5, 3 + 1));
            // a cursor past the window clamps into the inner area
            state.move_doc_end();
            state.insert_str(" and more text past the edge");
            let position = widget.cursor_position(area, &state);
            assert!(position.x <= area.right() - 2, "clamped inside the border");
            assert!(position.y >= area.y);
        }

        #[test]
        fn empty_state_renders_blank_rows() {
            let mut state = TextAreaState::new();
            let mut buffer = buffer_at(3, 2);
            TextArea::new().render(Rect::new(0, 0, 3, 2), &mut buffer, &mut state);
            assert_eq!(buffer.to_string(), "   \n   ");
        }
    }
}
