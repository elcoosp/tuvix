//! Bar charts: labeled vertical bars in one or many groups.
//!
//! A [`BarChart`] renders the classic dashboard bar chart: columns of
//! full-block glyphs scaled against a maximum, with a value printed
//! above each bar, a label under each bar and a group label under each
//! [`BarGroup`]. Bars that do not fit the area are dropped from the
//! trailing edge, whole groups included.
//!
//! The chart is vertical-only by design: grouped vertical bars cover
//! the dashboard use case, and a horizontal renderer would duplicate
//! the label layout rather than extend it (see the decision log).

use std::borrow::Cow;

use unicode_width::UnicodeWidthStr as _;

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;
use tuvix_core::text::Line;
use tuvix_core::widget::Widget;

use crate::block::Block;

/// One bar of a [`BarGroup`]: a value, an optional under-bar label and
/// the two style layers it can override.
///
/// # Examples
///
/// ```
/// use tuvix_core::style::{Color, Style};
/// use tuvix_core::stylize::Stylize as _;
/// use tuvix_core::text::Line;
/// use tuvix_widgets::Bar;
///
/// let bar = Bar::new(42)
///     .label("rpm")
///     .style(Style::new().fg(Color::Cyan))
///     .value_style(Style::new().bold());
/// assert_eq!(bar.value, 42);
/// assert_eq!(bar.label, Some(Line::raw("rpm")));
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Bar {
    /// The value the bar height is scaled from.
    pub value: u64,
    /// The label written under the bar, centered and clipped to the bar
    /// width.
    pub label: Option<Line>,
    /// The text printed above the bar; defaults to the decimal value.
    pub text_value: Option<Cow<'static, str>>,
    /// Style patched over the group and chart styles for the bar cells.
    pub style: Style,
    /// Style patched over the bar cells' style for the value text.
    pub value_style: Style,
}

impl Bar {
    /// Creates a bar for the given value.
    #[must_use]
    pub fn new(value: u64) -> Self {
        Self::default().value(value)
    }

    /// Sets the bar's value.
    #[must_use]
    pub fn value(mut self, value: u64) -> Self {
        self.value = value;
        self
    }

