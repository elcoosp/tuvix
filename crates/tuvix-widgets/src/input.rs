//! Input: a single-line text field with a full editing state.
//!
//! The value, cursor and display offset live in [`InputState`]; the
//! [`Input`] widget is the paint configuration (styles, placeholder,
//! block). Editing is char-indexed: the cursor never splits a grapheme,
//! word motions treat alphanumeric runs as words, and the display offset
//! syncs during paint so the cursor cell stays inside the visible window
//! (`InputState::sync_display_offset` — the same method the renderer
//! calls, exposed for tests and apps).
//!
//! The terminal cursor itself stays under the application's control: the
//! widget reports where it belongs through [`Input::cursor_position`],
//! and the app feeds that to `Frame::set_cursor_position` when the field
//! is focused (see the `inputs` example).

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Position, Rect};
use tuvix_core::style::{Modifier, Style};
use tuvix_core::text::Line;
use tuvix_core::widget::RenderWidget;

use crate::block::Block;

/// The editing state of an [`Input`]: value, cursor and scroll window.
///
/// # Examples
///
/// ```
/// use tuvix_widgets::InputState;
///
/// let mut state = InputState::new();
/// state.insert_str("hello");
/// state.move_left();
/// state.insert(' ');
/// assert_eq!(state.value(), "hell o");
/// assert_eq!(state.cursor(), 5);
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InputState {
    value: String,
    /// Cursor position as a char index (never inside a grapheme).
    cursor: usize,
    /// Char index of the first visible character.
    display_offset: usize,
}

impl InputState {
    /// An empty field.
    pub fn new() -> Self {
        Self::default()
    }

    /// The current text.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The cursor as a char index into the value (0 = before the first
    /// character, `value.chars().count()` = after the last).
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The char index of the first visible character (the scroll window).
    pub fn display_offset(&self) -> usize {
        self.display_offset
    }

    /// Replaces the whole value; the cursor moves to the end.
    pub fn set_value(&mut self, value: &str) {
        value.clone_into(&mut self.value);
        self.cursor = self.value.chars().count();
    }

