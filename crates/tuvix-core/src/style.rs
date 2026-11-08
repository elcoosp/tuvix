//! Colors, modifiers and styles for terminal cells.
//!
//! A [`Style`] stores optional colors plus *modifier deltas* (`add` and
//! `remove`), so styles layer through [`Style::patch`] without losing
//! information.

use bitflags::bitflags;

/// Terminal colors: the 16 ANSI colors, 8-bit indexed and 24-bit RGB.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    /// The terminal's default color.
    #[default]
    Reset,
    /// ANSI color 0.
    Black,
    /// ANSI color 1.
    Red,
    /// ANSI color 2.
    Green,
    /// ANSI color 3.
    Yellow,
    /// ANSI color 4.
    Blue,
    /// ANSI color 5.
    Magenta,
    /// ANSI color 6.
    Cyan,
    /// ANSI color 7.
    Gray,
    /// Bright black (ANSI 8).
    DarkGray,
    /// Bright red (ANSI 9).
    LightRed,
    /// Bright green (ANSI 10).
    LightGreen,
    /// Bright yellow (ANSI 11).
    LightYellow,
    /// Bright blue (ANSI 12).
    LightBlue,
    /// Bright magenta (ANSI 13).
    LightMagenta,
    /// Bright cyan (ANSI 14).
    LightCyan,
    /// Bright white (ANSI 15).
    White,
    /// 24-bit true color.
    Rgb {
        /// Red channel.
        r: u8,
        /// Green channel.
        g: u8,
        /// Blue channel.
        b: u8,
    },
    /// 8-bit indexed palette entry (0-255).
    Indexed(u8),
}

bitflags! {
    /// Text attributes combinable with bitwise operators.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Modifier: u16 {
        /// Bold / increased intensity.
        const BOLD = 1 << 0;
        /// Dim / decreased intensity.
        const DIM = 1 << 1;
        /// Italic.
        const ITALIC = 1 << 2;
        /// Underlined.
        const UNDERLINED = 1 << 3;
        /// Slow blink.
        const SLOW_BLINK = 1 << 4;
        /// Rapid blink.
        const RAPID_BLINK = 1 << 5;
        /// Swapped foreground and background.
        const REVERSED = 1 << 6;
        /// Invisible.
        const HIDDEN = 1 << 7;
        /// Strikethrough.
        const CROSSED_OUT = 1 << 8;
    }
}

/// A hyperlink attached to a cell, rendered with the OSC 8 protocol
/// (`ESC ] 8 ; params ; URI ST` — the standard for terminal hyperlinks).
///
/// The URI is reference-counted: cells are cloned by the thousands on
/// resize and `Symbol`/`Style` stay `Copy`-sized, so the link lives behind
/// an `Arc` and cloning a cell never copies the URI.
///
/// Control characters (C0 and DEL) are stripped on construction: they
/// cannot appear in a URI and would break the OSC 8 framing of the output
/// stream.
///
/// # Examples
///
/// ```
/// use tuvix_core::style::Hyperlink;
///
/// let link = Hyperlink::new("https://tuvix.dev");
/// assert_eq!(link.as_str(), "https://tuvix.dev");
///
/// // Framing characters are sanitized away, never passed through.
/// let injected = Hyperlink::new("https://x.dev/\u{1b}8;;evil");
/// assert!(!injected.as_str().contains('\u{1b}'));
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct Hyperlink(std::sync::Arc<str>);

impl Hyperlink {
    /// Builds a hyperlink from `uri`, stripping control characters.
    pub fn new<T: AsRef<str>>(uri: T) -> Self {
        let sanitized: String = uri
            .as_ref()
            .chars()
            .filter(|&c| !c.is_ascii_control() && c != '\u{7f}')
            .collect();
        Self(std::sync::Arc::from(sanitized))
    }

    /// The URI of the link.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<T: AsRef<str>> From<T> for Hyperlink {
    fn from(uri: T) -> Self {
        Self::new(uri)
    }
}

