//! Declarative inline markup: styled text written as plain strings.
//!
//! [`parse_line`] turns a small markup language into a [`Line`] of
//! [`Span`]s, so call sites keep literal text inline instead of wiring
//! builder chains:
//!
//! ```
//! use tuvix_core::markup::parse_line;
//! use tuvix_core::style::Color;
//!
//! let line = parse_line("build [green bold]ok[/] in [dim]2.1s[/]").unwrap();
//! assert_eq!(line.spans.len(), 4);
//! assert_eq!(line.spans[1].style.fg, Some(Color::Green));
//! assert_eq!(line.spans[1].as_str(), "ok");
//! ```
//!
//! # Grammar
//!
//! - `[` *spec* `]` opens a styled region; *spec* is whitespace-separated
//!   tokens (colors, `on <color>` backgrounds, `#rrggbb` hex, modifiers).
//! - `[/]` closes the innermost region. Regions nest; an inner region
//!   patches over its ancestors, and an outer region keeps styling the
//!   text after the inner one closes.
//! - `[[` is an escaped literal `[`. A lone `]` is literal.
//! - Errors are strict, so typos fail loudly: an unknown token, a tag
//!   that never closes, a `[/]` with no open region, or a `[` that does
//!   not form a tag all return [`MarkupError`].
//!
//! # The macros
//!
//! [`span!`](crate::span!), [`line!`](crate::line!) and
//! [`text!`](crate::text!) are the zero-parse sugar for building the same
//! types from expressions — the structural counterpart of the markup
//! parser.

use crate::style::{Color, Modifier, Style};
use crate::text::{Line, Span, Text};

/// The error returned by the markup parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkupError {
    /// A `[` that does not form a tag: no closing `]`, an empty spec, or
    /// a `[/` that does not close.
    InvalidTag {
        /// Byte offset of the offending `[`.
        at: usize,
    },
    /// A spec token that resolves to nothing (unknown color, modifier,
    /// or a malformed hex color).
    UnknownToken {
        /// The rejected token.
        token: String,
        /// Byte offset of the `[` that opened the tag.
        at: usize,
    },
    /// The input ended with at least one region still open.
    Unterminated {
        /// Byte offset of the `[` that opened the outermost open region.
        at: usize,
    },
    /// A `[/]` appeared with no open region.
    UnopenedClose {
        /// Byte offset of the `[/]`.
        at: usize,
    },
}

impl core::fmt::Display for MarkupError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidTag { at } => write!(f, "invalid markup tag at byte {at}"),
            Self::UnknownToken { token, at } => {
                write!(f, "unknown markup token `{token}` in tag at byte {at}")
            }
            Self::Unterminated { at } => {
                write!(f, "markup region opened at byte {at} is never closed")
            }
            Self::UnopenedClose { at } => write!(f, "`[/]` at byte {at} closes no region"),
        }
    }
}

impl core::error::Error for MarkupError {}

/// Parses one line of markup into a [`Line`].
///
/// Regions do not span newlines: a tag opened before a `\n` is
/// unterminated (error), not inherited by the next line. Use
/// [`parse_text`] for multi-line input.
///
/// # Examples
///
/// ```
/// use tuvix_core::markup::parse_line;
///
/// // A lone `]` is literal; `[[` escapes the bracket.
/// let line = parse_line("[[escaped] plain").unwrap();
/// assert_eq!(line.spans.len(), 1);
/// assert_eq!(line.spans[0].as_str(), "[escaped] plain");
/// ```
pub fn parse_line(input: &str) -> Result<Line, MarkupError> {
    let spans = parse_spans(input)?;
    Ok(Line::from_spans(spans))
}

/// Parses multi-line markup into a [`Text`], one [`Line`] per `\n`.
///
/// Every line is parsed independently (styles do not carry across
/// newlines) and errors report offsets within their own line.
///
/// # Examples
///
/// ```
/// use tuvix_core::markup::parse_text;
///
/// let text = parse_text("[red]top[/]\nbottom").unwrap();
/// assert_eq!(text.height(), 2);
/// assert_eq!(text.lines[1].spans[0].as_str(), "bottom");
/// ```
pub fn parse_text(input: &str) -> Result<Text, MarkupError> {
    let mut lines = Vec::new();
    for line in input.split('\n') {
        lines.push(parse_line(line)?);
    }
    Ok(Text::from_lines(lines))
}