    /// Sets the under-bar label.
    #[must_use]
    pub fn label(mut self, label: impl Into<Line>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Overrides the text printed above the bar.
    #[must_use]
    pub fn text_value(mut self, text_value: impl Into<Cow<'static, str>>) -> Self {
        self.text_value = Some(text_value.into());
        self
    }

    /// Sets the style patched over the group and chart styles for the
    /// bar cells.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the style patched over the bar cells' style for the value
    /// text.
    #[must_use]
    pub fn value_style(mut self, style: Style) -> Self {
        self.value_style = style;
        self
    }
}

/// A labeled group of [`Bar`]s sharing one style layer and one group
/// label row.
///
/// # Examples
///
/// ```
/// use tuvix_core::style::Style;
/// use tuvix_core::stylize::Stylize as _;
/// use tuvix_widgets::{Bar, BarGroup};
///
/// let group = BarGroup::new([Bar::new(1), Bar::new(2), Bar::new(3)])
///     .label("Q1")
///     .style(Style::new().bold());
/// assert_eq!(group.bars.len(), 3);
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BarGroup {
    /// The label written under the group's bar labels, centered across
    /// the group's columns.
    pub label: Option<Line>,
    /// The bars of the group, rendered left to right.
    pub bars: Vec<Bar>,
    /// Style patched over the chart style for every bar cell of the
    /// group.
    pub style: Style,
}

impl BarGroup {
    /// Creates a group from an iterator of bars.
    pub fn new<T, I>(bars: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Bar>,
    {
        Self {
            label: None,
            bars: bars.into_iter().map(Into::into).collect(),
            style: Style::new(),
        }
    }

    /// Sets the group label.
    #[must_use]
    pub fn label(mut self, label: impl Into<Line>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the style patched over the chart style for the group's bar
    /// cells.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

/// A vertical bar chart of grouped, labeled bars.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::widget::Widget;
/// use tuvix_widgets::{Bar, BarChart};
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 5, 3));
/// BarChart::new([Bar::new(1), Bar::new(2)])
///     .max(2)
///     .render(Rect::new(0, 0, 5, 3), &mut buffer);
/// // Two bars of bar_width 2 separated by a 1-column gap. The second
/// // bar fills the whole height; the first fills one row, and its value
/// // "1" is printed in the free row above it.
/// assert_eq!(buffer.to_string(), "   ██\n1  ██\n██ ██");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarChart {
    groups: Vec<BarGroup>,
    max: Option<u64>,
    bar_width: u16,
    bar_gap: u16,
    group_gap: u16,
    style: Style,
    value_style: Style,
    label_style: Style,
    block: Option<Block>,
}

impl Default for BarChart {
    fn default() -> Self {
        Self {
            groups: Vec::new(),
            max: None,
            bar_width: 2,
            bar_gap: 1,
            group_gap: 1,
            style: Style::new(),
            value_style: Style::new(),
            label_style: Style::new(),
            block: None,
        }
    }
}

impl BarChart {
    /// Creates a chart from a single group of bars.
    pub fn new<T, I>(bars: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Bar>,
    {
        Self::default().group(BarGroup::new(bars))
    }

    /// Appends a group to the chart.
    #[must_use]
    pub fn group(mut self, group: BarGroup) -> Self {
        self.groups.push(group);
        self
    }

    /// Appends several groups to the chart.
    #[must_use]
    pub fn groups<T, I>(mut self, groups: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<BarGroup>,
    {
        self.groups.extend(groups.into_iter().map(Into::into));
        self
    }

    /// Pins the maximum of the scale instead of the data maximum.
    #[must_use]
    pub fn max(mut self, max: u64) -> Self {
        self.max = Some(max);
        self
    }

    /// Sets the width of one bar in columns (default 2).
    #[must_use]
    pub fn bar_width(mut self, bar_width: u16) -> Self {
        self.bar_width = bar_width;
        self
    }

    /// Sets the gap between bars of one group in columns (default 1).
    #[must_use]
    pub fn bar_gap(mut self, bar_gap: u16) -> Self {
        self.bar_gap = bar_gap;
        self
    }

    /// Sets the gap between groups in columns (default 1).
    #[must_use]
    pub fn group_gap(mut self, group_gap: u16) -> Self {
        self.group_gap = group_gap;
        self
    }

    /// Sets the base style of the bar cells.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the base style of the value text above the bars.
    #[must_use]
    pub fn value_style(mut self, style: Style) -> Self {
        self.value_style = style;
        self
    }

    /// Sets the base style of the bar and group labels.
    #[must_use]
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Wraps the chart in a block; bars fill the block's inner area.
    #[must_use]
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Paints the chart into `buffer`, clipped to `area`.
    ///
    /// Bar heights are `(value * rows / max)` with integer flooring,
    /// clamped to the bar area. Labels below the bars reserve one row
    /// when any *rendered* bar carries one, and the group label row is
    /// reserved below the bar labels when any rendered group carries
    /// one — labels of dropped bars and groups reserve nothing. When
    /// the chart has no drawable maximum (no bars, or every value and
    /// the pinned max are zero), nothing is drawn at all.
    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
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

        let placed = self.place(inner);
        if placed.is_empty() {
            return;
        }
        let rows = LabelRows::new(&placed, inner);
        let bar_bottom = rows.bar_bottom(inner);

        // The pinned max wins; otherwise the data max. No drawable
        // maximum means no bars, but labels still paint.
        let data_max = placed
            .iter()
            .flat_map(|group| group.bars.iter())
            .map(|placed| placed.bar.value)
            .max()
            .unwrap_or(0);
        let max = self.max.unwrap_or(data_max);
        if max > 0 {
            self.paint_bars(&placed, inner, bar_bottom, max, buffer);
        }
        rows.paint_labels(&placed, self, bar_bottom, buffer);
    }

    /// The width of one bar in columns (never zero).
    fn bar_columns(&self) -> u16 {
        self.bar_width.max(1)
    }

    /// Pass 1: horizontal layout. Bars are laid left to right; a bar is
    /// kept only when it ends inside the area, and the first bar that
    /// does not fit ends the walk (everything after starts further
    /// right). A group that loses its leading bar is dropped wholesale,
    /// together with its label.
    fn place(&self, inner: Rect) -> Vec<PlacedGroup<'_>> {
        let bar_width = self.bar_columns();
        let mut placed: Vec<PlacedGroup<'_>> = Vec::new();
        let mut cursor = inner.x;
        for group in &self.groups {
            let group_x = cursor;
            let mut bars: Vec<PlacedBar<'_>> = Vec::new();
            for bar in &group.bars {
                let end = cursor.saturating_add(bar_width);
                if end > inner.right() {
                    break;
                }
                bars.push(PlacedBar { bar, x: cursor });
                cursor = end.saturating_add(self.bar_gap);
            }
            if bars.is_empty() {
                break;
            }
            let last_end = bars
                .last()
                .map_or(group_x, |placed| placed.x.saturating_add(bar_width));
            let width = last_end.saturating_sub(group_x);
            placed.push(PlacedGroup {
                group,
                bars,
                x: group_x,
                width,
            });
            cursor = last_end.saturating_add(self.group_gap);
        }
        placed
    }