impl core::fmt::Display for Hyperlink {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The visual style of a terminal cell.
///
/// Colors are `Option`s so that [`Style::patch`] can layer styles: only the
/// fields explicitly set by the upper layer override the lower one.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Style {
    /// Foreground color, if set.
    pub fg: Option<Color>,
    /// Background color, if set.
    pub bg: Option<Color>,
    /// Underline color, if set.
    pub underline_color: Option<Color>,
    /// Modifiers to enable.
    pub add_modifier: Modifier,
    /// Modifiers to disable.
    pub remove_modifier: Modifier,
}

impl Style {
    /// A style with nothing set.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuvix_core::style::Style;
    ///
    /// let style = Style::new().fg(tuvix_core::style::Color::Red);
    /// assert_eq!(style.fg, Some(tuvix_core::style::Color::Red));
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self {
            fg: None,
            bg: None,
            underline_color: None,
            add_modifier: Modifier::empty(),
            remove_modifier: Modifier::empty(),
        }
    }

    /// Sets the foreground color.
    #[must_use]
    pub const fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }

    /// Sets the background color.
    #[must_use]
    pub const fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }

    /// Sets the underline color.
    #[must_use]
    pub const fn underline_color(mut self, color: Color) -> Self {
        self.underline_color = Some(color);
        self
    }

    /// Enables `modifier` and cancels any pending removal of it.
    #[must_use]
    pub fn add_modifier(mut self, modifier: Modifier) -> Self {
        self.add_modifier |= modifier;
        self.remove_modifier &= !modifier;
        self
    }

    /// Disables `modifier` and cancels any pending addition of it.
    #[must_use]
    pub fn remove_modifier(mut self, modifier: Modifier) -> Self {
        self.remove_modifier |= modifier;
        self.add_modifier &= !modifier;
        self
    }

    /// Layers `other` on top of `self`: fields explicitly set by `other` win.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuvix_core::style::{Color, Style};
    ///
    /// let base = Style::new().fg(Color::Red).bg(Color::Blue);
    /// let upper = Style::new().fg(Color::Green);
    /// let patched = base.patch(upper);
    /// assert_eq!(patched.fg, Some(Color::Green));
    /// assert_eq!(patched.bg, Some(Color::Blue));
    /// ```
    #[must_use]
    pub fn patch(mut self, other: Self) -> Self {
        self.fg = other.fg.or(self.fg);
        self.bg = other.bg.or(self.bg);
        self.underline_color = other.underline_color.or(self.underline_color);
        self.add_modifier = (self.add_modifier & !other.remove_modifier) | other.add_modifier;
        self.remove_modifier = (self.remove_modifier & !other.add_modifier) | other.remove_modifier;
        self
    }
}

impl From<Color> for Style {
    fn from(color: Color) -> Self {
        Self::new().fg(color)
    }
}