    /// Removes every character; the cursor returns to the start.
    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
        self.display_offset = 0;
    }

    /// Inserts one character before the cursor.
    pub fn insert(&mut self, character: char) {
        self.insert_str(&character.to_string());
    }

    /// Inserts every character of `text` before the cursor, in order.
    pub fn insert_str(&mut self, text: &str) {
        let byte = self.byte_of(self.cursor);
        self.value.insert_str(byte, text);
        self.cursor += text.chars().count();
    }

    /// Deletes the character before the cursor (backspace), if any.
    pub fn delete_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        let byte = self.byte_of(self.cursor);
        self.value
            .replace_range(byte..self.byte_of(self.cursor + 1), "");
    }

    /// Deletes the character at the cursor (delete key), if any.
    pub fn delete_forward(&mut self) {
        if self.cursor >= self.value.chars().count() {
            return;
        }
        let byte = self.byte_of(self.cursor);
        self.value
            .replace_range(byte..self.byte_of(self.cursor + 1), "");
    }

    /// Deletes back to the start of the word left of the cursor.
    pub fn delete_word_backward(&mut self) {
        let end = self.cursor;
        self.move_word_left();
        let chars: Vec<char> = self.value.chars().collect();
        let removed: String = chars[self.cursor..end].iter().collect();
        let byte = self.byte_of(self.cursor);
        let end_byte = byte + removed.len();
        self.value.replace_range(byte..end_byte, "");
    }

    /// Deletes through the end of the word right of the cursor; the
    /// cursor stays where the deletion began.
    pub fn delete_word_forward(&mut self) {
        let start = self.cursor;
        self.move_word_right();
        let chars: Vec<char> = self.value.chars().collect();
        let removed: String = chars[start..self.cursor].iter().collect();
        let byte = self.byte_of(start);
        self.value.replace_range(byte..byte + removed.len(), "");
        self.cursor = start.min(self.value.chars().count());
    }

    /// Moves the cursor one character left (clamped at the start).
    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Moves the cursor one character right (clamped at the end).
    pub fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.value.chars().count());
    }

    /// Moves the cursor to the start of the line.
    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    /// Moves the cursor to the end of the line.
    pub fn move_end(&mut self) {
        self.cursor = self.value.chars().count();
    }

    /// Moves the cursor to the start of the word left of it: any run of
    /// non-alphanumeric characters is skipped, then the alphanumeric run.
    pub fn move_word_left(&mut self) {
        let chars: Vec<char> = self.value.chars().collect();
        let mut index = self.cursor.min(chars.len());
        while index > 0 && !chars[index - 1].is_alphanumeric() {
            index -= 1;
        }
        while index > 0 && chars[index - 1].is_alphanumeric() {
            index -= 1;
        }
        self.cursor = index;
    }

    /// Moves the cursor past the end of the word right of it: any run of
    /// non-alphanumeric characters is skipped, then the alphanumeric run.
    pub fn move_word_right(&mut self) {
        let chars: Vec<char> = self.value.chars().collect();
        let mut index = self.cursor;
        while index < chars.len() && !chars[index].is_alphanumeric() {
            index += 1;
        }
        while index < chars.len() && chars[index].is_alphanumeric() {
            index += 1;
        }
        self.cursor = index;
    }

    /// Syncs the display offset so the cursor cell is inside a window of
    /// `visible_width` columns. The offset moves minimally: scrolling
    /// only happens when the cursor would leave the window. Called by the
    /// renderer; exposed for tests and apps that compute layouts early.
    pub fn sync_display_offset(&mut self, visible_width: usize) {
        if visible_width == 0 {
            self.display_offset = 0;
            return;
        }
        let len = self.value.chars().count();
        let cursor = self.cursor.min(len);
        if cursor < self.display_offset {
            self.display_offset = cursor;
        }
        // While the cursor column sits at or past the window edge, scroll
        // one character at a time; when the offset reaches the cursor the
        // column is zero, so the loop always terminates.
        loop {
            let column = self.visible_cursor_column();
            if column < visible_width || self.display_offset >= cursor {
                break;
            }
            self.display_offset += 1;
        }
    }

    /// The column the cursor occupies relative to the visible window
    /// (the display width of the characters between the window start and
    /// the cursor; control characters count zero columns).
    pub fn visible_cursor_column(&self) -> usize {
        self.value
            .chars()
            .skip(self.display_offset)
            .take(self.cursor.saturating_sub(self.display_offset))
            .map(|character| character.width().unwrap_or(0))
            .sum()
    }

    /// The visible tail of the value: everything from the display offset
    /// on. Borrowed, not copied.
    fn visible_tail(&self) -> &str {
        let byte = self.byte_of(self.display_offset);
        &self.value[byte..]
    }

    /// Byte offset of a char index (`value.len()` past the end).
    fn byte_of(&self, char_index: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_index)
            .map_or(self.value.len(), |(byte, _)| byte)
    }
}

use unicode_width::UnicodeWidthChar as _;

/// A single-line text field: the paint configuration over [`InputState`].
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::style::Style;
/// use tuvix_widgets::{Input, InputState};
///
/// let mut state = InputState::new();
/// state.insert_str("tuvix");
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
/// Input::new().render(Rect::new(0, 0, 10, 1), &mut buffer, &mut state);
/// assert_eq!(buffer.to_string(), "tuvix     ");
/// assert_eq!(state.visible_cursor_column(), 5);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    block: Option<Block>,
    style: Style,
    placeholder: Option<Line>,
    placeholder_style: Style,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            block: None,
            style: Style::new(),
            placeholder: None,
            placeholder_style: Style::new().add_modifier(Modifier::DIM),
        }
    }
}

impl Input {
    /// A bare input field: default style, no placeholder, no block.
    pub fn new() -> Self {
        Self::default()
    }

    /// Wraps the field in a block; the text fills the block's inner area.
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// The style of the value text.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The line rendered while the value is empty.
    pub fn placeholder<T: Into<Line>>(mut self, placeholder: T) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// The style of the placeholder (default: dimmed).
    pub fn placeholder_style(mut self, style: Style) -> Self {
        self.placeholder_style = style;
        self
    }

