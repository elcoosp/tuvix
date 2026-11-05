//! Text primitives: spans, lines and styled multi-line text.
//!
//! - [`Span`] is a styled run of text.
//! - [`Line`] is a horizontal sequence of [`Span`]s with optional alignment.
//! - [`Text`] is a vertical sequence of [`Line`]s.
//!
//! Widths are measured with `unicode-width`, so CJK characters count as two
//! columns.
//!
//! # Zero-copy content (M1)
//!
//! Span content is a [`Cow<'static, str>`]: string literals become
//! [`Cow::Borrowed`] with **no allocation**, owned strings move in as
//! [`Cow::Owned`], and dynamic `&str` copies once through
//! [`Span::shared`]. Cloning a literal span is a pointer copy.

use std::borrow::Cow;

use unicode_width::UnicodeWidthStr as _;

use crate::style::Style;

/// The text storage of a [`Span`]: borrowed `&'static str` (literals, zero
/// cost) or an owned `String`.
pub type CowStr = Cow<'static, str>;

/// Horizontal alignment of a line inside its available width.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Alignment {
    /// Aligned to the left edge.
    #[default]
    Left,
    /// Centered in the available width.
    Center,
    /// Aligned to the right edge.
    Right,
}

/// A styled run of text.
///
/// # Examples
///
/// ```
/// use tuvix_core::style::{Color, Style};
/// use tuvix_core::text::Span;
///
/// let span = Span::styled("hello", Style::new().fg(Color::Red));
/// assert_eq!(span.width(), 5);
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct Span {
    /// The text content, kept as one grapheme-cluster sequence. Literals
    /// are borrowed (`&'static str`); see the [module docs](self).
    pub content: CowStr,
    /// The style applied to every cell the content occupies.
    pub style: Style,
}

impl Span {
    /// Creates an unstyled span.
    ///
    /// String literals are stored borrowed: `Span::raw("hi")` does not
    /// allocate.
    pub fn raw<T: Into<CowStr>>(content: T) -> Self {
        Self {
            content: content.into(),
            style: Style::new(),
        }
    }

    /// Creates a span with the given style. Literals do not allocate.
    pub fn styled<T: Into<CowStr>>(content: T, style: Style) -> Self {
        Self {
            content: content.into(),
            style,
        }
    }

    /// Creates a styled span from a runtime `&str`, copying it once.
    ///
    /// The only entry point for non-`'static` borrows; use [`Span::raw`]
    /// with literals or move an owned `String` in for the free paths.
    pub fn shared(content: &str, style: Style) -> Self {
        Self {
            content: Cow::Owned(content.to_owned()),
            style,
        }
    }

    /// Returns the content as a string slice.
    pub fn as_str(&self) -> &str {
        &self.content
    }

    /// Display width of the content in terminal columns.
    ///
    /// Wide characters (e.g. CJK ideographs) count as two columns.
    pub fn width(&self) -> u16 {
        self.content.width() as u16
    }
}

impl From<&'static str> for Span {
    fn from(content: &'static str) -> Self {
        Self::raw(content)
    }
}

impl From<String> for Span {
    fn from(content: String) -> Self {
        Self::raw(content)
    }
}

impl From<CowStr> for Span {
    fn from(content: CowStr) -> Self {
        Self::raw(content)
    }
}

impl AsRef<str> for Span {
    fn as_ref(&self) -> &str {
        self.content.as_ref()
    }
}

/// A horizontal sequence of spans, with an optional line-wide style and
/// alignment override.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct Line {
    /// The spans composing the line, rendered left to right.
    pub spans: Vec<Span>,
    /// Style layered under every span of the line.
    pub style: Style,
    /// Alignment override; `None` inherits the widget's alignment.
    pub alignment: Option<Alignment>,
}

impl Line {
    /// Creates a single-span, unstyled line. Literals do not allocate.
    pub fn raw<T: Into<CowStr>>(content: T) -> Self {
        Self {
            spans: vec![Span::raw(content)],
            ..Self::default()
        }
    }

    /// Creates a single-span line with the given style. Literals do not
    /// allocate.
    pub fn styled<T: Into<CowStr>>(content: T, style: Style) -> Self {
        Self {
            spans: vec![Span::styled(content, style)],
            ..Self::default()
        }
    }

