//! Select: a closed trigger that opens an option list below itself.
//!
//! The options and the committed choice are the app's data; the
//! navigation state (open flag, highlighted row, committed selection)
//! lives in [`SelectState`], and the [`Select`] widget is the paint
//! configuration (label, styles, indicators, block). The contract is the
//! M6 `Input` honesty: the state's navigation ops are pure, the widget
//! reports nothing back but cells, and the app decides when a choice is
//! final ([`SelectState::choose`] commits the highlighted row and
//! closes — what `Enter` means in an app).
//!
//! Navigation takes the option count (`select_next(count)`), the same
//! honesty as `ListState::select_last(count)` and the M7 tree ops: the
//! state never hides a copy of the options.

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;
use tuvix_core::text::{Line, Span};
use tuvix_core::widget::RenderWidget;

use crate::block::Block;

/// The navigation state of a [`Select`]: open flag, highlight, choice.
///
/// # Examples
///
/// ```
/// use tuvix_widgets::SelectState;
///
/// let mut state = SelectState::new();
/// state.open();
/// assert!(state.is_open());
/// state.select_next(3);
/// state.select_next(3);
/// state.select_next(3); // clamped at the last option
/// assert_eq!(state.highlight(), 2);
/// assert_eq!(state.choose(), Some(2), "choose commits and closes");
/// assert!(!state.is_open());
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SelectState {
    open: bool,
    highlight: usize,
    selected: Option<usize>,
}

impl SelectState {
    /// A closed dropdown with nothing selected.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder mirror of [`SelectState::set_selected`].
    pub fn with_selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Whether the option list is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The highlighted row (the keyboard/mouse cursor of the open list).
    pub fn highlight(&self) -> usize {
        self.highlight
    }

    /// The committed selection, if any.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Replaces the committed selection without touching the highlight;
    /// opening re-syncs the highlight to it.
    pub fn set_selected(&mut self, selected: Option<usize>) {
        self.selected = selected;
    }

    /// Opens the list, placing the highlight on the committed selection
    /// (or the first row when nothing is selected).
    pub fn open(&mut self) {
        self.open = true;
        self.highlight = self.selected.unwrap_or(0);
    }

    /// Closes the list without committing; the next open re-syncs the
    /// highlight to the committed selection.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Opens when closed, closes when open.
    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open();
        }
    }

    /// Moves the highlight one row down, clamped at `count - 1`.
    pub fn select_next(&mut self, count: usize) {
        let last = count.saturating_sub(1);
        self.highlight = (self.highlight + 1).min(last);
    }

    /// Moves the highlight one row up, clamped at the top.
    pub fn select_previous(&mut self) {
        self.highlight = self.highlight.saturating_sub(1);
    }

    /// Moves the highlight to `index` verbatim; the renderer clamps for
    /// painting and the app is expected to keep it in range through the
    /// navigation ops.
    pub fn set_highlight(&mut self, index: usize) {
        self.highlight = index;
    }

    /// Commits the highlighted row as the selection and closes the list.
    /// Returns the new selection; a closed dropdown selects nothing.
    pub fn choose(&mut self) -> Option<usize> {
        if !self.open {
            return None;
        }
        self.selected = Some(self.highlight);
        self.open = false;
        self.selected
    }
}

/// A dropdown: a trigger row showing the committed choice, with the
/// option list below while open.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_widgets::{Select, SelectState};
///
/// let mut state = SelectState::new().with_selected(Some(1));
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 16, 1));
/// Select::new(["eu-west", "eu-north", "us-east"])
///     .label("region")
///     .render(Rect::new(0, 0, 16, 1), &mut buffer, &mut state);
/// assert_eq!(buffer.to_string(), "region: eu-north");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Select {
    label: Option<Line>,
    options: Vec<Line>,
    placeholder: Option<Line>,
    style: Style,
    highlight_style: Style,
    indicator_closed: Span,
    indicator_open: Span,
    block: Option<Block>,
}

impl Default for Select {
    fn default() -> Self {
        Self {
            label: None,
            options: Vec::new(),
            placeholder: None,
            style: Style::new(),
            highlight_style: Style::new(),
            indicator_closed: Span::raw("▾"),
            indicator_open: Span::raw("▴"),
            block: None,
        }
    }
}