    /// Maps the theme: the `text` slot styles the value, the `muted` slot
    /// the placeholder.
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.style = theme.text;
        self.placeholder_style = theme.muted;
        self
    }

    /// Paints the field into `buffer` and syncs `state`'s display offset
    /// to the visible width.
    ///
    /// With an empty value and a placeholder configured, the placeholder
    /// renders in the placeholder style; the value is otherwise painted
    /// from the display offset on, clipped to the inner area.
    pub fn render(&self, area: Rect, buffer: &mut Buffer, state: &mut InputState) {
        if area.is_empty() {
            return;
        }
        if let Some(block) = &self.block {
            block.render(area, buffer);
        }
        let field_area = self.block.as_ref().map_or(area, |block| block.inner(area));
        if field_area.is_empty() {
            return;
        }
        let width = usize::from(field_area.width);
        state.sync_display_offset(width);
        let row = field_area.y;
        let right = field_area.right();

        if state.value.is_empty()
            && let Some(placeholder) = &self.placeholder
        {
            let mut x = field_area.x;
            for span in &placeholder.spans {
                if x >= right {
                    break;
                }
                x = buffer
                    .set_stringn(
                        x,
                        row,
                        span.content.as_ref(),
                        right - x,
                        self.placeholder_style.patch(span.style),
                    )
                    .0;
            }
            return;
        }

        buffer.set_stringn(
            field_area.x,
            row,
            state.visible_tail(),
            field_area.width,
            self.style,
        );
    }

    /// The terminal cell the field's cursor belongs to, given the same
    /// `area` the field was rendered into. The application feeds this to
    /// `Frame::set_cursor_position` while the field is focused; the
    /// position is clamped to the field area.
    pub fn cursor_position(&self, area: Rect, state: &InputState) -> Position {
        let field_area = self.block.as_ref().map_or(area, |block| block.inner(area));
        if field_area.is_empty() {
            return Position::new(area.x, area.y);
        }
        let column = usize::from(field_area.x) + state.visible_cursor_column();
        let max = usize::from(field_area.right().saturating_sub(1));
        Position::new(column.min(max) as u16, field_area.y)
    }
}

impl RenderWidget for Input {
    type State = InputState;