    /// Creates a line from already-built spans.
    pub fn from_spans(spans: Vec<Span>) -> Self {
        Self {
            spans,
            ..Self::default()
        }
    }

    /// Sets the alignment of the line.
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Total display width of all spans in terminal columns.
    pub fn width(&self) -> u16 {
        self.spans.iter().map(Span::width).sum()
    }
}

impl From<&'static str> for Line {
    fn from(content: &'static str) -> Self {
        Self::raw(content)
    }
}

impl From<String> for Line {
    fn from(content: String) -> Self {
        Self::raw(content)
    }
}

impl From<Span> for Line {
    fn from(span: Span) -> Self {
        Self::from_spans(vec![span])
    }
}

/// A vertical sequence of lines with a text-wide base style.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct Text {
    /// The lines, rendered top to bottom.
    pub lines: Vec<Line>,
    /// Style layered under every line of the text.
    pub style: Style,
}

impl Text {
    /// Creates a single-line, unstyled text. Literals do not allocate.
    pub fn raw<T: Into<CowStr>>(content: T) -> Self {
        Self {
            lines: vec![Line::raw(content)],
            ..Self::default()
        }
    }

    /// Creates a text from already-built lines.
    pub fn from_lines(lines: Vec<Line>) -> Self {
        Self {
            lines,
            ..Self::default()
        }
    }

    /// Total number of lines.
    pub fn height(&self) -> usize {
        self.lines.len()
    }

    /// Display width of the widest line in terminal columns.
    pub fn width(&self) -> u16 {
        self.lines.iter().map(Line::width).max().unwrap_or(0)
    }
}

impl From<&'static str> for Text {
    /// Splits on newlines into multiple [`Line`]s.
    fn from(content: &'static str) -> Self {
        Self::from_lines(content.lines().map(|l| Line::raw(l.to_owned())).collect())
    }
}

impl From<String> for Text {
    /// Splits on newlines into multiple [`Line`]s.
    fn from(content: String) -> Self {
        Self::from_lines(content.lines().map(|l| Line::raw(l.to_owned())).collect())
    }
}

impl From<Line> for Text {
    fn from(line: Line) -> Self {
        Self::from_lines(vec![line])
    }
}