/// Parses markup into a flat sequence of spans.
///
/// Adjacent text is merged into one span per region; empty regions
/// produce no spans.
///
/// # Examples
///
/// ```
/// use tuvix_core::markup::parse_spans;
///
/// let spans = parse_spans("[bold]hi[/]").unwrap();
/// assert_eq!(spans.len(), 1);
/// assert_eq!(spans[0].as_str(), "hi");
/// ```
pub fn parse_spans(input: &str) -> Result<Vec<Span>, MarkupError> {
    let mut spans: Vec<Span> = Vec::new();
    // Composed style of the region currently being written; the stack
    // holds the style active in every enclosing region (outermost first).
    let mut stack: Vec<(usize, Style)> = Vec::new();
    let mut current = Style::new();
    let mut text = String::new();
    let mut chars = input.char_indices().peekable();

    while let Some((index, c)) = chars.next() {
        match c {
            '[' => {
                let escaped = chars.peek().is_some_and(|&(_, next)| next == '[');
                if escaped {
                    chars.next();
                    text.push('[');
                    continue;
                }
                // The spec is everything up to the first `]`, consumed
                // straight from the iterator (no nesting inside a tag).
                let mut spec = String::new();
                let mut closed = false;
                for (_, sc) in chars.by_ref() {
                    if sc == ']' {
                        closed = true;
                        break;
                    }
                    spec.push(sc);
                }
                if !closed {
                    return Err(MarkupError::InvalidTag { at: index });
                }
                if spec == "/" {
                    if stack.is_empty() {
                        return Err(MarkupError::UnopenedClose { at: index });
                    }
                    flush(&mut text, current, &mut spans);
                    let (_, style) = stack.pop().unwrap_or_default();
                    current = style;
                    continue;
                }
                if spec.trim().is_empty() || spec.contains('[') {
                    return Err(MarkupError::InvalidTag { at: index });
                }
                let region = resolve_spec(&spec, index)?;
                flush(&mut text, current, &mut spans);
                stack.push((index, current));
                current = current.patch(region);
            }
            ']' => text.push(']'),
            c => text.push(c),
        }
    }

    if let Some(&(at, _)) = stack.first() {
        return Err(MarkupError::Unterminated { at });
    }
    flush(&mut text, current, &mut spans);
    Ok(spans)
}

/// Pushes the pending text as one span, unless it is empty.
fn flush(text: &mut String, style: Style, spans: &mut Vec<Span>) {
    if !text.is_empty() {
        spans.push(Span::styled(core::mem::take(text), style));
    }
}

/// Resolves a full tag spec (the text between `[` and `]`) into the style
/// it applies. `at` anchors errors to the opening bracket.
fn resolve_spec(spec: &str, at: usize) -> Result<Style, MarkupError> {
    let mut style = Style::new();
    let mut pending_bg = false;
    let mut first = true;
    for token in spec.split_whitespace() {
        // The spec is taken verbatim between the brackets, so a leading
        // token closes a stray `[/` form; everything else resolves.
        if first && token == "/" {
            first = false;
            continue;
        }
        first = false;
        let resolved = resolve_token(style, token, pending_bg);
        match resolved {
            Ok((next, still_pending)) => {
                style = next;
                pending_bg = still_pending;
            }
            Err(token) => {
                return Err(MarkupError::UnknownToken { token, at });
            }
        }
    }
    if pending_bg {
        // A trailing `on` with nothing after it.
        return Err(MarkupError::UnknownToken {
            token: "on".into(),
            at,
        });
    }
    Ok(style)
}

/// The named colors, keyed by their markup token.
fn named_color(token: &str) -> Option<Color> {
    let color = match token {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" => Color::Gray,
        "dark-gray" => Color::DarkGray,
        "light-red" => Color::LightRed,
        "light-green" => Color::LightGreen,
        "light-yellow" => Color::LightYellow,
        "light-blue" => Color::LightBlue,
        "light-magenta" => Color::LightMagenta,
        "light-cyan" => Color::LightCyan,
        "white" => Color::White,
        _ => return None,
    };
    Some(color)
}

