//! Paragraphs: styled multi-line text with alignment, word wrapping and
//! scroll state.
//!
//! M1 added the text pipeline the M0 widget deferred: [`Paragraph::wrap`]
//! enables greedy word wrapping (see [`wrap_line`] for the exact
//! specification) and [`ScrollState`] scrolls a paragraph by columns and
//! rows. Since M3 scrolling is configuration ([`Paragraph::scroll`])
//! rather than a stateful contract — the ratatui-0.30 shape: scroll
//! offsets live in the widget, the app owns a `ScrollState` and applies
//! it each frame.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr as _;

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;
use tuvix_core::text::{Alignment, Line, Span, Text};
use tuvix_core::widget::Widget;

/// Greedy word-wrapping policy for a [`Paragraph`].
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::widget::Widget;
/// use tuvix_widgets::{Paragraph, Wrap};
///
/// // Drop indentation of wrapped lines (the default choice).
/// let paragraph = Paragraph::new("hello world").wrap(Wrap::trim(true));
///
/// // Preserve the leading whitespace of source lines.
/// let paragraph = Paragraph::new("  indented").wrap(Wrap::trim(false));
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Wrap {
    /// When `true`, whitespace runs at the start of a source line are
    /// dropped instead of preserved.
    pub trim: bool,
}

impl Wrap {
    /// A wrapping policy with the given trim behavior.
    #[must_use]
    pub fn trim(trim: bool) -> Self {
        Self { trim }
    }
}

/// A greedy word wrapper.
///
/// Specification (pinned by tests):
///
/// 1. Break opportunities are runs of space graphemes; a token is a
///    leading space run plus the word after it.
/// 2. Lines fill greedily: a word is placed on the current line when
///    `current + separator + word` fits; separators that would cross a
///    break are dropped (trailing spaces are invisible anyway).
/// 3. `trim = false` preserves the leading space run of a source line:
///    the first display line starts with it and the word is hard-broken
///    to fit behind it. `trim = true` drops that indentation.
/// 4. A word wider than the line hard-breaks at grapheme boundaries;
///    wide glyphs are never split (a two-column glyph that does not fit
///    moves to the next line).
/// 5. Styles are preserved: consecutive same-style graphemes re-group
///    into spans; the source line's style and alignment carry over.
/// 6. Empty source lines stay empty (one blank display line).
#[must_use]
pub fn wrap_line(line: &Line, max_width: u16, trim: bool) -> Vec<Line> {
    if max_width == 0 {
        return Vec::new();
    }
    if line.width() == 0 {
        return vec![Line {
            spans: Vec::new(),
            style: line.style,
            alignment: line.alignment,
        }];
    }

    let mut wrapper = Wrapper::new(line, max_width);

    // Token scanner: leading space run, then the word after it.
    let mut pending_sep: Vec<(String, Style)> = Vec::new();
    let mut pending_word: Vec<(String, Style)> = Vec::new();
    for span in &line.spans {
        for grapheme in span.content.graphemes(true) {
            if grapheme == " " {
                if !pending_word.is_empty() {
                    let word = core::mem::take(&mut pending_word);
                    let sep = core::mem::take(&mut pending_sep);
                    wrapper.place_word(sep, word, trim);
                }
                pending_sep.push((grapheme.to_string(), span.style));
            } else {
                pending_word.push((grapheme.to_string(), span.style));
            }
        }
    }
    if !pending_word.is_empty() {
        wrapper.place_word(pending_sep, pending_word, trim);
    }

    wrapper.finish()
}

/// Greedy line-builder state for [`wrap_line`].
struct Wrapper<'a> {
    line: &'a Line,
    max_width: u16,
    result: Vec<Line>,
    current: Vec<(String, Style)>,
    current_width: u16,
}

impl<'a> Wrapper<'a> {
    fn new(line: &'a Line, max_width: u16) -> Self {
        Self {
            line,
            max_width,
            result: Vec::new(),
            current: Vec::new(),
            current_width: 0,
        }
    }