    /// Passes 3 and 4: bar heights against the scale, then the bar
    /// cells and the value text above each bar top.
    fn paint_bars(
        &self,
        placed: &[PlacedGroup<'_>],
        inner: Rect,
        bar_bottom: u16,
        max: u64,
        buffer: &mut Buffer,
    ) {
        let bar_width = self.bar_columns();
        let rows = u64::from(bar_bottom - inner.y);
        for placed_group in placed {
            for placed in &placed_group.bars {
                let cell_style = self
                    .style
                    .patch(placed_group.group.style)
                    .patch(placed.bar.style);
                let height = placed
                    .bar
                    .value
                    .saturating_mul(rows)
                    .saturating_div(max)
                    .min(rows);
                let height = u16::try_from(height).unwrap_or(u16::MAX);
                for row in 0..height {
                    let y = bar_bottom - 1 - row;
                    for column in 0..bar_width {
                        if let Some(cell) = buffer.get_mut(placed.x + column, y) {
                            cell.set_symbol("█").set_style(cell_style);
                        }
                    }
                }
                if height == 0 {
                    continue;
                }
                self.paint_value(
                    placed, inner, bar_width, bar_bottom, height, cell_style, buffer,
                );
            }
        }
    }

    /// The value text sits in the free row above the bar top, centered
    /// in the bar columns; skipped at the ceiling or when wider than
    /// the bar.
    fn paint_value(
        &self,
        placed: &PlacedBar<'_>,
        inner: Rect,
        bar_width: u16,
        bar_bottom: u16,
        height: u16,
        cell_style: Style,
        buffer: &mut Buffer,
    ) {
        let text = placed
            .bar
            .text_value
            .as_ref()
            .map_or_else(|| Cow::from(placed.bar.value.to_string()), Cow::clone);
        let text_width = u16::try_from(text.width()).unwrap_or(u16::MAX);
        if text_width > bar_width {
            return;
        }
        let bar_top = bar_bottom - height;
        if bar_top == inner.y {
            return;
        }
        let value_style = cell_style
            .patch(self.value_style)
            .patch(placed.bar.value_style);
        let offset = (bar_width - text_width) / 2;
        buffer.set_stringn(
            placed.x + offset,
            bar_top - 1,
            text.as_ref(),
            bar_width,
            value_style,
        );
    }
}

/// One kept bar and its first column.
struct PlacedBar<'a> {
    bar: &'a Bar,
    x: u16,
}

/// One kept group: the group, its kept bars and the column span its
/// label centers across.
struct PlacedGroup<'a> {
    group: &'a BarGroup,
    bars: Vec<PlacedBar<'a>>,
    /// First column of the group's first kept bar.
    x: u16,
    /// Columns from `x` to the end of the last kept bar.
    width: u16,
}

/// The label rows reserved below the bars (pass 2): the group label row
/// sits below the bar label row, and only *rendered* content reserves.
struct LabelRows {
    group_row: Option<u16>,
    bar_row: Option<u16>,
}