/// Parses `#rgb` / `#rrggbb` into an RGB color.
fn hex_color(token: &str) -> Option<Color> {
    let digits = token.strip_prefix('#')?;
    let parse_channel = |hex: &str| u8::from_str_radix(hex, 16).ok();
    let (r, g, b) = match digits.len() {
        3 => {
            let mut channels = [0_u8; 3];
            for (channel, c) in digits.chars().enumerate() {
                let doubled: String = core::iter::repeat_n(c, 2).collect();
                channels[channel] = parse_channel(&doubled)?;
            }
            (channels[0], channels[1], channels[2])
        }
        6 => (
            parse_channel(&digits[0..2])?,
            parse_channel(&digits[2..4])?,
            parse_channel(&digits[4..6])?,
        ),
        _ => return None,
    };
    Some(Color::Rgb { r, g, b })
}

/// Resolves one whitespace-separated spec token against a style under
/// construction. `pending_bg` carries the `on` prefix to the next token.
fn resolve_token(style: Style, token: &str, pending_bg: bool) -> Result<(Style, bool), String> {
    // A color after `on` becomes the background color.
    if pending_bg {
        let color = named_color(token)
            .or_else(|| hex_color(token))
            .ok_or_else(|| token.to_owned())?;
        return Ok((style.bg(color), false));
    }
    if token == "on" {
        return Ok((style, true));
    }
    if let Some(color) = named_color(token) {
        return Ok((style.fg(color), false));
    }
    if let Some(color) = hex_color(token) {
        return Ok((style.fg(color), false));
    }
    let modifier = match token {
        "bold" => Modifier::BOLD,
        "dim" => Modifier::DIM,
        "italic" => Modifier::ITALIC,
        "underlined" => Modifier::UNDERLINED,
        "slow-blink" => Modifier::SLOW_BLINK,
        "rapid-blink" => Modifier::RAPID_BLINK,
        "reversed" => Modifier::REVERSED,
        "hidden" => Modifier::HIDDEN,
        "crossed-out" => Modifier::CROSSED_OUT,
        _ => return Err(token.to_owned()),
    };
    Ok((style.add_modifier(modifier), false))
}

#[macro_export]
/// Builds a [`Span`](crate::text::Span) without parsing.
///
/// - `span!(content)` — an unstyled span.
/// - `span!(style; content)` — a styled span; the style is anything
///   `Into<Style>` (a [`Style`](crate::style::Style), a
///   [`Color`](crate::style::Color), a [`Modifier`](crate::style::Modifier)).
///
/// ```
/// use tuvix_core::span;
/// use tuvix_core::style::{Color, Modifier, Style};
/// use tuvix_core::text::Span;
///
/// assert_eq!(span!("hi"), Span::raw("hi"));
/// assert_eq!(
///     span!(Color::Red; "hi"),
///     Span::styled("hi", Style::new().fg(Color::Red))
/// );
/// assert_eq!(
///     span!(Modifier::BOLD; "hi"),
///     Span::styled("hi", Style::new().add_modifier(Modifier::BOLD))
/// );
/// ```
macro_rules! span {
    ($content:expr $(,)?) => {
        $crate::text::Span::raw($content)
    };
    ($style:expr; $content:expr $(,)?) => {
        $crate::text::Span::styled(
            $content,
            ::core::convert::Into::<$crate::style::Style>::into($style),
        )
    };
}

#[macro_export]
/// Builds a [`Line`](crate::text::Line) from span-convertible parts.
///
/// ```
/// use tuvix_core::line;
/// use tuvix_core::text::Line;
///
/// let l = line!["a", "b"];
/// assert_eq!(l, Line::from_spans(vec!["a".into(), "b".into()]));
/// ```
macro_rules! line {
    [$($part:expr),* $(,)?] => {
        $crate::text::Line::from_spans(vec![$(
            $crate::text::Span::from($part)
        ),*])
    };
}

