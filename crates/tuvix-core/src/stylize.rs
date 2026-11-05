//! Ergonomic styling sugar: `"hello".red().bold()`.
//!
//! Any type that owns a [`Style`] implements [`Styled`], which unlocks the
//! [`Stylize`] builder methods. String literals implement [`Stylize`] directly
//! and build a [`Span`] instead.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::style::{Color, Modifier, Style};
//! use tuvix_core::stylize::Stylize;
//! use tuvix_core::text::Span;
//!
//! let span = "hello".red().bold();
//! let expected = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
//! assert_eq!(span, Span::styled("hello", expected));
//!
//! let style = Style::new().green().italic();
//! assert_eq!(style.fg, Some(Color::Green));
//! ```

use crate::style::{Color, Modifier, Style};
use crate::text::{Line, Span, Text};

/// A type that owns a [`Style`] and can be mutated through it.
pub trait Styled {
    /// Returns a mutable reference to the style of `self`.
    fn style_mut(&mut self) -> &mut Style;
}

impl Styled for Style {
    fn style_mut(&mut self) -> &mut Style {
        self
    }
}

impl Styled for Span {
    fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}

impl Styled for Line {
    fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}

impl Styled for Text {
    fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}

/// Enables `modifier` on `style` and cancels any pending removal of it.
fn add_mod(style: &mut Style, modifier: Modifier) {
    style.add_modifier |= modifier;
    style.remove_modifier &= !modifier;
}

/// Disables `modifier` on `style` and cancels any pending addition of it.
fn remove_mod(style: &mut Style, modifier: Modifier) {
    style.remove_modifier |= modifier;
    style.add_modifier &= !modifier;
}

/// Fluent styling methods for [`Styled`] types and string literals.
///
/// For [`Styled`] types every method mutates the owned style and returns
/// `Self`. For `&str` every method builds a styled [`Span`].
pub trait Stylize: Sized {
    /// What the styling methods return: `Self` for [`Styled`] types,
    /// [`Span`] for `&str`.
    type Output;

    /// Sets the foreground color.
    fn fg(self, color: Color) -> Self::Output;
    /// Sets the background color.
    fn bg(self, color: Color) -> Self::Output;
    /// Adds a modifier.
    fn add_modifier(self, modifier: Modifier) -> Self::Output;
    /// Removes a modifier.
    fn remove_modifier(self, modifier: Modifier) -> Self::Output;

    /// Makes the text bold.
    fn bold(self) -> Self::Output;
    /// Dims the text.
    fn dim(self) -> Self::Output;
    /// Italicizes the text.
    fn italic(self) -> Self::Output;
    /// Underlines the text.
    fn underlined(self) -> Self::Output;
    /// Blinks slowly.
    fn slow_blink(self) -> Self::Output;
    /// Blinks rapidly.
    fn rapid_blink(self) -> Self::Output;
    /// Swaps foreground and background.
    fn reversed(self) -> Self::Output;
    /// Hides the text.
    fn hidden(self) -> Self::Output;
    /// Strikes the text through.
    fn crossed_out(self) -> Self::Output;

    /// Paints the text black.
    fn black(self) -> Self::Output;
    /// Paints the text red.
    fn red(self) -> Self::Output;
    /// Paints the text green.
    fn green(self) -> Self::Output;
    /// Paints the text yellow.
    fn yellow(self) -> Self::Output;
    /// Paints the text blue.
    fn blue(self) -> Self::Output;
    /// Paints the text magenta.
    fn magenta(self) -> Self::Output;
    /// Paints the text cyan.
    fn cyan(self) -> Self::Output;
    /// Paints the text gray.
    fn gray(self) -> Self::Output;
    /// Paints the text dark gray.
    fn dark_gray(self) -> Self::Output;
    /// Paints the text light red.
    fn light_red(self) -> Self::Output;
    /// Paints the text light green.
    fn light_green(self) -> Self::Output;
    /// Paints the text light yellow.
    fn light_yellow(self) -> Self::Output;
    /// Paints the text light blue.
    fn light_blue(self) -> Self::Output;
    /// Paints the text light magenta.
    fn light_magenta(self) -> Self::Output;
    /// Paints the text light cyan.
    fn light_cyan(self) -> Self::Output;
    /// Paints the text white.
    fn white(self) -> Self::Output;