impl LabelRows {
    fn new(placed: &[PlacedGroup<'_>], inner: Rect) -> Self {
        let has_bar_labels = placed
            .iter()
            .any(|group| group.bars.iter().any(|placed| placed.bar.label.is_some()));
        let has_group_labels = placed.iter().any(|group| group.group.label.is_some());
        let group_row = has_group_labels.then(|| inner.bottom() - 1);
        // Degenerate areas: the bar label row never leaves the inner
        // area (a 1-row area with both label kinds collapses them onto
        // the same row).
        let bar_row = has_bar_labels
            .then(|| {
                inner
                    .bottom()
                    .saturating_sub(1 + u16::from(has_group_labels))
            })
            .map(|row| row.max(inner.y));
        Self { group_row, bar_row }
    }

    /// Exclusive bottom of the bar area: the first reserved row below
    /// the bars, or the inner bottom when no labels are rendered.
    fn bar_bottom(&self, inner: Rect) -> u16 {
        self.bar_row
            .or(self.group_row)
            .unwrap_or(inner.bottom())
            .max(inner.y)
    }

    /// Pass 5: bar labels centered under their bars, group labels
    /// centered across their group span.
    fn paint_labels(
        &self,
        placed: &[PlacedGroup<'_>],
        chart: &BarChart,
        bar_bottom: u16,
        buffer: &mut Buffer,
    ) {
        let _ = bar_bottom;
        if let Some(row) = self.bar_row {
            let bar_width = chart.bar_columns();
            for group in placed {
                for placed in &group.bars {
                    let Some(label) = &placed.bar.label else {
                        continue;
                    };
                    write_clipped_line(buffer, label, placed.x, bar_width, row, chart.label_style);
                }
            }
        }
        if let Some(row) = self.group_row {
            for group in placed {
                let Some(label) = &group.group.label else {
                    continue;
                };
                write_clipped_line(buffer, label, group.x, group.width, row, chart.label_style);
            }
        }
    }
}

/// Writes a label [`Line`] centered in `span_width` columns at `(x,
/// y)`, clipped to the span when wider. Span styles patch over `base`.
fn write_clipped_line(
    buffer: &mut Buffer,
    label: &Line,
    x: u16,
    span_width: u16,
    y: u16,
    base: Style,
) {
    let width = label.width();
    let offset = if width < span_width {
        (span_width - width) / 2
    } else {
        0
    };
    let mut cursor = x + offset;
    let end = x.saturating_add(span_width);
    let line_style = base.patch(label.style);
    for span in &label.spans {
        if cursor >= end {
            break;
        }
        let remaining = end - cursor;
        let (next, _) = buffer.set_stringn(
            cursor,
            y,
            span.content.as_ref(),
            remaining,
            line_style.patch(span.style),
        );
        cursor = next.max(cursor);
    }
}

impl Widget for BarChart {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        BarChart::render(&self, area, buffer);
    }
}

impl BarChart {
    /// Maps the theme onto the chart: the `sparkline` slot styles the
    /// bars, the `axis` slot the bar and group labels.
    #[must_use]
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.style = theme.sparkline;
        self.label_style = theme.axis;
        self
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::buffer::Cell;
    use tuvix_core::style::{Color, Modifier};

