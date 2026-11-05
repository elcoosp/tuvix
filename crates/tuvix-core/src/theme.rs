//! The theme system: named style slots resolved once, applied everywhere.
//!
//! A [`Theme`] is a record of named [`Style`] slots (text, border, gauge,
//! highlight, ...). Widgets consume it through their `.theme(&theme)`
//! builders, mapping each slot to the style it owns; apps pick a preset or
//! build their own. Three presets ship: [`Theme::dark`], [`Theme::light`]
//! and [`Theme::monochrome`] (default colors + modifiers only).
//!
//! [`Theme::from_env`] honors `NO_COLOR`: when the variable is set and
//! non-empty (the no-color.org convention, and what the crossterm backend
//! already honors), it resolves to the monochrome preset — the whole
//! framework stays readable without color.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::style::{Color, Modifier, Style};
//! use tuvix_core::theme::Theme;
//!
//! // A preset, tuned per app through the builder.
//! let theme = Theme::dark().accent(Color::Cyan).muted(Style::new());
//!
//! // Or let the environment decide (NO_COLOR -> monochrome).
//! let theme = Theme::from_env();
//! assert!(theme.accent.fg.is_some() || theme.accent.add_modifier != Modifier::empty());
//! ```

use crate::style::{Color, Modifier, Style};

/// Named style slots consumed by widgets through `.theme(&theme)`.
///
/// Every slot is a plain [`Style`]; unset fields mean "terminal default",
/// which is what the monochrome preset uses (plus modifiers for emphasis).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Primary text.
    pub text: Style,
    /// Secondary, dimmed text.
    pub muted: Style,
    /// Block borders.
    pub border: Style,
    /// Block titles and table headers.
    pub title: Style,
    /// Selected rows in List and Table.
    pub highlight: Style,
    /// Gauge fill.
    pub gauge: Style,
    /// Sparkline bars.
    pub sparkline: Style,
    /// Chart axes.
    pub axis: Style,
    /// Scrollbar track and thumb.
    pub scrollbar: Style,
    /// Positive states.
    pub success: Style,
    /// Caution states.
    pub warning: Style,
    /// Failure states.
    pub error: Style,
    /// Focus accents (cursor position, active pane, gauge labels).
    pub accent: Style,
}

impl Default for Theme {
    /// The dark preset: themes should never panic or fall back to unstyled.
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    /// The dark preset: bright text on default (dark) background.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuvix_core::style::{Color, Modifier, Style};
    /// use tuvix_core::theme::Theme;
    ///
    /// let theme = Theme::dark();
    /// assert_eq!(theme.text.fg, Some(Color::Gray));
    /// assert_eq!(theme.accent.fg, Some(Color::Cyan));
    /// assert_eq!(theme.gauge.fg, Some(Color::Green));
    /// assert_eq!(theme.error.fg, Some(Color::LightRed));
    /// ```
    pub fn dark() -> Self {
        Self {
            text: Style::new().fg(Color::Gray),
            muted: Style::new().fg(Color::DarkGray),
            border: Style::new().fg(Color::DarkGray),
            title: Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            highlight: Style::new().bg(Color::DarkGray).fg(Color::White),
            gauge: Style::new().fg(Color::Green),
            sparkline: Style::new().fg(Color::Blue),
            axis: Style::new().fg(Color::DarkGray),
            scrollbar: Style::new().fg(Color::DarkGray),
            success: Style::new().fg(Color::LightGreen),
            warning: Style::new().fg(Color::LightYellow),
            error: Style::new().fg(Color::LightRed),
            accent: Style::new().fg(Color::Cyan),
        }
    }

    /// The light preset: dark text on default (light) background.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuvix_core::style::Color;
    /// use tuvix_core::theme::Theme;
    ///
    /// let theme = Theme::light();
    /// assert_eq!(theme.text.fg, Some(Color::Black));
    /// assert_eq!(theme.muted.fg, Some(Color::Gray));
    /// ```
    pub fn light() -> Self {
        Self {
            text: Style::new().fg(Color::Black),
            muted: Style::new().fg(Color::Gray),
            border: Style::new().fg(Color::Gray),
            title: Style::new().fg(Color::Black).add_modifier(Modifier::BOLD),
            highlight: Style::new().bg(Color::Gray).fg(Color::Black),
            gauge: Style::new().fg(Color::Green),
            sparkline: Style::new().fg(Color::Blue),
            axis: Style::new().fg(Color::Gray),
            scrollbar: Style::new().fg(Color::Gray),
            success: Style::new().fg(Color::Green),
            warning: Style::new().fg(Color::Yellow),
            error: Style::new().fg(Color::Red),
            accent: Style::new().fg(Color::Blue),
        }
    }