impl Select {
    /// A dropdown over the given options, closed, nothing selected.
    pub fn new<T, C>(options: C) -> Self
    where
        T: Into<Line>,
        C: IntoIterator<Item = T>,
    {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// A caption rendered before the value (`label: value ▾`).
    pub fn label<T: Into<Line>>(mut self, label: T) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The line rendered in place of the value while nothing is selected.
    pub fn placeholder<T: Into<Line>>(mut self, placeholder: T) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// The base style of the trigger and the unhighlighted options.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The style of the highlighted row of the open list.
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// The indicator rendered after the value while closed (default `▾`).
    pub fn indicator_closed<T: Into<Span>>(mut self, indicator: T) -> Self {
        self.indicator_closed = indicator.into();
        self
    }

    /// The indicator rendered after the value while open (default `▴`).
    pub fn indicator_open<T: Into<Span>>(mut self, indicator: T) -> Self {
        self.indicator_open = indicator.into();
        self
    }

    /// Wraps the dropdown in a block; trigger and list fill the inner area.
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Maps the theme: the `text` slot styles the trigger and options,
    /// the `highlight` slot the highlighted row.
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.style = theme.text;
        self.highlight_style = theme.highlight;
        self
    }

    /// Paints the dropdown into `buffer`, clipped to `area`, reading
    /// `state` for the open flag, the highlight and the committed choice.
    ///
    /// Row one is the trigger (`label: value indicator`); while open, one
    /// option renders per row below, the highlighted one in
    /// [`Select::highlight_style`]. Everything clips at the inner area —
    /// an open list in a one-row area shows only the trigger.
    pub fn render(&self, area: Rect, buffer: &mut Buffer, state: &SelectState) {
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
        let right = inner.right();
        let row = inner.y;
        let mut x = inner.x;

        if let Some(label) = &self.label {
            for span in &label.spans {
                if x >= right {
                    break;
                }
                x = buffer
                    .set_stringn(
                        x,
                        row,
                        span.content.as_ref(),
                        right - x,
                        self.style.patch(span.style),
                    )
                    .0;
            }
            if x < right {
                x = buffer.set_stringn(x, row, ": ", right - x, self.style).0;
            }
        }

        // The committed choice renders where a placeholder would: an
        // out-of-range selection leaves the value empty, it never falls
        // back to the placeholder.
        let value: Option<&Line> = match state.selected() {
            Some(index) => self.options.get(index),
            None => self.placeholder.as_ref(),
        };
        if let Some(value) = value {
            for span in &value.spans {
                if x >= right {
                    break;
                }
                x = buffer
                    .set_stringn(
                        x,
                        row,
                        span.content.as_ref(),
                        right - x,
                        self.style.patch(span.style),
                    )
                    .0;
            }
        }

        if x < right {
            x = buffer.set_stringn(x, row, " ", right - x, self.style).0;
        }
        if x < right {
            let indicator = if state.is_open() {
                &self.indicator_open
            } else {
                &self.indicator_closed
            };
            buffer.set_stringn(
                x,
                row,
                indicator.content.as_ref(),
                right - x,
                self.style.patch(indicator.style),
            );
        }

        if !state.is_open() {
            return;
        }
        for (index, option) in self.options.iter().enumerate() {
            let option_row = inner.y + 1 + index as u16;
            if option_row >= inner.bottom() {
                break;
            }
            // Out-of-range highlights render no row — the same honesty
            // as the Tabs and List selections.
            let style = if index == state.highlight() {
                self.style.patch(self.highlight_style)
            } else {
                self.style
            };
            let mut option_x = inner.x;
            for span in &option.spans {
                if option_x >= right {
                    break;
                }
                option_x = buffer
                    .set_stringn(
                        option_x,
                        option_row,
                        span.content.as_ref(),
                        right - option_x,
                        style.patch(span.style),
                    )
                    .0;
            }
        }
    }
}

impl RenderWidget for Select {
    type State = SelectState;