    fn render(self, area: Rect, buffer: &mut Buffer, state: &mut Self::State) {
        Input::render(&self, area, buffer, state);
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    fn state_with(value: &str) -> InputState {
        let mut state = InputState::new();
        state.insert_str(value);
        state
    }

    mod editing {
        use super::*;

        #[test]
        fn insert_appends_at_the_cursor() {
            let mut state = InputState::new();
            state.insert('a');
            state.insert('b');
            state.insert('c');
            assert_eq!(state.value(), "abc");
            assert_eq!(state.cursor(), 3);
        }

        #[test]
        fn insert_str_keeps_char_counting() {
            let mut state = InputState::new();
            state.insert_str("héllo");
            assert_eq!(state.value(), "héllo");
            assert_eq!(state.cursor(), 5, "é is one char");
            state.move_home();
            state.insert_str("ü");
            assert_eq!(state.value(), "ühéllo");
            assert_eq!(state.cursor(), 1);
        }

        #[test]
        fn set_value_moves_the_cursor_to_the_end() {
            let mut state = state_with("abc");
            state.set_value("longer value");
            assert_eq!(state.cursor(), 12);
        }

        #[test]
        fn delete_backward_removes_before_the_cursor() {
            let mut state = state_with("abc");
            state.delete_backward();
            assert_eq!(state.value(), "ab");
            assert_eq!(state.cursor(), 2);
            // At the start it is a no-op.
            state.move_home();
            state.delete_backward();
            assert_eq!(state.value(), "ab");
        }

        #[test]
        fn delete_forward_removes_at_the_cursor() {
            let mut state = state_with("abc");
            state.move_home();
            state.delete_forward();
            assert_eq!(state.value(), "bc");
            assert_eq!(state.cursor(), 0);
            // At the end it is a no-op.
            state.move_end();
            state.delete_forward();
            assert_eq!(state.value(), "bc");
        }

        #[test]
        fn clear_resets_everything() {
            let mut state = state_with("abc");
            state.clear();
            assert_eq!(state.value(), "");
            assert_eq!(state.cursor(), 0);
            assert_eq!(state.display_offset(), 0);
        }

        #[test]
        fn moves_clamp_at_both_ends() {
            let mut state = state_with("ab");
            state.move_right();
            assert_eq!(state.cursor(), 2, "clamped at the end");
            state.move_right();
            assert_eq!(state.cursor(), 2);
            state.move_home();
            state.move_left();
            assert_eq!(state.cursor(), 0, "clamped at the start");
            state.move_end();
            assert_eq!(state.cursor(), 2);
        }
    }

    mod word_motions {
        use super::*;

        #[test]
        fn word_left_skips_separators_then_the_run() {
            let mut state = state_with("foo.bar baz");
            assert_eq!(state.cursor(), 11);
            state.move_word_left(); // "baz"
            assert_eq!(state.cursor(), 8);
            state.move_word_left(); // the '.' separator, then "bar"
            assert_eq!(state.cursor(), 4);
            state.move_word_left(); // "foo"
            assert_eq!(state.cursor(), 0);
            state.move_word_left(); // clamped
            assert_eq!(state.cursor(), 0);
        }

        #[test]
        fn word_right_symmetric() {
            let mut state = state_with("foo.bar baz");
            state.move_home();
            state.move_word_right(); // "foo"
            assert_eq!(state.cursor(), 3);
            state.move_word_right(); // the '.', then "bar"
            assert_eq!(state.cursor(), 7);
            state.move_word_right(); // "baz"
            assert_eq!(state.cursor(), 11);
            state.move_word_right(); // clamped
            assert_eq!(state.cursor(), 11);
        }

        #[test]
        fn delete_word_backward_removes_the_word_and_gaps() {
            let mut state = state_with("foo bar");
            state.delete_word_backward();
            assert_eq!(state.value(), "foo ");
            assert_eq!(state.cursor(), 4);
            state.insert_str("!");
            assert_eq!(state.value(), "foo !", "the separator run is deleted too");
        }

        #[test]
        fn delete_word_forward_removes_through_the_word() {
            let mut state = state_with("foo bar");
            state.move_home();
            state.delete_word_forward();
            assert_eq!(state.value(), " bar");
            assert_eq!(state.cursor(), 0);
        }
    }

    mod scroll_window {
        use super::*;

        #[test]
        fn offset_stays_zero_while_the_cursor_fits() {
            let mut state = state_with("hello");
            state.sync_display_offset(10);
            assert_eq!(state.display_offset(), 0);
        }

        #[test]
        fn offset_scrolls_when_the_cursor_leaves_the_window() {
            let mut state = state_with("hello world");
            state.sync_display_offset(5);
            // Cursor at 11: the window shows "orld" and the cursor cell
            // sits on column 4, inside the 5-wide window.
            assert_eq!(state.display_offset(), 7);
            state.move_left();
            state.sync_display_offset(5);
            assert_eq!(state.display_offset(), 7, "cursor 10 is visible");
            for _ in 0..3 {
                state.move_left();
            }
            state.sync_display_offset(5);
            assert_eq!(state.display_offset(), 7, "cursor 7 sits at column 0");
        }

        #[test]
        fn offset_pulls_back_when_the_cursor_moves_left() {
            let mut state = state_with("hello world");
            state.sync_display_offset(5);
            assert_eq!(state.display_offset(), 7);
            for _ in 0..6 {
                state.move_left();
            }
            state.sync_display_offset(5);
            assert_eq!(state.display_offset(), 5, "cursor 5 < offset 7");
        }

        #[test]
        fn offset_clamps_after_deletions() {
            let mut state = state_with("0123456789");
            state.sync_display_offset(4);
            assert_eq!(state.display_offset(), 7, "cursor 10 in a 4-wide window");
            while state.cursor() > 0 {
                state.delete_backward();
            }
            state.sync_display_offset(4);
            assert_eq!(state.display_offset(), 0);
        }

        #[test]
        fn zero_width_window_resets_the_offset() {
            let mut state = state_with("hello");
            state.sync_display_offset(0);
            assert_eq!(state.display_offset(), 0);
        }

        #[test]
        fn wide_glyphs_count_their_columns() {
            let mut state = state_with("中文中");
            assert_eq!(state.visible_cursor_column(), 6);
            state.move_left();
            assert_eq!(state.visible_cursor_column(), 4);
            state.sync_display_offset(5);
            assert_eq!(state.display_offset(), 0, "cursor column 4 fits in 5");
            state.sync_display_offset(4);
            assert_eq!(state.display_offset(), 1, "column 4 no longer fits");
        }
    }

    mod rendering {
        use super::*;

        #[test]
        fn paints_the_value_from_the_window_start() {
            let mut state = state_with("hello");
            let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
            Input::new().render(Rect::new(0, 0, 6, 1), &mut buffer, &mut state);
            assert_eq!(buffer.to_string(), "hello ");
        }

        #[test]
        fn scrolled_window_shows_the_tail() {
            let mut state = state_with("hello world");
            for _ in 0..11 {
                state.move_left();
            }
            state.move_end();
            // The cursor at 11 in a 6-wide field scrolls the window to
            // offset 6: the tail "world" renders, cursor at column 5.
            let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
            Input::new().render(Rect::new(0, 0, 6, 1), &mut buffer, &mut state);
            assert_eq!(buffer.to_string(), "world ");
        }

        #[test]
        fn placeholder_renders_while_empty() {
            let mut state = InputState::new();
            let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
            Input::new().placeholder("your name").render(
                Rect::new(0, 0, 10, 1),
                &mut buffer,
                &mut state,
            );
            assert_eq!(buffer.to_string(), "your name ");
            assert_eq!(buffer.get(0, 0).unwrap().style.add_modifier, Modifier::DIM);
        }

        #[test]
        fn placeholder_style_is_overridable() {
            let mut state = InputState::new();
            let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
            Input::new()
                .placeholder("hint")
                .placeholder_style(Style::new().fg(Color::Green))
                .render(Rect::new(0, 0, 10, 1), &mut buffer, &mut state);
            assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Green));
        }