    /// Paints the background black.
    fn on_black(self) -> Self::Output;
    /// Paints the background red.
    fn on_red(self) -> Self::Output;
    /// Paints the background green.
    fn on_green(self) -> Self::Output;
    /// Paints the background yellow.
    fn on_yellow(self) -> Self::Output;
    /// Paints the background blue.
    fn on_blue(self) -> Self::Output;
    /// Paints the background magenta.
    fn on_magenta(self) -> Self::Output;
    /// Paints the background cyan.
    fn on_cyan(self) -> Self::Output;
    /// Paints the background gray.
    fn on_gray(self) -> Self::Output;
    /// Paints the background dark gray.
    fn on_dark_gray(self) -> Self::Output;
    /// Paints the background light red.
    fn on_light_red(self) -> Self::Output;
    /// Paints the background light green.
    fn on_light_green(self) -> Self::Output;
    /// Paints the background light yellow.
    fn on_light_yellow(self) -> Self::Output;
    /// Paints the background light blue.
    fn on_light_blue(self) -> Self::Output;
    /// Paints the background light magenta.
    fn on_light_magenta(self) -> Self::Output;
    /// Paints the background light cyan.
    fn on_light_cyan(self) -> Self::Output;
    /// Paints the background white.
    fn on_white(self) -> Self::Output;
}

/// Generates the `Stylize` impl for a [`Styled`] type (`Output = Self`).
macro_rules! impl_stylize_styled {
    ($($t:ty),* $(,)?) => {
        $(
            impl Stylize for $t {
                type Output = Self;

                fn fg(mut self, color: Color) -> Self {
                    self.style_mut().fg = Some(color);
                    self
                }
                fn bg(mut self, color: Color) -> Self {
                    self.style_mut().bg = Some(color);
                    self
                }
                fn add_modifier(mut self, modifier: Modifier) -> Self {
                    add_mod(self.style_mut(), modifier);
                    self
                }
                fn remove_modifier(mut self, modifier: Modifier) -> Self {
                    remove_mod(self.style_mut(), modifier);
                    self
                }
                fn bold(self) -> Self {
                    self.add_modifier(Modifier::BOLD)
                }
                fn dim(self) -> Self {
                    self.add_modifier(Modifier::DIM)
                }
                fn italic(self) -> Self {
                    self.add_modifier(Modifier::ITALIC)
                }
                fn underlined(self) -> Self {
                    self.add_modifier(Modifier::UNDERLINED)
                }
                fn slow_blink(self) -> Self {
                    self.add_modifier(Modifier::SLOW_BLINK)
                }
                fn rapid_blink(self) -> Self {
                    self.add_modifier(Modifier::RAPID_BLINK)
                }
                fn reversed(self) -> Self {
                    self.add_modifier(Modifier::REVERSED)
                }
                fn hidden(self) -> Self {
                    self.add_modifier(Modifier::HIDDEN)
                }
                fn crossed_out(self) -> Self {
                    self.add_modifier(Modifier::CROSSED_OUT)
                }
                fn black(self) -> Self {
                    self.fg(Color::Black)
                }
                fn red(self) -> Self {
                    self.fg(Color::Red)
                }
                fn green(self) -> Self {
                    self.fg(Color::Green)
                }
                fn yellow(self) -> Self {
                    self.fg(Color::Yellow)
                }
                fn blue(self) -> Self {
                    self.fg(Color::Blue)
                }
                fn magenta(self) -> Self {
                    self.fg(Color::Magenta)
                }
                fn cyan(self) -> Self {
                    self.fg(Color::Cyan)
                }
                fn gray(self) -> Self {
                    self.fg(Color::Gray)
                }
                fn dark_gray(self) -> Self {
                    self.fg(Color::DarkGray)
                }
                fn light_red(self) -> Self {
                    self.fg(Color::LightRed)
                }
                fn light_green(self) -> Self {
                    self.fg(Color::LightGreen)
                }
                fn light_yellow(self) -> Self {
                    self.fg(Color::LightYellow)
                }
                fn light_blue(self) -> Self {
                    self.fg(Color::LightBlue)
                }
                fn light_magenta(self) -> Self {
                    self.fg(Color::LightMagenta)
                }
                fn light_cyan(self) -> Self {
                    self.fg(Color::LightCyan)
                }
                fn white(self) -> Self {
                    self.fg(Color::White)
                }
                fn on_black(self) -> Self {
                    self.bg(Color::Black)
                }
                fn on_red(self) -> Self {
                    self.bg(Color::Red)
                }
                fn on_green(self) -> Self {
                    self.bg(Color::Green)
                }
                fn on_yellow(self) -> Self {
                    self.bg(Color::Yellow)
                }
                fn on_blue(self) -> Self {
                    self.bg(Color::Blue)
                }
                fn on_magenta(self) -> Self {
                    self.bg(Color::Magenta)
                }
                fn on_cyan(self) -> Self {
                    self.bg(Color::Cyan)
                }
                fn on_gray(self) -> Self {
                    self.bg(Color::Gray)
                }
                fn on_dark_gray(self) -> Self {
                    self.bg(Color::DarkGray)
                }
                fn on_light_red(self) -> Self {
                    self.bg(Color::LightRed)
                }
                fn on_light_green(self) -> Self {
                    self.bg(Color::LightGreen)
                }
                fn on_light_yellow(self) -> Self {
                    self.bg(Color::LightYellow)
                }
                fn on_light_blue(self) -> Self {
                    self.bg(Color::LightBlue)
                }
                fn on_light_magenta(self) -> Self {
                    self.bg(Color::LightMagenta)
                }
                fn on_light_cyan(self) -> Self {
                    self.bg(Color::LightCyan)
                }
                fn on_white(self) -> Self {
                    self.bg(Color::White)
                }
            }
        )*
    };
}