    fn width_of(glyphs: &[(String, Style)]) -> u16 {
        glyphs
            .iter()
            .map(|(g, _)| u16::try_from(g.width()).unwrap_or(u16::MAX))
            .sum()
    }

    /// Groups consecutive same-style glyphs into spans and pushes a line.
    fn emit(&mut self, glyphs: &[(String, Style)]) {
        let mut spans: Vec<Span> = Vec::new();
        for (glyph, style) in glyphs {
            match spans.last_mut() {
                Some(span) if span.style == *style => span.content.to_mut().push_str(glyph),
                _ => spans.push(Span {
                    content: std::borrow::Cow::Owned(glyph.clone()),
                    style: *style,
                }),
            }
        }
        self.result.push(Line {
            spans,
            style: self.line.style,
            alignment: self.line.alignment,
        });
    }

    /// Hard-breaks `glyphs` into full lines, returning the trailing partial
    /// line content for the current line.
    fn hard_break(&mut self, glyphs: Vec<(String, Style)>) -> Vec<(String, Style)> {
        let mut rest: Vec<(String, Style)> = Vec::new();
        let mut rest_width = 0_u16;
        for (glyph, style) in glyphs {
            let width = u16::try_from(glyph.width()).unwrap_or(u16::MAX);
            if rest_width + width > self.max_width {
                self.emit(&rest);
                rest = Vec::new();
                rest_width = 0;
            }
            rest_width += width;
            rest.push((glyph, style));
        }
        rest
    }

    /// Places one word (with its leading separator) onto the lines.
    fn place_word(&mut self, sep: Vec<(String, Style)>, word: Vec<(String, Style)>, trim: bool) {
        let word_width = Self::width_of(&word);
        let sep_width = Self::width_of(&sep);

        if self.current.is_empty() {
            // Line start: preserve the separator unless trimming.
            if !trim && !sep.is_empty() {
                let stream: Vec<(String, Style)> = sep.into_iter().chain(word).collect();
                if Self::width_of(&stream) <= self.max_width {
                    self.current_width = Self::width_of(&stream);
                    self.current = stream;
                } else {
                    let rest = self.hard_break(stream);
                    self.current_width = Self::width_of(&rest);
                    self.current = rest;
                }
            } else if word_width <= self.max_width {
                self.current_width = word_width;
                self.current = word;
            } else {
                let rest = self.hard_break(word);
                self.current_width = Self::width_of(&rest);
                self.current = rest;
            }
        } else if self.current_width + sep_width + word_width <= self.max_width {
            self.current.extend(sep);
            self.current.extend(word);
            self.current_width += sep_width + word_width;
        } else {
            // Break before the separator: trailing spaces are never emitted.
            let current = core::mem::take(&mut self.current);
            self.emit(&current);
            self.current_width = 0;
            if word_width <= self.max_width {
                self.current_width = word_width;
                self.current = word;
            } else {
                let rest = self.hard_break(word);
                self.current_width = Self::width_of(&rest);
                self.current = rest;
            }
        }
    }

    fn finish(mut self) -> Vec<Line> {
        if !self.current.is_empty() {
            let current = core::mem::take(&mut self.current);
            self.emit(&current);
        }
        self.result
    }
}

/// A block of text rendered into an area, with a base style, an optional
/// alignment override, optional word wrapping and a scroll offset.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::text::Alignment;
/// use tuvix_core::widget::Widget;
/// use tuvix_widgets::{Paragraph, Wrap};
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
/// Paragraph::new("hi").alignment(Alignment::Right).render(Rect::new(0, 0, 6, 1), &mut buffer);
/// assert_eq!(buffer.to_string(), "    hi");
///
/// let mut wrapped = Buffer::empty(Rect::new(0, 0, 5, 2));
/// Paragraph::new("hello world").wrap(Wrap { trim: true }).render(Rect::new(0, 0, 5, 2), &mut wrapped);
/// assert_eq!(wrapped.to_string(), "hello\nworld");
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Paragraph {
    text: Text,
    style: Style,
    alignment: Option<Alignment>,
    wrap: Option<Wrap>,
    scroll: ScrollState,
}