    fn buf(w: u16, h: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, w, h))
    }

    fn assert_rows(buffer: &Buffer, expected: &[&str]) {
        assert_eq!(buffer.to_string(), expected.join("\n"));
    }

    mod bars {
        use super::*;

        #[test]
        fn single_bar_full_height() {
            let mut b = buf(2, 3);
            BarChart::new([Bar::new(10)]).render(Rect::new(0, 0, 2, 3), &mut b);
            assert_rows(&b, &["██", "██", "██"]);
        }

        #[test]
        fn bar_height_is_floor_scaled() {
            let mut b = buf(2, 3);
            BarChart::new([Bar::new(1)])
                .max(4)
                .render(Rect::new(0, 0, 2, 3), &mut b);
            // 1 * 3 / 4 = 0.75 -> 0 rows? No: flooring gives 0; pinned
            // contract says the bar occupies floor(3 * 1 / 4) = 0 rows.
            assert_rows(&b, &["  ", "  ", "  "]);
        }

        #[test]
        fn quarter_bar_rounds_down_to_a_row_at_height_four() {
            let mut b = buf(2, 4);
            BarChart::new([Bar::new(1)])
                .max(4)
                .render(Rect::new(0, 0, 2, 4), &mut b);
            // 1 * 4 / 4 = 1 bar row; the value "1" sits above its top.
            assert_rows(&b, &["  ", "  ", "1 ", "██"]);
        }

        #[test]
        fn custom_max_pins_the_scale() {
            let mut b = buf(2, 2);
            BarChart::new([Bar::new(1)])
                .max(8)
                .render(Rect::new(0, 0, 2, 2), &mut b);
            assert_rows(&b, &["  ", "  "]);
        }

        #[test]
        fn values_above_max_are_clamped() {
            let mut b = buf(2, 2);
            BarChart::new([Bar::new(9)])
                .max(4)
                .render(Rect::new(0, 0, 2, 2), &mut b);
            assert_rows(&b, &["██", "██"]);
        }

        #[test]
        fn zero_value_renders_nothing() {
            let mut b = buf(2, 2);
            BarChart::new([Bar::new(0)])
                .max(4)
                .render(Rect::new(0, 0, 2, 2), &mut b);
            assert!(b.content.iter().all(Cell::is_empty));
        }

        #[test]
        fn all_zero_or_no_bars_renders_nothing() {
            let mut b = buf(5, 2);
            BarChart::new([Bar::new(0), Bar::new(0)]).render(Rect::new(0, 0, 5, 2), &mut b);
            assert!(b.content.iter().all(Cell::is_empty));
            let mut b = buf(5, 2);
            BarChart::new(Vec::<Bar>::new()).render(Rect::new(0, 0, 5, 2), &mut b);
            assert!(b.content.iter().all(Cell::is_empty));
        }

        #[test]
        fn bars_share_one_column_pair_with_gap() {
            let mut b = buf(8, 2);
            BarChart::new([Bar::new(4), Bar::new(4), Bar::new(4)])
                .max(4)
                .render(Rect::new(0, 0, 8, 2), &mut b);
            // bar_width 2, bar_gap 1: columns 0-1, 3-4, 6-7; gap 2 and 5.
            assert_rows(&b, &["██ ██ ██", "██ ██ ██"]);
        }

        #[test]
        fn bar_width_and_gap_are_configurable() {
            let mut b = buf(7, 1);
            BarChart::new([Bar::new(1), Bar::new(1)])
                .max(1)
                .bar_width(3)
                .bar_gap(1)
                .render(Rect::new(0, 0, 7, 1), &mut b);
            assert_rows(&b, &["███ ███"]);
        }
    }

    mod values {
        use super::*;

        #[test]
        fn value_printed_above_the_bar_top() {
            let mut b = buf(2, 3);
            BarChart::new([Bar::new(2)])
                .max(4)
                .render(Rect::new(0, 0, 2, 3), &mut b);
            // 2 * 3 / 4 = 1 row of bar; value "2" centered in the row above.
            assert_rows(&b, &["  ", "2 ", "██"]);
        }

        #[test]
        fn value_is_centered_in_the_bar_columns() {
            let mut b = buf(4, 3);
            BarChart::new([Bar::new(2)])
                .max(4)
                .bar_width(4)
                .render(Rect::new(0, 0, 4, 3), &mut b);
            // 2 * 3 / 4 = 1 bar row; "2" centered in the 4 columns.
            assert_rows(&b, &["    ", " 2  ", "████"]);
        }

        #[test]
        fn value_skipped_when_bar_reaches_the_top() {
            let mut b = buf(2, 2);
            BarChart::new([Bar::new(4)])
                .max(4)
                .render(Rect::new(0, 0, 2, 2), &mut b);
            assert_rows(&b, &["██", "██"]);
        }

        #[test]
        fn value_skipped_when_wider_than_the_bar() {
            let mut b = buf(2, 3);
            BarChart::new([Bar::new(399)])
                .max(400)
                .render(Rect::new(0, 0, 2, 3), &mut b);
            // 399 * 3 / 400 = 2 rows; "399" needs 3 columns > bar_width 2.
            assert_rows(&b, &["  ", "██", "██"]);
        }

        #[test]
        fn text_value_overrides_the_number() {
            let mut b = buf(3, 2);
            BarChart::new([Bar::new(50).text_value("50%")])
                .max(100)
                .bar_width(3)
                .render(Rect::new(0, 0, 3, 2), &mut b);
            // 50 * 2 / 100 = 1 bar row; "50%" above.
            assert_rows(&b, &["50%", "███"]);
        }

        #[test]
        fn zero_height_bar_prints_no_value() {
            let mut b = buf(2, 2);
            BarChart::new([Bar::new(1)])
                .max(100)
                .render(Rect::new(0, 0, 2, 2), &mut b);
            assert!(b.content.iter().all(Cell::is_empty));
        }
    }

    mod labels {
        use super::*;

        #[test]
        fn bar_labels_sit_in_the_bottom_row_centered() {
            let mut b = buf(5, 3);
            BarChart::new([Bar::new(4).label("ab"), Bar::new(4).label("c")])
                .max(4)
                .render(Rect::new(0, 0, 5, 3), &mut b);
            // Bars fill rows 0-1 (one label row reserved), labels "ab"
            // and "c" each centered in their 2-column bar.
            assert_rows(&b, &["██ ██", "██ ██", "ab c "]);
        }

        #[test]
        fn wide_labels_are_clipped_to_the_bar_width() {
            let mut b = buf(2, 2);
            BarChart::new([Bar::new(4).label("long")])
                .max(4)
                .render(Rect::new(0, 0, 2, 2), &mut b);
            assert_rows(&b, &["██", "lo"]);
        }

        #[test]
        fn no_labels_reserve_no_rows() {
            let mut b = buf(2, 2);
            BarChart::new([Bar::new(4)])
                .max(4)
                .render(Rect::new(0, 0, 2, 2), &mut b);
            assert_rows(&b, &["██", "██"]);
        }

        #[test]
        fn one_row_area_shows_only_labels() {
            let mut b = buf(2, 1);
            BarChart::new([Bar::new(4).label("x")])
                .max(4)
                .render(Rect::new(0, 0, 2, 1), &mut b);
            assert_rows(&b, &["x "]);
        }

        #[test]
        fn group_label_sits_below_the_bar_labels() {
            let mut b = buf(8, 4);
            BarChart::default()
                .group(BarGroup::new([
                    Bar::new(4).label("a"),
                    Bar::new(4).label("b"),
                ]))
                .group(BarGroup::new([Bar::new(4).label("c")]).label("G"))
                .max(4)
                .group_gap(1)
                .render(Rect::new(0, 0, 8, 4), &mut b);
            // Bars at columns 0-1, 3-4, 6-7; two label rows reserved.
            assert_rows(&b, &["██ ██ ██", "██ ██ ██", "a  b  c ", "      G "]);
        }

        #[test]
        fn group_label_centered_across_the_group_span() {
            let mut b = buf(5, 3);
            let chart = BarChart::default()
                .group(BarGroup::new([Bar::new(4).label("a"), Bar::new(4).label("b")]).label("grp"))
                .max(4);
            chart.render(Rect::new(0, 0, 5, 3), &mut b);
            // Group spans columns 0..5; "grp" centered there. One bar row.
            assert_rows(&b, &["██ ██", "a  b ", " grp "]);
        }
    }

    mod groups {
        use super::*;

        #[test]
        fn groups_are_separated_by_the_group_gap() {
            let mut b = buf(7, 1);
            BarChart::default()
                .group(BarGroup::new([Bar::new(1)]))
                .group(BarGroup::new([Bar::new(1)]))
                .max(1)
                .bar_width(2)
                .group_gap(2)
                .render(Rect::new(0, 0, 7, 1), &mut b);
            // Group 1 at columns 0-1, a 2-column gap, group 2 at 4-5.
            assert_rows(&b, &["██  ██ "]);
        }

        #[test]
        fn trailing_group_dropped_when_it_does_not_fit() {
            let mut b = buf(4, 1);
            BarChart::default()
                .group(BarGroup::new([Bar::new(1)]))
                .group(BarGroup::new([Bar::new(1)]))
                .max(1)
                .bar_width(2)
                .group_gap(1)
                .render(Rect::new(0, 0, 4, 1), &mut b);
            // Group 1 at columns 0-1; group 2 would start at 3 and end
            // at 5, past the 4-column area.
            assert_rows(&b, &["██  "]);
        }

        #[test]
        fn trailing_bars_of_a_group_are_dropped() {
            let mut b = buf(5, 1);
            BarChart::new([Bar::new(1), Bar::new(1), Bar::new(1)])
                .max(1)
                .bar_width(2)
                .bar_gap(1)
                .render(Rect::new(0, 0, 5, 1), &mut b);
            // Bars at 0-1, 3-4; the third would start at 6 -> dropped.
            assert_rows(&b, &["██ ██"]);
        }

        #[test]
        fn dropped_group_label_does_not_reserve_a_row() {
            let mut b = buf(4, 2);
            BarChart::default()
                .group(BarGroup::new([Bar::new(1)]))
                .group(BarGroup::new([Bar::new(1)]).label("far"))
                .max(1)
                .bar_width(2)
                .group_gap(1)
                .render(Rect::new(0, 0, 4, 2), &mut b);
            // The "far" group is dropped wholesale, so its label does
            // not reserve a label row: bars fill the full height.
            assert_rows(&b, &["██  ", "██  "]);
        }

        #[test]
        fn dropped_bar_label_does_not_reserve_a_row() {
            let mut b = buf(5, 2);
            BarChart::new([Bar::new(1), Bar::new(1), Bar::new(1).label("z")])
                .max(1)
                .bar_width(2)
                .bar_gap(1)
                .render(Rect::new(0, 0, 5, 2), &mut b);
            // Only the dropped third bar carries a label, so no label
            // row is reserved and the bars fill both rows.
            assert_rows(&b, &["██ ██", "██ ██"]);
        }
    }

    mod styles {
        use super::*;

        #[test]
        fn chart_group_and_bar_styles_layer() {
            let mut b = buf(2, 1);
            BarChart::new([Bar::new(1).style(Style::new().fg(Color::Red))])
                .max(1)
                .style(Style::new().bg(Color::Blue))
                .render(Rect::new(0, 0, 2, 1), &mut b);
            let cell = b.get(0, 0).unwrap();
            assert_eq!(cell.style.fg, Some(Color::Red));
            assert_eq!(cell.style.bg, Some(Color::Blue));
        }

        #[test]
        fn group_style_applies_to_its_bars() {
            let mut b = buf(2, 1);
            BarChart::default()
                .group(BarGroup::new([Bar::new(1)]).style(Style::new().fg(Color::Green)))
                .max(1)
                .render(Rect::new(0, 0, 2, 1), &mut b);
            assert_eq!(b.get(0, 0).unwrap().style.fg, Some(Color::Green));
        }

        #[test]
        fn value_style_patches_the_bar_style() {
            let mut b = buf(2, 2);
            BarChart::new([Bar::new(2).value_style(Style::new().add_modifier(Modifier::BOLD))])
                .max(4)
                .render(Rect::new(0, 0, 2, 2), &mut b);
            // Value "2" in the row above the single bar row.
            assert_eq!(b.get(0, 0).unwrap().style.add_modifier, Modifier::BOLD);
        }

        #[test]
        fn label_style_applies_to_labels() {
            let mut b = buf(2, 2);
            BarChart::new([Bar::new(4).label("x")])
                .max(4)
                .label_style(Style::new().fg(Color::Yellow))
                .render(Rect::new(0, 0, 2, 2), &mut b);
            assert_eq!(b.get(0, 1).unwrap().style.fg, Some(Color::Yellow));
        }

        #[test]
        fn theme_maps_sparkline_to_bars_and_axis_to_labels() {
            let theme = Theme::dark();
            let chart = BarChart::new([Bar::new(1)]).theme(&theme);
            assert_eq!(chart.style, theme.sparkline);
            assert_eq!(chart.label_style, theme.axis);
        }
    }

    mod surface {
        use super::*;

        #[test]
        fn block_wraps_and_bars_fill_inner() {
            let mut b = buf(6, 3);
            BarChart::new([Bar::new(4)])
                .max(4)
                .block(Block::bordered())
                .render(Rect::new(0, 0, 6, 3), &mut b);
            assert_rows(&b, &["┌────┐", "│██  │", "└────┘"]);
        }

        #[test]
        fn empty_area_is_a_noop() {
            let mut b = buf(3, 2);
            BarChart::new([Bar::new(1)]).render(Rect::ZERO, &mut b);
            assert!(b.content.iter().all(Cell::is_empty));
        }

        #[test]
        fn chart_renders_through_the_widget_trait() {
            let mut b = buf(2, 1);
            Widget::render(
                BarChart::new([Bar::new(1)]).max(1),
                Rect::new(0, 0, 2, 1),
                &mut b,
            );
            assert_rows(&b, &["██"]);
        }

        #[test]
        fn doctest_example_pin() {
            let mut b = buf(5, 3);
            BarChart::new([Bar::new(1), Bar::new(2)])
                .max(2)
                .render(Rect::new(0, 0, 5, 3), &mut b);
            assert_rows(&b, &["   ██", "1  ██", "██ ██"]);
        }
    }
}