/// Generates the `Stylize` impl for `&str` (`Output = Span`).
macro_rules! impl_stylize_str {
    () => {
        impl Stylize for &str {
            type Output = Span;

            fn fg(self, color: Color) -> Span {
                Span::shared(self, Style::new().fg(color))
            }
            fn bg(self, color: Color) -> Span {
                Span::shared(self, Style::new().bg(color))
            }
            fn add_modifier(self, modifier: Modifier) -> Span {
                Span::shared(self, Style::new().add_modifier(modifier))
            }
            fn remove_modifier(self, modifier: Modifier) -> Span {
                Span::shared(self, Style::new().remove_modifier(modifier))
            }
            fn bold(self) -> Span {
                self.add_modifier(Modifier::BOLD)
            }
            fn dim(self) -> Span {
                self.add_modifier(Modifier::DIM)
            }
            fn italic(self) -> Span {
                self.add_modifier(Modifier::ITALIC)
            }
            fn underlined(self) -> Span {
                self.add_modifier(Modifier::UNDERLINED)
            }
            fn slow_blink(self) -> Span {
                self.add_modifier(Modifier::SLOW_BLINK)
            }
            fn rapid_blink(self) -> Span {
                self.add_modifier(Modifier::RAPID_BLINK)
            }
            fn reversed(self) -> Span {
                self.add_modifier(Modifier::REVERSED)
            }
            fn hidden(self) -> Span {
                self.add_modifier(Modifier::HIDDEN)
            }
            fn crossed_out(self) -> Span {
                self.add_modifier(Modifier::CROSSED_OUT)
            }
            fn black(self) -> Span {
                self.fg(Color::Black)
            }
            fn red(self) -> Span {
                self.fg(Color::Red)
            }
            fn green(self) -> Span {
                self.fg(Color::Green)
            }
            fn yellow(self) -> Span {
                self.fg(Color::Yellow)
            }
            fn blue(self) -> Span {
                self.fg(Color::Blue)
            }
            fn magenta(self) -> Span {
                self.fg(Color::Magenta)
            }
            fn cyan(self) -> Span {
                self.fg(Color::Cyan)
            }
            fn gray(self) -> Span {
                self.fg(Color::Gray)
            }
            fn dark_gray(self) -> Span {
                self.fg(Color::DarkGray)
            }
            fn light_red(self) -> Span {
                self.fg(Color::LightRed)
            }
            fn light_green(self) -> Span {
                self.fg(Color::LightGreen)
            }
            fn light_yellow(self) -> Span {
                self.fg(Color::LightYellow)
            }
            fn light_blue(self) -> Span {
                self.fg(Color::LightBlue)
            }
            fn light_magenta(self) -> Span {
                self.fg(Color::LightMagenta)
            }
            fn light_cyan(self) -> Span {
                self.fg(Color::LightCyan)
            }
            fn white(self) -> Span {
                self.fg(Color::White)
            }
            fn on_black(self) -> Span {
                self.bg(Color::Black)
            }
            fn on_red(self) -> Span {
                self.bg(Color::Red)
            }
            fn on_green(self) -> Span {
                self.bg(Color::Green)
            }
            fn on_yellow(self) -> Span {
                self.bg(Color::Yellow)
            }
            fn on_blue(self) -> Span {
                self.bg(Color::Blue)
            }
            fn on_magenta(self) -> Span {
                self.bg(Color::Magenta)
            }
            fn on_cyan(self) -> Span {
                self.bg(Color::Cyan)
            }
            fn on_gray(self) -> Span {
                self.bg(Color::Gray)
            }
            fn on_dark_gray(self) -> Span {
                self.bg(Color::DarkGray)
            }
            fn on_light_red(self) -> Span {
                self.bg(Color::LightRed)
            }
            fn on_light_green(self) -> Span {
                self.bg(Color::LightGreen)
            }
            fn on_light_yellow(self) -> Span {
                self.bg(Color::LightYellow)
            }
            fn on_light_blue(self) -> Span {
                self.bg(Color::LightBlue)
            }
            fn on_light_magenta(self) -> Span {
                self.bg(Color::LightMagenta)
            }
            fn on_light_cyan(self) -> Span {
                self.bg(Color::LightCyan)
            }
            fn on_white(self) -> Span {
                self.bg(Color::White)
            }
        }
    };
}