    /// The monochrome preset: terminal-default colors, emphasis through
    /// modifiers only. This is what `NO_COLOR` resolves to.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuvix_core::style::{Color, Modifier, Style};
    /// use tuvix_core::theme::Theme;
    ///
    /// let theme = Theme::monochrome();
    /// assert_eq!(theme.text.fg, None);
    /// assert_eq!(theme.title.add_modifier, Modifier::BOLD);
    /// // Emphasis never comes from color:
    /// for slot in [
    ///     &theme.text, &theme.muted, &theme.border, &theme.title,
    ///     &theme.highlight, &theme.gauge, &theme.sparkline, &theme.axis,
    ///     &theme.scrollbar, &theme.success, &theme.warning, &theme.error,
    ///     &theme.accent,
    /// ] {
    ///     assert_eq!(slot.fg, None);
    ///     assert_eq!(slot.bg, None);
    /// }
    /// ```
    pub fn monochrome() -> Self {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let reversed = Style::new().add_modifier(Modifier::REVERSED);
        let underlined = Style::new().add_modifier(Modifier::UNDERLINED);
        Self {
            text: Style::new(),
            muted: Style::new().add_modifier(Modifier::DIM),
            border: Style::new(),
            title: bold,
            highlight: reversed,
            gauge: reversed,
            sparkline: Style::new(),
            axis: Style::new().add_modifier(Modifier::DIM),
            scrollbar: Style::new(),
            success: bold,
            warning: underlined,
            error: reversed.add_modifier(Modifier::BOLD),
            accent: underlined,
        }
    }

    /// Resolves the preset from the process environment: `NO_COLOR` set and
    /// non-empty selects the monochrome preset (the no-color.org convention
    /// the crossterm backend also honors); otherwise the dark preset.
    ///
    /// Resolution rules live in the pure [`Theme::resolve`]; see there for
    /// pinned examples.
    pub fn from_env() -> Self {
        Self::resolve(std::env::var_os("NO_COLOR").as_deref())
    }

    /// The pure resolver behind [`Theme::from_env`]: a present, non-empty
    /// `NO_COLOR` value selects monochrome; anything else selects dark.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::ffi::OsStr;
    /// use tuvix_core::theme::Theme;
    ///
    /// assert_eq!(Theme::resolve(None), Theme::dark());
    /// assert_eq!(Theme::resolve(Some(OsStr::new(""))), Theme::dark());
    /// assert_eq!(
    ///     Theme::resolve(Some(OsStr::new("1"))),
    ///     Theme::monochrome()
    /// );
    /// ```
    pub fn resolve(no_color: Option<&std::ffi::OsStr>) -> Self {
        if no_color.is_some_and(|value| !value.is_empty()) {
            Self::monochrome()
        } else {
            Self::dark()
        }
    }

    /// Overrides the `text` slot (builder style).
    #[must_use]
    pub fn text(mut self, style: Style) -> Self {
        self.text = style;
        self
    }

    /// Overrides the `muted` slot (builder style).
    #[must_use]
    pub fn muted(mut self, style: Style) -> Self {
        self.muted = style;
        self
    }

    /// Overrides the `border` slot (builder style).
    #[must_use]
    pub fn border(mut self, style: Style) -> Self {
        self.border = style;
        self
    }

    /// Overrides the `title` slot (builder style).
    #[must_use]
    pub fn title(mut self, style: Style) -> Self {
        self.title = style;
        self
    }

    /// Overrides the `highlight` slot (builder style).
    #[must_use]
    pub fn highlight(mut self, style: Style) -> Self {
        self.highlight = style;
        self
    }

    /// Overrides the `gauge` slot (builder style).
    #[must_use]
    pub fn gauge(mut self, style: Style) -> Self {
        self.gauge = style;
        self
    }

    /// Overrides the `sparkline` slot (builder style).
    #[must_use]
    pub fn sparkline(mut self, style: Style) -> Self {
        self.sparkline = style;
        self
    }

    /// Overrides the `axis` slot (builder style).
    #[must_use]
    pub fn axis(mut self, style: Style) -> Self {
        self.axis = style;
        self
    }