        #[test]
        fn value_style_applies_to_the_text() {
            let mut state = state_with("ab");
            let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
            Input::new().style(Style::new().fg(Color::Cyan)).render(
                Rect::new(0, 0, 4, 1),
                &mut buffer,
                &mut state,
            );
            assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Cyan));
        }

        #[test]
        fn block_wraps_and_fills_inner() {
            let mut state = state_with("ab");
            let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 3));
            Input::new().block(Block::bordered()).render(
                Rect::new(0, 0, 8, 3),
                &mut buffer,
                &mut state,
            );
            assert_eq!(buffer.to_string(), "┌──────┐\n│ab    │\n└──────┘");
        }

        #[test]
        fn empty_area_is_a_noop() {
            let mut state = state_with("ab");
            let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
            Input::new().render(Rect::ZERO, &mut buffer, &mut state);
            assert!(
                buffer
                    .content
                    .iter()
                    .all(tuvix_core::buffer::Cell::is_empty)
            );
        }

        #[test]
        fn renders_through_the_render_widget_contract() {
            let mut state = state_with("xy");
            let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
            <Input as RenderWidget>::render(
                Input::new(),
                Rect::new(0, 0, 4, 1),
                &mut buffer,
                &mut state,
            );
            assert_eq!(buffer.to_string(), "xy  ");
        }

        #[test]
        fn theme_maps_text_and_muted_slots() {
            let theme = Theme::dark();
            let input = Input::new().theme(&theme);
            assert_eq!(input.style, theme.text);
            assert_eq!(input.placeholder_style, theme.muted);
        }
    }

    mod cursor_position {
        use super::*;

        #[test]
        fn reports_the_cursor_cell() {
            let state = state_with("hello");
            let position = Input::new().cursor_position(Rect::new(0, 0, 10, 1), &state);
            assert_eq!(position, Position::new(5, 0));
        }

        #[test]
        fn accounts_for_the_block_inner_area() {
            let state = state_with("hi");
            let input = Input::new().block(Block::bordered());
            let position = input.cursor_position(Rect::new(0, 0, 10, 3), &state);
            assert_eq!(position, Position::new(3, 1), "inner starts at 1,1");
        }

        #[test]
        fn clamps_inside_the_field() {
            let state = state_with("hello");
            // The field sees a 3-wide window although the cursor is at 5.
            let position = Input::new().cursor_position(Rect::new(0, 0, 3, 1), &state);
            assert_eq!(position, Position::new(2, 0));
        }

        #[test]
        fn empty_field_places_the_cursor_at_the_field_start() {
            let state = InputState::new();
            let position = Input::new().cursor_position(Rect::new(4, 2, 10, 1), &state);
            assert_eq!(position, Position::new(4, 2));
        }
    }
}