impl Paragraph {
    /// Creates a paragraph from anything [`Text`]-convertible.
    pub fn new<T: Into<Text>>(text: T) -> Self {
        Self {
            text: text.into(),
            style: Style::new(),
            alignment: None,
            wrap: None,
            scroll: ScrollState::default(),
        }
    }

    /// Sets the base style layered under every line and span.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Overrides the alignment of every line without its own.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Enables word wrapping with the given policy.
    #[must_use]
    pub fn wrap(mut self, wrap: Wrap) -> Self {
        self.wrap = Some(wrap);
        self
    }

    /// Sets the scroll offsets: `state.x` columns off the left edge,
    /// `state.y` display lines off the top edge.
    #[must_use]
    pub fn scroll(mut self, scroll: ScrollState) -> Self {
        self.scroll = scroll;
        self
    }

    /// Paints the paragraph into `buffer`, clipped to `area`.
    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        self.render_scrolled(area, buffer, self.scroll.x, self.scroll.y);
    }

    /// Paints the paragraph scrolled by `scroll_x` columns and `scroll_y`
    /// display lines.
    fn render_scrolled(&self, area: Rect, buffer: &mut Buffer, scroll_x: u16, scroll_y: u16) {
        if area.is_empty() {
            return;
        }
        let display: Vec<Line> = match self.wrap {
            None => self.text.lines.clone(),
            Some(wrap) => self
                .text
                .lines
                .iter()
                .flat_map(|line| wrap_line(line, area.width, wrap.trim))
                .collect(),
        };

        for (index, line) in display.iter().skip(usize::from(scroll_y)).enumerate() {
            let y = area
                .y
                .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
            if y >= area.bottom() {
                break;
            }
            let alignment = line.alignment.or(self.alignment).unwrap_or_default();
            let line_width = line.width();
            let x = match alignment {
                Alignment::Left => area.x,
                Alignment::Center => {
                    area.x + area.width.saturating_sub(line_width.min(area.width)) / 2
                }
                Alignment::Right => area.x + area.width.saturating_sub(line_width.min(area.width)),
            };
            let max_width = area.right().saturating_sub(x);
            let line_style = self.style.patch(line.style);
            render_line_with_x_skip(line, x, y, max_width, scroll_x, line_style, buffer);
        }
    }
}

/// Writes `line`'s spans left to right, clipped to `max_width` columns,
/// after skipping `skip_x` leading columns of content.
///
/// A wide glyph straddling the skip boundary is skipped entirely: the
/// terminal cannot show half a glyph.
fn render_line_with_x_skip(
    line: &Line,
    x: u16,
    y: u16,
    max_width: u16,
    skip_x: u16,
    line_style: Style,
    buffer: &mut Buffer,
) {
    if max_width == 0 {
        return;
    }
    if skip_x == 0 {
        let mut cursor = x;
        let mut remaining = max_width;
        for span in &line.spans {
            if remaining == 0 {
                break;
            }
            let style = line_style.patch(span.style);
            let (end_x, _) = buffer.set_stringn(cursor, y, span.content.as_ref(), remaining, style);
            let used = end_x.saturating_sub(cursor);
            remaining = remaining.saturating_sub(used);
            cursor = end_x;
        }
        return;
    }
    // Scrolled path: every glyph lands at its exact screen column, so the
    // horizontal geometry is preserved. A wide glyph whose leading half
    // scrolled off the left edge leaves its visible trailing half blank
    // (terminals cannot show half a glyph); glyphs crossing the right
    // edge are clipped.
    let mut content_col = 0_u32;
    for span in &line.spans {
        let style = line_style.patch(span.style);
        for grapheme in span.content.graphemes(true) {
            let width = u16::try_from(grapheme.width()).unwrap_or(u16::MAX);
            if width == 0 {
                continue;
            }
            let screen_pos = i32::try_from(content_col).unwrap_or(i32::MAX) - i32::from(skip_x);
            content_col += u32::from(width);
            if screen_pos + i32::from(width) <= 0 {
                continue;
            }
            if screen_pos < 0 {
                continue;
            }
            let screen_pos = u16::try_from(screen_pos).unwrap_or(u16::MAX);
            if screen_pos >= max_width {
                break;
            }
            if screen_pos + width > max_width {
                continue;
            }
            buffer.set_stringn(x + screen_pos, y, grapheme, max_width - screen_pos, style);
        }
    }
}