    /// Overrides the `scrollbar` slot (builder style).
    #[must_use]
    pub fn scrollbar(mut self, style: Style) -> Self {
        self.scrollbar = style;
        self
    }

    /// Overrides the `success` slot (builder style).
    #[must_use]
    pub fn success(mut self, style: Style) -> Self {
        self.success = style;
        self
    }

    /// Overrides the `warning` slot (builder style).
    #[must_use]
    pub fn warning(mut self, style: Style) -> Self {
        self.warning = style;
        self
    }

    /// Overrides the `error` slot (builder style).
    #[must_use]
    pub fn error(mut self, style: Style) -> Self {
        self.error = style;
        self
    }

    /// Overrides the `accent` slot with a foreground color (builder sugar).
    #[must_use]
    pub fn accent(mut self, color: Color) -> Self {
        self.accent = Style::new().fg(color);
        self
    }

    /// Overrides the `gauge` slot with a foreground color (builder sugar).
    #[must_use]
    pub fn gauge_fg(mut self, color: Color) -> Self {
        self.gauge = Style::new().fg(color);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_preset_pins_every_slot() {
        let theme = Theme::dark();
        assert_eq!(theme.text.fg, Some(Color::Gray));
        assert_eq!(theme.muted.fg, Some(Color::DarkGray));
        assert_eq!(theme.border.fg, Some(Color::DarkGray));
        assert_eq!(theme.title.fg, Some(Color::White));
        assert_eq!(theme.title.add_modifier, Modifier::BOLD);
        assert_eq!(theme.highlight.bg, Some(Color::DarkGray));
        assert_eq!(theme.gauge.fg, Some(Color::Green));
        assert_eq!(theme.sparkline.fg, Some(Color::Blue));
        assert_eq!(theme.axis.fg, Some(Color::DarkGray));
        assert_eq!(theme.scrollbar.fg, Some(Color::DarkGray));
        assert_eq!(theme.success.fg, Some(Color::LightGreen));
        assert_eq!(theme.warning.fg, Some(Color::LightYellow));
        assert_eq!(theme.error.fg, Some(Color::LightRed));
        assert_eq!(theme.accent.fg, Some(Color::Cyan));
    }

    #[test]
    fn light_preset_pins_every_slot() {
        let theme = Theme::light();
        assert_eq!(theme.text.fg, Some(Color::Black));
        assert_eq!(theme.muted.fg, Some(Color::Gray));
        assert_eq!(theme.highlight.bg, Some(Color::Gray));
        assert_eq!(theme.accent.fg, Some(Color::Blue));
        assert_eq!(theme.error.fg, Some(Color::Red));
    }

    #[test]
    fn monochrome_uses_no_color_anywhere() {
        let theme = Theme::monochrome();
        let slots = [
            &theme.text,
            &theme.muted,
            &theme.border,
            &theme.title,
            &theme.highlight,
            &theme.gauge,
            &theme.sparkline,
            &theme.axis,
            &theme.scrollbar,
            &theme.success,
            &theme.warning,
            &theme.error,
            &theme.accent,
        ];
        for slot in slots {
            assert_eq!(slot.fg, None, "monochrome slot carries a color: {slot:?}");
            assert_eq!(slot.bg, None, "monochrome slot carries a color: {slot:?}");
        }
        // Emphasis exists through modifiers.
        assert_eq!(theme.title.add_modifier, Modifier::BOLD);
        assert_eq!(theme.highlight.add_modifier, Modifier::REVERSED);
        assert_eq!(theme.muted.add_modifier, Modifier::DIM);
    }

    #[test]
    fn builder_overrides_only_named_slots() {
        let theme = Theme::dark()
            .accent(Color::Magenta)
            .gauge_fg(Color::Yellow)
            .border(Style::new().fg(Color::Black));
        assert_eq!(theme.accent.fg, Some(Color::Magenta));
        assert_eq!(theme.gauge.fg, Some(Color::Yellow));
        assert_eq!(theme.border.fg, Some(Color::Black));
        // Untouched slots keep their preset values.
        assert_eq!(theme.text, Theme::dark().text);
        assert_eq!(theme.error, Theme::dark().error);
    }

    #[test]
    fn default_is_dark() {
        assert_eq!(Theme::default(), Theme::dark());
    }

    #[test]
    fn themes_are_debug_cloneable_and_comparable() {
        let theme = Theme::dark();
        let cloned = theme.clone();
        assert_eq!(theme, cloned);
        assert!(format!("{theme:?}").contains("Theme"));
    }
}