impl_stylize_styled!(Style, Span, Line, Text);
impl_stylize_str!();

#[cfg(test)]
mod tests {
    use super::*;

    mod str_stylize {
        use super::*;

        #[test]
        fn red_builds_span() {
            let span = "hello".red();
            assert_eq!(span, Span::styled("hello", Style::new().fg(Color::Red)));
        }

        #[test]
        fn chained_builders_accumulate() {
            let span = "hello".red().bold().underlined();
            let expected = Style::new()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
            assert_eq!(span, Span::styled("hello", expected));
        }

        #[test]
        fn on_blue_sets_background() {
            let span = "hi".on_blue();
            assert_eq!(span.style.bg, Some(Color::Blue));
        }
    }

    mod styled_types {
        use super::*;

        #[test]
        fn style_is_stylizable() {
            let style = Style::new().green().italic();
            assert_eq!(style.fg, Some(Color::Green));
            assert_eq!(style.add_modifier, Modifier::ITALIC);
        }

        #[test]
        fn span_is_stylizable() {
            let mut span = Span::raw("x");
            span = span.blue().bold();
            assert_eq!(span.style.fg, Some(Color::Blue));
            assert!(span.style.add_modifier.contains(Modifier::BOLD));
        }

        #[test]
        fn line_and_text_are_stylizable() {
            let line = Line::raw("x").on_yellow();
            assert_eq!(line.style.bg, Some(Color::Yellow));
            let text = Text::raw("x").dim();
            assert!(text.style.add_modifier.contains(Modifier::DIM));
        }

        #[test]
        fn remove_modifier_strips_pending_addition() {
            let style = Style::new().bold().remove_modifier(Modifier::BOLD);
            assert_eq!(style.add_modifier, Modifier::empty());
            assert_eq!(style.remove_modifier, Modifier::BOLD);
        }

        #[test]
        fn rgb_via_fg_builder() {
            let style = Style::new().fg(Color::Rgb {
                r: 10,
                g: 20,
                b: 30,
            });
            assert_eq!(
                style.fg,
                Some(Color::Rgb {
                    r: 10,
                    g: 20,
                    b: 30
                })
            );
        }
    }
}