impl From<Vec<Line>> for Text {
    fn from(lines: Vec<Line>) -> Self {
        Self::from_lines(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Color, Modifier};

    mod span {
        use super::*;

        #[test]
        fn raw_has_no_style() {
            let span = Span::raw("hello");
            assert_eq!(span.content, "hello");
            assert_eq!(span.style, Style::new());
        }

        #[test]
        fn styled_carries_style() {
            let style = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
            let span = Span::styled("hello", style);
            assert_eq!(span.style, style);
        }

        #[test]
        fn width_measures_ascii() {
            assert_eq!(Span::raw("hello").width(), 5);
            assert_eq!(Span::raw("").width(), 0);
        }

        #[test]
        fn width_measures_wide_characters_as_two_columns() {
            assert_eq!(Span::raw("汉").width(), 2);
            assert_eq!(Span::raw("汉a字").width(), 5);
        }

        #[test]
        fn width_counts_combining_marks_as_one_column() {
            assert_eq!(Span::raw("e\u{301}").width(), 1);
        }

        #[test]
        fn from_str_creates_raw_span() {
            assert_eq!(Span::from("hi"), Span::raw("hi"));
        }

        #[test]
        fn as_ref_exposes_content() {
            assert_eq!(Span::raw("hi").as_ref(), "hi");
        }
    }

    mod zero_copy {
        use super::*;
        use std::borrow::Cow;

        #[test]
        fn literals_are_borrowed_with_no_allocation() {
            let span = Span::raw("hello");
            assert!(matches!(span.content, Cow::Borrowed("hello")));

            let styled = Span::styled("hello", Style::new().fg(Color::Red));
            assert!(matches!(styled.content, Cow::Borrowed("hello")));

            let line = Line::raw("hello");
            assert!(matches!(line.spans[0].content, Cow::Borrowed("hello")));

            let text = Text::raw("hello");
            assert!(matches!(
                text.lines[0].spans[0].content,
                Cow::Borrowed("hello")
            ));
        }

        #[test]
        fn owned_strings_move_in() {
            let span = Span::raw(String::from("hi"));
            assert!(matches!(span.content, Cow::Owned(_)));
            assert_eq!(span.as_str(), "hi");
        }

        #[test]
        fn shared_copies_runtime_borrows_once() {
            let runtime = format!("n-{}", 42);
            let span = Span::shared(&runtime, Style::new());
            assert!(matches!(span.content, Cow::Owned(_)));
            assert_eq!(span.as_str(), "n-42");
        }

        #[test]
        fn into_adapters_cover_all_ownership_shapes() {
            let borrowed: Span = "static".into();
            assert!(matches!(borrowed.content, Cow::Borrowed(_)));
            let owned: Span = String::from("owned").into();
            assert!(matches!(owned.content, Cow::Owned(_)));
            let cow: Span = Cow::from("still borrowed").into();
            assert!(matches!(cow.content, Cow::Borrowed(_)));
            let line: Line = "line".into();
            assert_eq!(line.spans[0].as_str(), "line");
            let text: Text = "a\nb".into();
            assert_eq!(text.height(), 2);
        }

        #[test]
        fn width_and_as_ref_still_work() {
            let span = Span::raw("汉字");
            assert_eq!(span.width(), 4);
            assert_eq!(span.as_ref(), "汉字");
            let line = Line::from_spans(vec![Span::raw("ab"), Span::raw("汉")]);
            assert_eq!(line.width(), 4);
        }

        #[test]
        fn clone_of_borrowed_content_is_cheap() {
            let span = Span::styled("literal", Style::new().fg(Color::Red));
            let copy = span.clone();
            assert!(matches!(copy.content, Cow::Borrowed("literal")));
        }
    }

    mod alignment {
        use super::*;

        #[test]
        fn default_is_left() {
            assert_eq!(Alignment::default(), Alignment::Left);
        }
    }

    mod line {
        use super::*;

        #[test]
        fn width_sums_span_widths() {
            let line = Line::from_spans(vec![Span::raw("ab"), Span::raw("汉"), Span::raw("c")]);
            assert_eq!(line.width(), 5);
        }

        #[test]
        fn styled_creates_single_span() {
            let line = Line::styled("hi", Style::new().fg(Color::Blue));
            assert_eq!(line.spans.len(), 1);
            assert_eq!(line.spans[0].style.fg, Some(Color::Blue));
        }

        #[test]
        fn alignment_builder_overrides() {
            let line = Line::raw("hi").alignment(Alignment::Center);
            assert_eq!(line.alignment, Some(Alignment::Center));
        }

        #[test]
        fn from_span_wraps() {
            let line = Line::from(Span::raw("hi"));
            assert_eq!(line.spans, vec![Span::raw("hi")]);
        }

        #[test]
        fn empty_line_has_zero_width() {
            assert_eq!(Line::default().width(), 0);
        }
    }

    mod text {
        use super::*;

        #[test]
        fn width_is_max_of_lines() {
            let text = Text::from_lines(vec![Line::raw("short"), Line::raw("a longer line")]);
            assert_eq!(text.width(), 13);
        }

        #[test]
        fn height_counts_lines() {
            let text = Text::from_lines(vec![Line::raw("a"), Line::raw("b"), Line::raw("c")]);
            assert_eq!(text.height(), 3);
        }

        #[test]
        fn empty_text_has_zero_width_and_height() {
            let text = Text::default();
            assert_eq!(text.width(), 0);
            assert_eq!(text.height(), 0);
        }

        #[test]
        fn from_str_splits_on_newlines() {
            let text = Text::from("a\nb\nc");
            assert_eq!(
                text.lines,
                vec![Line::raw("a"), Line::raw("b"), Line::raw("c")]
            );
            let single = Text::from("hello");
            assert_eq!(single.lines, vec![Line::raw("hello")]);
        }

        #[test]
        fn from_vec_of_lines() {
            let lines = vec![Line::raw("a"), Line::raw("b")];
            let text = Text::from(lines.clone());
            assert_eq!(text.lines, lines);
        }
    }
}