/// Scroll offsets for a [`Paragraph`], applied through
/// [`Paragraph::scroll`].
///
/// The app owns the state, mutates it in response to events, and hands a
/// copy to the widget every frame:
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::widget::Widget;
/// use tuvix_widgets::{Paragraph, ScrollState};
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
/// let state = ScrollState { x: 0, y: 1 };
/// Paragraph::new("first\nsecond")
///     .scroll(state)
///     .render(Rect::new(0, 0, 10, 1), &mut buffer);
/// assert_eq!(buffer.to_string(), "second    ");
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScrollState {
    /// Columns scrolled off the left edge.
    pub x: u16,
    /// Display lines scrolled off the top edge.
    pub y: u16,
}

impl Widget for Paragraph {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Paragraph::render(&self, area, buffer);
    }
}

impl Paragraph {
    /// Maps the theme onto the paragraph: the `text` slot styles the text.
    #[must_use]
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.style = theme.text;
        self
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    fn buffer_10x3() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 10, 3))
    }

    fn lines_of(text: &str, width: u16, trim: bool) -> Vec<String> {
        let line = Line::raw(text.to_string());
        wrap_line(&line, width, trim)
            .iter()
            .map(|l| l.spans.iter().map(Span::as_str).collect::<String>())
            .collect()
    }

    mod wrapping {
        use super::*;

        #[test]
        fn splits_at_word_boundaries() {
            assert_eq!(
                lines_of("the quick brown fox", 10, true),
                vec!["the quick", "brown fox"]
            );
        }

        #[test]
        fn exact_fit_does_not_break() {
            assert_eq!(lines_of("abcdefghij", 10, true), vec!["abcdefghij"]);
            assert_eq!(lines_of("aa bb", 5, true), vec!["aa bb"]);
        }

        #[test]
        fn separator_crossing_a_break_is_dropped() {
            assert_eq!(lines_of("aa bb", 3, true), vec!["aa", "bb"]);
            assert_eq!(lines_of("ab  cd", 2, true), vec!["ab", "cd"]);
        }

        #[test]
        fn trim_drops_source_indentation() {
            assert_eq!(lines_of("   indented", 4, true), vec!["inde", "nted"]);
        }

        #[test]
        fn no_trim_preserves_source_indentation() {
            assert_eq!(
                lines_of("   indented", 4, false),
                vec!["   i", "nden", "ted"]
            );
        }

        #[test]
        fn wide_glyphs_are_never_split() {
            // 汉 is 2 columns wide: it never straddles a 3-column line.
            assert_eq!(lines_of("汉汉", 3, true), vec!["汉", "汉"]);
            assert_eq!(lines_of("a汉b", 3, true), vec!["a汉", "b"]);
        }

        #[test]
        fn cjk_runs_without_spaces_hard_break() {
            assert_eq!(lines_of("汉字汉", 4, true), vec!["汉字", "汉"]);
        }

        #[test]
        fn word_wider_than_the_line_hard_breaks() {
            assert_eq!(lines_of("abcdefgh", 3, true), vec!["abc", "def", "gh"]);
            assert_eq!(lines_of("ab cdefgh", 3, true), vec!["ab", "cde", "fgh"]);
        }

        #[test]
        fn styles_survive_the_wrap() {
            let line = Line::from_spans(vec![
                Span::raw("aa"),
                Span::styled(" bb", Style::new().fg(Color::Red)),
            ]);
            let wrapped = wrap_line(&line, 4, true);
            assert_eq!(wrapped.len(), 2);
            assert_eq!(wrapped[0].spans.len(), 1);
            assert_eq!(wrapped[0].spans[0].as_str(), "aa");
            assert_eq!(wrapped[1].spans[0].as_str(), "bb");
            assert_eq!(wrapped[1].spans[0].style.fg, Some(Color::Red));
        }

        #[test]
        fn empty_lines_stay_empty() {
            let line = Line::raw("");
            let wrapped = wrap_line(&line, 4, true);
            assert_eq!(wrapped.len(), 1);
            assert_eq!(wrapped[0].spans, [] as [Span; 0]);
        }

        #[test]
        fn zero_width_is_no_lines() {
            let line = Line::raw("abc");
            assert_eq!(wrap_line(&line, 0, true), [] as [Line; 0]);
        }

        #[test]
        fn alignment_and_line_style_carry_over() {
            let line =
                Line::styled("aa bb", Style::new().fg(Color::Blue)).alignment(Alignment::Center);
            let wrapped = wrap_line(&line, 2, true);
            assert!(
                wrapped
                    .iter()
                    .all(|l| l.alignment == Some(Alignment::Center))
            );
            // The span styles carry over; the line-wide style stays default.
            assert!(
                wrapped
                    .iter()
                    .all(|l| l.spans.iter().all(|s| s.style.fg == Some(Color::Blue)))
            );
        }
    }

    mod paragraph {
        use super::*;

        #[test]
        fn renders_single_line_at_origin() {
            let mut buffer = buffer_10x3();
            Paragraph::new("hi").render(Rect::new(0, 0, 10, 3), &mut buffer);
            let rendered = buffer.to_string();
            let rows: Vec<&str> = rendered.lines().collect();
            assert_eq!(rows[0], "hi        ");
        }

        #[test]
        fn renders_multiple_lines() {
            let mut buffer = buffer_10x3();
            Paragraph::new("a\nb\nc").render(Rect::new(0, 0, 10, 3), &mut buffer);
            let rendered = buffer.to_string();
            let rows: Vec<&str> = rendered.lines().collect();
            assert_eq!(rows, vec!["a         ", "b         ", "c         "]);
        }

        #[test]
        fn truncates_at_area_width_without_wrap() {
            let mut buffer = buffer_10x3();
            Paragraph::new("0123456789abc").render(Rect::new(0, 0, 10, 1), &mut buffer);
            assert_eq!(buffer.to_string(), "0123456789\n          \n          ");
        }

        #[test]
        fn wraps_across_rows_when_enabled() {
            let mut buffer = buffer_10x3();
            Paragraph::new("the quick brown fox")
                .wrap(Wrap::trim(true))
                .render(Rect::new(0, 0, 10, 3), &mut buffer);
            let rendered = buffer.to_string();
            let rows: Vec<&str> = rendered.lines().collect();
            assert_eq!(rows, vec!["the quick ", "brown fox ", "          "]);
        }

        #[test]
        fn respects_offset_area() {
            let mut buffer = buffer_10x3();
            Paragraph::new("hi").render(Rect::new(4, 1, 4, 1), &mut buffer);
            let rendered = buffer.to_string();
            let rows: Vec<&str> = rendered.lines().collect();
            assert_eq!(rows[1], "    hi    ");
        }

        #[test]
        fn clips_at_area_edge_when_offset() {
            let mut buffer = buffer_10x3();
            Paragraph::new("abcdef").render(Rect::new(5, 0, 3, 1), &mut buffer);
            let rendered = buffer.to_string();
            let rows: Vec<&str> = rendered.lines().collect();
            assert_eq!(rows[0], "     abc  ");
        }

        #[test]
        fn right_alignment_offsets_to_edge() {
            let mut buffer = buffer_10x3();
            Paragraph::new("hi")
                .alignment(Alignment::Right)
                .render(Rect::new(0, 0, 10, 1), &mut buffer);
            assert_eq!(buffer.to_string(), "        hi\n          \n          ");
        }

        #[test]
        fn center_alignment_offsets_to_middle() {
            let mut buffer = buffer_10x3();
            Paragraph::new("hi")
                .alignment(Alignment::Center)
                .render(Rect::new(0, 0, 10, 1), &mut buffer);
            assert_eq!(buffer.to_string(), "    hi    \n          \n          ");
        }

        #[test]
        fn per_line_alignment_overrides_paragraph() {
            let mut buffer = buffer_10x3();
            let text = Text::from_lines(vec![
                Line::raw("l").alignment(Alignment::Left),
                Line::raw("c").alignment(Alignment::Center),
            ]);
            Paragraph::new(text)
                .alignment(Alignment::Right)
                .render(Rect::new(0, 0, 10, 2), &mut buffer);
            let rendered = buffer.to_string();
            let rows: Vec<&str> = rendered.lines().collect();
            assert_eq!(rows[0], "l         ");
            assert_eq!(rows[1], "    c     ");
        }

        #[test]
        fn styles_layer_paragraph_line_span() {
            let mut buffer = buffer_10x3();
            let mut line = Line::from_spans(vec![Span::styled("x", Style::new().fg(Color::Green))]);
            line.alignment = Some(Alignment::Left);
            let text = Text::from_lines(vec![line]);
            Paragraph::new(text)
                .style(
                    Style::new()
                        .fg(Color::Red)
                        .add_modifier(tuvix_core::style::Modifier::BOLD),
                )
                .render(Rect::new(0, 0, 5, 1), &mut buffer);
            let cell = buffer.get(0, 0).unwrap();
            assert_eq!(cell.style.fg, Some(Color::Green));
            assert!(
                cell.style
                    .add_modifier
                    .contains(tuvix_core::style::Modifier::BOLD)
            );
        }

        #[test]
        fn wide_characters_render_and_clip() {
            let mut buffer = buffer_10x3();
            Paragraph::new("汉a汉").render(Rect::new(0, 0, 5, 1), &mut buffer);
            let rendered = buffer.to_string();
            let rows: Vec<&str> = rendered.lines().collect();
            assert_eq!(rows[0], "汉·a汉·     ");
        }

        #[test]
        fn spans_flow_left_to_right() {
            let mut buffer = buffer_10x3();
            let line = Line::from_spans(vec![
                Span::raw("ab"),
                Span::styled("cd", Style::new().fg(Color::Red)),
                Span::raw("e"),
            ]);
            Paragraph::new(line).render(Rect::new(0, 0, 10, 1), &mut buffer);
            let rendered = buffer.to_string();
            let rows: Vec<&str> = rendered.lines().collect();
            assert_eq!(rows[0], "abcde     ");
            assert_eq!(buffer.get(2, 0).unwrap().style.fg, Some(Color::Red));
            assert_eq!(buffer.get(4, 0).unwrap().style.fg, None);
        }

        #[test]
        fn empty_paragraph_is_noop() {
            let mut buffer = buffer_10x3();
            Paragraph::new("").render(Rect::new(0, 0, 10, 3), &mut buffer);
            assert!(
                buffer
                    .content
                    .iter()
                    .all(tuvix_core::buffer::Cell::is_empty)
            );
        }

        #[test]
        fn rendering_into_empty_area_is_noop() {
            let mut buffer = buffer_10x3();
            Paragraph::new("hi").render(Rect::ZERO, &mut buffer);
            assert!(
                buffer
                    .content
                    .iter()
                    .all(tuvix_core::buffer::Cell::is_empty)
            );
        }
    }

    mod scrolling {
        use super::*;

        #[test]
        fn vertical_scroll_skips_display_lines() {
            let mut buffer = buffer_10x3();
            Paragraph::new("first\nsecond")
                .scroll(ScrollState { x: 0, y: 1 })
                .render(Rect::new(0, 0, 10, 1), &mut buffer);
            assert_eq!(buffer.to_string(), "second    \n          \n          ");
        }

        #[test]
        fn wrapped_lines_scroll_as_display_lines() {
            let mut buffer = buffer_10x3();
            Paragraph::new("aaaa bbbb")
                .wrap(Wrap::trim(true))
                .scroll(ScrollState { x: 0, y: 1 })
                .render(Rect::new(0, 0, 4, 2), &mut buffer);
            // Display lines: "aaaa", "bbbb" — scrolled by 1 shows "bbbb".
            let rendered = buffer.to_string();
            let rows: Vec<&str> = rendered.lines().collect();
            assert_eq!(rows[0], "bbbb      ");
            assert_eq!(rows[1], "          ");
        }

        #[test]
        fn horizontal_scroll_skips_columns() {
            let mut buffer = buffer_10x3();
            Paragraph::new("abcdef")
                .scroll(ScrollState { x: 2, y: 0 })
                .render(Rect::new(0, 0, 4, 1), &mut buffer);
            assert_eq!(buffer.to_string(), "cdef      \n          \n          ");
        }

        #[test]
        fn horizontal_scroll_keeps_column_geometry() {
            let mut buffer = buffer_10x3();
            // 汉 covers content columns 0-1; scrolled by 1 its trailing half
            // lands on screen column 0 as a blank, never split.
            Paragraph::new("汉ab")
                .scroll(ScrollState { x: 1, y: 0 })
                .render(Rect::new(0, 0, 3, 1), &mut buffer);
            let rendered = buffer.to_string();
            let rows: Vec<&str> = rendered.lines().collect();
            assert_eq!(rows[0], " ab       ");
        }

        #[test]
        fn beyond_the_end_renders_nothing() {
            let mut buffer = buffer_10x3();
            Paragraph::new("only")
                .scroll(ScrollState { x: 0, y: 5 })
                .render(Rect::new(0, 0, 10, 3), &mut buffer);
            assert!(
                buffer
                    .content
                    .iter()
                    .all(tuvix_core::buffer::Cell::is_empty)
            );
        }

        #[test]
        fn zero_scroll_matches_the_stateless_render() {
            let mut a = buffer_10x3();
            Paragraph::new("hello world").render(Rect::new(0, 0, 10, 2), &mut a);
            let mut b = buffer_10x3();
            Paragraph::new("hello world")
                .scroll(ScrollState::default())
                .render(Rect::new(0, 0, 10, 2), &mut b);
            assert_eq!(a.to_string(), b.to_string());
        }

        #[test]
        fn scroll_builder_feeds_the_frame_state_pattern() {
            // The app-owns-state pattern: mutate ScrollState between frames
            // and feed a copy to the widget each frame.
            let mut buffer = buffer_10x3();
            let mut state = ScrollState::default();
            state.y += 1;
            Paragraph::new("first\nsecond")
                .scroll(state)
                .render(Rect::new(0, 0, 10, 1), &mut buffer);
            assert_eq!(buffer.to_string(), "second    \n          \n          ");
        }
    }

    mod widget_trait {
        use super::*;
        use tuvix_core::widget::Widget as WidgetTrait;

        #[test]
        fn paragraph_renders_through_the_widget_trait() {
            let mut buffer = buffer_10x3();
            WidgetTrait::render(Paragraph::new("trait"), Rect::new(0, 0, 10, 1), &mut buffer);
            assert_eq!(buffer.to_string(), "trait     \n          \n          ");
        }

        #[test]
        fn owned_paragraph_renders_through_the_widget_trait() {
            let mut buffer = buffer_10x3();
            WidgetTrait::render(Paragraph::new("owned"), Rect::new(0, 0, 10, 1), &mut buffer);
            assert_eq!(buffer.to_string(), "owned     \n          \n          ");
        }
    }
}