    fn render(self, area: Rect, buffer: &mut Buffer, state: &mut Self::State) {
        Select::render(&self, area, buffer, state);
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    const OPTIONS: [&str; 3] = ["eu-west", "eu-north", "us-east"];

    fn buffer_16x4() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 16, 4))
    }

    mod state {
        use super::*;

        #[test]
        fn new_state_is_closed_and_empty() {
            let state = SelectState::new();
            assert!(!state.is_open());
            assert_eq!(state.highlight(), 0);
            assert_eq!(state.selected(), None);
        }

        #[test]
        fn open_places_the_highlight_on_the_selection() {
            let mut state = SelectState::new().with_selected(Some(2));
            state.open();
            assert!(state.is_open());
            assert_eq!(state.highlight(), 2);
        }

        #[test]
        fn open_without_a_selection_starts_at_the_top() {
            let mut state = SelectState::new();
            state.select_next(3);
            state.close();
            state.open();
            assert_eq!(state.highlight(), 0, "no selection -> highlight resets");
        }

        #[test]
        fn toggle_round_trips() {
            let mut state = SelectState::new();
            state.toggle();
            assert!(state.is_open());
            state.toggle();
            assert!(!state.is_open());
        }

        #[test]
        fn next_clamps_at_the_last_option() {
            let mut state = SelectState::new();
            state.select_next(3);
            state.select_next(3);
            state.select_next(3);
            state.select_next(3);
            assert_eq!(state.highlight(), 2);
            assert_eq!(state.select_next(0), (), "count 0 is a no-op clamp");
            assert_eq!(state.highlight(), 0, "count 0 clamps the highlight to 0");
        }

        #[test]
        fn previous_clamps_at_the_top() {
            let mut state = SelectState::new();
            state.select_previous();
            assert_eq!(state.highlight(), 0);
            state.select_next(3);
            state.select_previous();
            assert_eq!(state.highlight(), 0);
        }

        #[test]
        fn choose_commits_and_closes_only_when_open() {
            let mut state = SelectState::new();
            assert_eq!(state.choose(), None, "a closed dropdown selects nothing");
            assert!(!state.is_open());
            state.open();
            state.select_next(3);
            assert_eq!(state.choose(), Some(1));
            assert_eq!(state.selected(), Some(1));
            assert!(!state.is_open());
        }

        #[test]
        fn close_without_committing_resyncs_on_open() {
            let mut state = SelectState::new().with_selected(Some(2));
            state.open();
            state.select_previous();
            state.select_previous();
            assert_eq!(state.highlight(), 0);
            state.close();
            state.open();
            assert_eq!(state.highlight(), 2, "dismissed navigation is forgotten");
            assert_eq!(state.selected(), Some(2));
        }

        #[test]
        fn set_highlight_moves_verbatim() {
            let mut state = SelectState::new();
            state.set_highlight(7);
            assert_eq!(state.highlight(), 7);
        }

        #[test]
        fn set_selected_replaces_the_choice() {
            let mut state = SelectState::new();
            state.set_selected(Some(1));
            assert_eq!(state.selected(), Some(1));
            state.set_selected(None);
            assert_eq!(state.selected(), None);
        }
    }

    mod paint {
        use super::*;

        #[test]
        fn closed_trigger_shows_label_value_indicator() {
            let mut buffer = buffer_16x4();
            let mut state = SelectState::new().with_selected(Some(1));
            Select::new(OPTIONS).label("region").render(
                Rect::new(0, 0, 16, 4),
                &mut buffer,
                &mut state,
            );
            assert_eq!(
                buffer.to_string(),
                "region: eu-north\n                \n                \n                "
            );
        }

        #[test]
        fn trigger_without_a_label_shows_only_value_and_indicator() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 1));
            let mut state = SelectState::new().with_selected(Some(0));
            Select::new(OPTIONS).render(Rect::new(0, 0, 12, 1), &mut buffer, &mut state);
            assert_eq!(buffer.to_string(), "eu-west ▾   ");
        }

        #[test]
        fn empty_selection_renders_the_placeholder() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 1));
            let mut state = SelectState::new();
            Select::new(OPTIONS)
                .label("region")
                .placeholder("pick one")
                .render(Rect::new(0, 0, 20, 1), &mut buffer, &mut state);
            assert_eq!(buffer.to_string(), "region: pick one ▾  ");
        }

        #[test]
        fn open_indicator_points_up() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 3));
            let mut state = SelectState::new().with_selected(Some(0));
            state.open();
            Select::new(OPTIONS).render(Rect::new(0, 0, 12, 3), &mut buffer, &mut state);
            assert_eq!(
                buffer.to_string(),
                "eu-west ▴   \neu-west     \neu-north    "
            );
        }

        #[test]
        fn open_list_renders_one_option_per_row() {
            let mut buffer = buffer_16x4();
            let mut state = SelectState::new();
            state.open();
            Select::new(OPTIONS).label("region").render(
                Rect::new(0, 0, 16, 4),
                &mut buffer,
                &mut state,
            );
            assert_eq!(
                buffer.to_string(),
                "region:  ▴      \neu-west         \neu-north        \nus-east         "
            );
        }

        #[test]
        fn highlighted_row_takes_the_highlight_style() {
            let mut buffer = buffer_16x4();
            let mut state = SelectState::new();
            state.open();
            state.select_next(3);
            state.select_next(3);
            Select::new(OPTIONS)
                .highlight_style(Style::new().fg(Color::Cyan))
                .render(Rect::new(0, 0, 16, 4), &mut buffer, &mut state);
            // Option i paints on the row below the trigger: highlight 2
            // lights the third option row.
            assert_eq!(buffer.get(0, 1).unwrap().style.fg, None);
            assert_eq!(buffer.get(0, 2).unwrap().style.fg, None);
            assert_eq!(buffer.get(0, 3).unwrap().style.fg, Some(Color::Cyan));
        }

        #[test]
        fn out_of_range_highlight_renders_without_panic() {
            let mut buffer = buffer_16x4();
            let mut state = SelectState::new();
            state.open();
            state.set_highlight(99);
            Select::new(OPTIONS).render(Rect::new(0, 0, 16, 4), &mut buffer, &mut state);
            assert!(buffer.content.iter().all(|cell| cell.style.fg.is_none()));
        }

        #[test]
        fn one_row_area_shows_only_the_trigger() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 16, 1));
            let mut state = SelectState::new();
            state.open();
            Select::new(OPTIONS).label("region").render(
                Rect::new(0, 0, 16, 1),
                &mut buffer,
                &mut state,
            );
            assert_eq!(buffer.to_string(), "region:  ▴      ");
        }

        #[test]
        fn options_clip_at_the_area_height() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 2));
            let mut state = SelectState::new();
            state.open();
            Select::new(OPTIONS).render(Rect::new(0, 0, 12, 2), &mut buffer, &mut state);
            assert_eq!(buffer.to_string(), " ▴          \neu-west     ");
        }

        #[test]
        fn long_values_clip_at_the_right_edge() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
            let mut state = SelectState::new().with_selected(Some(0));
            Select::new(["eu-west-1-really-long"])
                .label("region")
                .render(Rect::new(0, 0, 8, 1), &mut buffer, &mut state);
            assert_eq!(buffer.to_string(), "region: ");
        }

        #[test]
        fn no_options_open_renders_only_the_trigger() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 3));
            let mut state = SelectState::new();
            state.open();
            Select::new([""])
                .label("none")
                .render(Rect::new(0, 0, 12, 3), &mut buffer, &mut state);
            assert_eq!(
                buffer.to_string(),
                "none:  ▴    \n            \n            "
            );
        }

        #[test]
        fn zero_options_render_only_the_trigger() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 3));
            let mut state = SelectState::new();
            state.open();
            let select: Select = Select::new(Vec::<Line>::new()).label("none");
            select.render(Rect::new(0, 0, 12, 3), &mut buffer, &mut state);
            assert_eq!(
                buffer.to_string(),
                "none:  ▴    \n            \n            "
            );
        }

        #[test]
        fn block_wraps_trigger_and_list() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 14, 5));
            let mut state = SelectState::new().with_selected(Some(0));
            state.open();
            Select::new(["a", "b"]).block(Block::bordered()).render(
                Rect::new(0, 0, 14, 5),
                &mut buffer,
                &mut state,
            );
            assert_eq!(
                buffer.to_string(),
                "┌────────────┐\n│a ▴         │\n│a           │\n│b           │\n└────────────┘"
            );
        }

        #[test]
        fn theme_maps_text_and_highlight_slots() {
            let theme = Theme::dark();
            let mut buffer = buffer_16x4();
            let mut state = SelectState::new();
            state.open();
            state.select_next(3);
            Select::new(OPTIONS).theme(&theme).render(
                Rect::new(0, 0, 16, 4),
                &mut buffer,
                &mut state,
            );
            assert_eq!(buffer.get(0, 0).unwrap().style.fg, theme.text.fg);
            assert_eq!(buffer.get(0, 1).unwrap().style, theme.text);
            assert_eq!(buffer.get(0, 2).unwrap().style, theme.highlight);
        }

        #[test]
        fn styled_labels_keep_their_own_style() {
            use tuvix_core::text::Span;
            let mut buffer = Buffer::empty(Rect::new(0, 0, 16, 4));
            let mut state = SelectState::new();
            state.open();
            Select::new([Line::from_spans(vec![
                Span::raw("eu-"),
                Span::styled("west", Style::new().fg(Color::Green)),
            ])])
            .render(Rect::new(0, 0, 16, 4), &mut buffer, &mut state);
            assert_eq!(buffer.get(0, 1).unwrap().style.fg, None);
            assert_eq!(buffer.get(3, 1).unwrap().style.fg, Some(Color::Green));
        }

        #[test]
        fn renders_through_the_render_widget_contract() {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 16, 1));
            let mut state = SelectState::new().with_selected(Some(1));
            Select::new(OPTIONS).label("region").render(
                Rect::new(0, 0, 16, 1),
                &mut buffer,
                &mut state,
            );
            assert_eq!(buffer.to_string(), "region: eu-north");
        }

        #[test]
        fn empty_area_is_a_noop() {
            let mut buffer = buffer_16x4();
            let mut state = SelectState::new();
            state.open();
            Select::new(OPTIONS).render(Rect::ZERO, &mut buffer, &mut state);
            assert!(
                buffer
                    .content
                    .iter()
                    .all(tuvix_core::buffer::Cell::is_empty)
            );
        }
    }
}