impl From<Modifier> for Style {
    fn from(modifier: Modifier) -> Self {
        Self::new().add_modifier(modifier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod color {
        use super::*;

        #[test]
        fn default_is_reset() {
            assert_eq!(Color::default(), Color::Reset);
        }

        #[test]
        fn rgb_carries_channels() {
            let c = Color::Rgb {
                r: 0x12,
                g: 0x34,
                b: 0x56,
            };
            assert!(matches!(
                c,
                Color::Rgb {
                    r: 0x12,
                    g: 0x34,
                    b: 0x56
                }
            ));
        }

        #[test]
        fn indexed_carries_value() {
            assert_eq!(Color::Indexed(231), Color::Indexed(231));
        }
    }

    mod modifier {
        use super::*;

        #[test]
        fn empty_is_default() {
            assert_eq!(Modifier::default(), Modifier::empty());
        }

        #[test]
        fn bits_are_distinct() {
            let all = [
                Modifier::BOLD,
                Modifier::DIM,
                Modifier::ITALIC,
                Modifier::UNDERLINED,
                Modifier::SLOW_BLINK,
                Modifier::RAPID_BLINK,
                Modifier::REVERSED,
                Modifier::HIDDEN,
                Modifier::CROSSED_OUT,
            ];
            let combined = all.iter().fold(Modifier::empty(), |acc, m| acc | *m);
            for m in all {
                assert!(combined.contains(m));
            }
            assert_eq!(combined.bits(), 0x01FF);
        }

        #[test]
        fn union_and_difference() {
            let m = Modifier::BOLD | Modifier::ITALIC;
            assert!(m.contains(Modifier::BOLD));
            assert!(!m.contains(Modifier::DIM));
            let m = m - Modifier::BOLD;
            assert_eq!(m, Modifier::ITALIC);
        }
    }

    mod style {
        use super::*;

        #[test]
        fn new_is_empty() {
            let s = Style::new();
            assert_eq!(s.fg, None);
            assert_eq!(s.bg, None);
            assert_eq!(s.underline_color, None);
            assert_eq!(s.add_modifier, Modifier::empty());
            assert_eq!(s.remove_modifier, Modifier::empty());
        }

        #[test]
        fn builders_set_colors() {
            let s = Style::new()
                .fg(Color::Red)
                .bg(Color::Blue)
                .underline_color(Color::Green);
            assert_eq!(s.fg, Some(Color::Red));
            assert_eq!(s.bg, Some(Color::Blue));
            assert_eq!(s.underline_color, Some(Color::Green));
        }

        #[test]
        fn add_then_remove_leaves_pending_removal() {
            // The removal delta survives so that patching can strip an
            // inherited BOLD from lower layers.
            let s = Style::new()
                .add_modifier(Modifier::BOLD)
                .remove_modifier(Modifier::BOLD);
            assert_eq!(s.add_modifier, Modifier::empty());
            assert_eq!(s.remove_modifier, Modifier::BOLD);
        }

        #[test]
        fn remove_then_add_cancels_out() {
            let s = Style::new()
                .remove_modifier(Modifier::BOLD)
                .add_modifier(Modifier::BOLD);
            assert_eq!(s.add_modifier, Modifier::BOLD);
            assert_eq!(s.remove_modifier, Modifier::empty());
        }

        #[test]
        fn patch_upper_layer_colors_win() {
            let base = Style::new().fg(Color::Red).bg(Color::Blue);
            let upper = Style::new().fg(Color::Green);
            let patched = base.patch(upper);
            assert_eq!(patched.fg, Some(Color::Green));
            assert_eq!(patched.bg, Some(Color::Blue));
        }

        #[test]
        fn patch_lower_layer_kept_when_upper_unset() {
            let base = Style::new().fg(Color::Red);
            assert_eq!(base.patch(Style::new()), base);
        }

        #[test]
        fn patch_merges_modifier_deltas() {
            let base = Style::new().add_modifier(Modifier::BOLD);
            let upper = Style::new().add_modifier(Modifier::ITALIC);
            let patched = base.patch(upper);
            assert_eq!(patched.add_modifier, Modifier::BOLD | Modifier::ITALIC);
        }

        #[test]
        fn patch_upper_removal_cancels_lower_addition() {
            let base = Style::new().add_modifier(Modifier::BOLD);
            let upper = Style::new().remove_modifier(Modifier::BOLD);
            let patched = base.patch(upper);
            assert_eq!(patched.add_modifier, Modifier::empty());
            // The removal delta survives patching: the upper layer intends to
            // strip BOLD from anything below it as well.
            assert_eq!(patched.remove_modifier, Modifier::BOLD);
        }

        #[test]
        fn from_color_sets_foreground() {
            assert_eq!(Style::from(Color::Red), Style::new().fg(Color::Red));
        }

        #[test]
        fn from_modifier_adds_modifier() {
            assert_eq!(
                Style::from(Modifier::BOLD),
                Style::new().add_modifier(Modifier::BOLD)
            );
        }

        #[test]
        fn color_conversion_is_lossless() {
            let s = Style::new()
                .fg(Color::Indexed(42))
                .bg(Color::Rgb { r: 1, g: 2, b: 3 });
            assert_eq!(s.fg, Some(Color::Indexed(42)));
            assert_eq!(s.bg, Some(Color::Rgb { r: 1, g: 2, b: 3 }));
        }
    }
}