#[macro_export]
/// Builds a [`Text`](crate::text::Text) from line-convertible parts.
///
/// ```
/// use tuvix_core::text;
/// use tuvix_core::text::Text;
///
/// let t = text!["one", "two"];
/// assert_eq!(t, Text::from("one\ntwo"));
/// ```
macro_rules! text {
    [$($part:expr),* $(,)?] => {
        $crate::text::Text::from_lines(vec![$(
            $crate::text::Line::from($part)
        ),*])
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Color;

    fn fg(c: Color) -> Style {
        Style::new().fg(c)
    }

    mod errors {
        use super::*;

        #[test]
        fn unknown_token_is_rejected() {
            let err = parse_line("[blurple]hi[/]").unwrap_err();
            assert_eq!(
                err,
                MarkupError::UnknownToken {
                    token: "blurple".into(),
                    at: 0
                }
            );
        }

        #[test]
        fn empty_spec_is_invalid() {
            assert_eq!(
                parse_line("[]").unwrap_err(),
                MarkupError::InvalidTag { at: 0 }
            );
            assert_eq!(
                parse_line("[ ]").unwrap_err(),
                MarkupError::InvalidTag { at: 0 }
            );
        }

        #[test]
        fn tag_without_close_bracket_is_invalid() {
            assert_eq!(
                parse_line("[bold hi").unwrap_err(),
                MarkupError::InvalidTag { at: 0 }
            );
        }

        #[test]
        fn unterminated_region_is_rejected() {
            assert_eq!(
                parse_line("[bold]hi").unwrap_err(),
                MarkupError::Unterminated { at: 0 }
            );
            assert_eq!(
                parse_line("[red][bold]hi[/]").unwrap_err(),
                MarkupError::Unterminated { at: 0 }
            );
        }

        #[test]
        fn unopened_close_is_rejected() {
            assert_eq!(
                parse_line("hi[/]").unwrap_err(),
                MarkupError::UnopenedClose { at: 2 }
            );
            assert_eq!(
                parse_line("[bold]a[/][/]").unwrap_err(),
                MarkupError::UnopenedClose { at: 10 }
            );
        }

        #[test]
        fn malformed_hex_is_unknown_token() {
            let err = parse_line("[#12g45f]x[/]").unwrap_err();
            assert!(matches!(err, MarkupError::UnknownToken { token, .. } if token == "#12g45f"));
        }

        #[test]
        fn dangling_on_is_unknown_token() {
            let err = parse_line("[on]x[/]").unwrap_err();
            assert!(matches!(err, MarkupError::UnknownToken { token, .. } if token == "on"));
        }

        #[test]
        fn error_displays_context() {
            let err = parse_line("[blurple]hi[/]").unwrap_err();
            assert_eq!(
                err.to_string(),
                "unknown markup token `blurple` in tag at byte 0"
            );
        }

        #[test]
        fn styles_do_not_carry_across_lines_in_parse_text() {
            // The region is unterminated on its own line.
            assert!(parse_text("[red]top\nbottom").is_err());
        }
    }

    mod plain {
        use super::*;

        #[test]
        fn no_tags_is_one_raw_span() {
            let line = parse_line("hello world").unwrap();
            assert_eq!(line.spans.len(), 1);
            assert_eq!(line.spans[0], Span::raw("hello world"));
        }

        #[test]
        fn empty_input_produces_no_spans() {
            let line = parse_line("").unwrap();
            assert_eq!(line.spans.len(), 0);
        }

        #[test]
        fn lone_close_bracket_is_literal() {
            let line = parse_line("a]b").unwrap();
            assert_eq!(line.spans[0].as_str(), "a]b");
        }

        #[test]
        fn doubled_open_bracket_escapes() {
            let line = parse_line("[[tag] stays").unwrap();
            assert_eq!(line.spans.len(), 1);
            assert_eq!(line.spans[0].as_str(), "[tag] stays");
        }

        #[test]
        fn escaped_bracket_inside_a_region() {
            let line = parse_line("[bold]a [[ b[/] c").unwrap();
            assert_eq!(line.spans.len(), 2);
            assert_eq!(line.spans[0].as_str(), "a [ b");
            assert_eq!(line.spans[1].as_str(), " c");
        }

        #[test]
        fn adjacent_text_merges_into_one_span() {
            let line = parse_line("a[red]c[/]d").unwrap();
            assert_eq!(line.spans.len(), 3);
            assert_eq!(line.spans[0].as_str(), "a");
            assert_eq!(line.spans[2].as_str(), "d");
        }
    }

    mod colors {
        use super::*;

        #[test]
        fn named_color_sets_fg() {
            let line = parse_line("[red]x[/]").unwrap();
            assert_eq!(line.spans[0].style, fg(Color::Red));
        }

        #[test]
        fn all_named_colors_resolve() {
            let pairs = [
                ("black", Color::Black),
                ("red", Color::Red),
                ("green", Color::Green),
                ("yellow", Color::Yellow),
                ("blue", Color::Blue),
                ("magenta", Color::Magenta),
                ("cyan", Color::Cyan),
                ("gray", Color::Gray),
                ("dark-gray", Color::DarkGray),
                ("light-red", Color::LightRed),
                ("light-green", Color::LightGreen),
                ("light-yellow", Color::LightYellow),
                ("light-blue", Color::LightBlue),
                ("light-magenta", Color::LightMagenta),
                ("light-cyan", Color::LightCyan),
                ("white", Color::White),
            ];
            for (name, color) in pairs {
                let line = parse_line(&format!("[{name}]x[/]")).unwrap();
                assert_eq!(line.spans[0].style.fg, Some(color), "token {name}");
            }
        }

        #[test]
        fn on_sets_bg() {
            let line = parse_line("[on blue]x[/]").unwrap();
            assert_eq!(line.spans[0].style.bg, Some(Color::Blue));
            assert_eq!(line.spans[0].style.fg, None);
        }

        #[test]
        fn fg_and_bg_combine() {
            let line = parse_line("[white on blue]x[/]").unwrap();
            assert_eq!(line.spans[0].style.fg, Some(Color::White));
            assert_eq!(line.spans[0].style.bg, Some(Color::Blue));
        }

        #[test]
        fn hex_6_sets_fg() {
            let line = parse_line("[#ff8000]x[/]").unwrap();
            assert_eq!(
                line.spans[0].style.fg,
                Some(Color::Rgb {
                    r: 0xff,
                    g: 0x80,
                    b: 0x00
                })
            );
        }

        #[test]
        fn hex_3_expands_to_rgb() {
            let line = parse_line("[#f0a]x[/]").unwrap();
            assert_eq!(
                line.spans[0].style.fg,
                Some(Color::Rgb {
                    r: 0xff,
                    g: 0x00,
                    b: 0xaa
                })
            );
        }

        #[test]
        fn on_hex_sets_bg() {
            let line = parse_line("[on #010203]x[/]").unwrap();
            assert_eq!(
                line.spans[0].style.bg,
                Some(Color::Rgb {
                    r: 0x01,
                    g: 0x02,
                    b: 0x03
                })
            );
        }
    }

    mod modifiers {
        use super::*;
        use crate::style::Modifier as M;

        #[test]
        fn single_modifier() {
            let line = parse_line("[italic]x[/]").unwrap();
            assert_eq!(line.spans[0].style.add_modifier, M::ITALIC);
        }

        #[test]
        fn all_modifiers_resolve() {
            let pairs = [
                ("bold", M::BOLD),
                ("dim", M::DIM),
                ("italic", M::ITALIC),
                ("underlined", M::UNDERLINED),
                ("slow-blink", M::SLOW_BLINK),
                ("rapid-blink", M::RAPID_BLINK),
                ("reversed", M::REVERSED),
                ("hidden", M::HIDDEN),
                ("crossed-out", M::CROSSED_OUT),
            ];
            for (name, modifier) in pairs {
                let line = parse_line(&format!("[{name}]x[/]")).unwrap();
                assert_eq!(line.spans[0].style.add_modifier, modifier, "token {name}");
            }
        }

        #[test]
        fn modifiers_compose_in_one_tag() {
            let line = parse_line("[bold italic underlined]x[/]").unwrap();
            let expected = M::BOLD | M::ITALIC | M::UNDERLINED;
            assert_eq!(line.spans[0].style.add_modifier, expected);
        }
    }

    mod nesting {
        use super::*;

        #[test]
        fn regions_nest_and_patch() {
            let line = parse_line("[red]a[bold]b[/]c[/]").unwrap();
            assert_eq!(line.spans.len(), 3);
            assert_eq!(line.spans[0].style, fg(Color::Red));
            let inner = line.spans[1].style;
            assert_eq!(inner.fg, Some(Color::Red));
            assert_eq!(inner.add_modifier, Modifier::BOLD);
            // The outer region keeps styling after the inner closes.
            assert_eq!(line.spans[2].style, fg(Color::Red));
        }

        #[test]
        fn inner_color_overrides_outer() {
            let line = parse_line("[red]a[blue]b[/]c[/]").unwrap();
            assert_eq!(line.spans[1].style.fg, Some(Color::Blue));
            assert_eq!(line.spans[2].style.fg, Some(Color::Red));
        }

        #[test]
        fn deep_nesting_unwinds_in_order() {
            let line = parse_line("[red]1[green]2[blue]3[/]2b[/]1b[/]").unwrap();
            assert_eq!(line.spans.len(), 5);
            assert_eq!(line.spans[0].style.fg, Some(Color::Red));
            assert_eq!(line.spans[1].style.fg, Some(Color::Green));
            assert_eq!(line.spans[2].style.fg, Some(Color::Blue));
            assert_eq!(line.spans[3].style.fg, Some(Color::Green));
            assert_eq!(line.spans[4].style.fg, Some(Color::Red));
            assert_eq!(
                line.spans.iter().map(Span::as_str).collect::<Vec<_>>(),
                ["1", "2", "3", "2b", "1b"]
            );
        }

        #[test]
        fn empty_region_produces_no_span() {
            let line = parse_line("a[bold][/]").unwrap();
            assert_eq!(line.spans.len(), 1);
            assert_eq!(line.spans[0].as_str(), "a");
        }

        #[test]
        fn full_spec_combines_everything() {
            let line = parse_line("[#ff0000 on #0000ff bold italic]x[/]").unwrap();
            let style = line.spans[0].style;
            assert_eq!(style.fg, Some(Color::Rgb { r: 255, g: 0, b: 0 }));
            assert_eq!(style.bg, Some(Color::Rgb { r: 0, g: 0, b: 255 }));
            assert_eq!(style.add_modifier, Modifier::BOLD | Modifier::ITALIC);
        }
    }

    mod entries {
        use super::*;

        #[test]
        fn parse_line_returns_a_line() {
            let line = parse_line("[red]count[/]: 3").unwrap();
            assert_eq!(line.spans[0].style.fg, Some(Color::Red));
            assert_eq!(line.spans[1].as_str(), ": 3");
        }

        #[test]
        fn parse_text_splits_on_newlines() {
            let text = parse_text("one\ntwo\n[bold]three[/]").unwrap();
            assert_eq!(text.height(), 3);
            assert_eq!(text.lines[2].spans[0].style.add_modifier, Modifier::BOLD);
        }

        #[test]
        fn parse_spans_is_the_flat_form() {
            let spans = parse_spans("[red]a[/][blue]b[/]").unwrap();
            assert_eq!(spans.len(), 2);
            assert_eq!(spans[0].style.fg, Some(Color::Red));
            assert_eq!(spans[1].style.fg, Some(Color::Blue));
        }

        #[test]
        fn parse_text_trailing_newline_is_an_empty_last_line() {
            let text = parse_text("a\n").unwrap();
            assert_eq!(text.height(), 2);
            assert_eq!(text.lines[1].spans.len(), 0);
        }
    }

    mod macros {
        use super::*;
        use crate::style::Modifier as M;
        use crate::stylize::Stylize as _;

        #[test]
        fn span_bare_is_raw() {
            assert_eq!(span!("hi"), Span::raw("hi"));
            assert_eq!(
                span!(String::from("owned")),
                Span::raw(String::from("owned"))
            );
        }

        #[test]
        fn span_with_style_expr() {
            let expected = Span::styled("hi", Style::new().fg(Color::Red).add_modifier(M::BOLD));
            assert_eq!(span!(Style::new().fg(Color::Red).bold(); "hi"), expected);
        }

        #[test]
        fn span_with_color_literal() {
            assert_eq!(
                span!(Color::Cyan; "hi"),
                Span::styled("hi", fg(Color::Cyan))
            );
        }

        #[test]
        fn span_with_modifier_literal() {
            let expected = Span::styled("hi", Style::new().add_modifier(M::REVERSED));
            assert_eq!(span!(M::REVERSED; "hi"), expected);
        }

        #[test]
        fn line_makes_spans_of_parts() {
            let l = line!["ab", "汉"];
            assert_eq!(l.spans.len(), 2);
            assert_eq!(l.width(), 4);
        }

        #[test]
        fn line_accepts_styled_spans() {
            let l = line![span!(Color::Red; "x"), "y"];
            assert_eq!(l.spans[0].style.fg, Some(Color::Red));
        }

        #[test]
        fn text_makes_lines_of_parts() {
            let t = text!["one", "two", "three"];
            assert_eq!(t.height(), 3);
            assert_eq!(t, Text::from("one\ntwo\nthree"));
        }

        #[test]
        fn text_accepts_lines_and_mixed_parts() {
            let t = text![line!["a", "b"], "c"];
            assert_eq!(t.height(), 2);
        }
    }
}
