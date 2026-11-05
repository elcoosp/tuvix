//! Charts: datasets plotted against labeled axes.
//!
//! The plot maps data points into a graph area derived from the chart
//! area: a right-aligned label gutter plus one gap column for the y
//! axis line on the left, and one axis row for the x line + labels on
//! the bottom (each reserved only when that axis has labels). Points
//! outside the axis bounds are skipped; `NaN` points are skipped.

use std::borrow::Cow;

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;
use tuvix_core::text::Line;
use tuvix_core::widget::Widget;

use crate::block::Block;
use crate::marker::Marker;

/// How a [`Dataset`] connects its points.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraphType {
    /// Plot each point as an isolated marker.
    #[default]
    Scatter,
    /// Connect consecutive points (in data order) with cell lines.
    Line,
}

/// One named series of `(x, y)` points.
///
/// # Examples
///
/// ```
/// use tuvix_core::style::{Color, Style};
/// use tuvix_core::widget::Widget;
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_widgets::{Axis, Chart, Dataset, GraphType};
///
/// let dataset = Dataset::new("sin", [(0.0, 0.0), (1.0, 1.0)])
///     .graph_type(GraphType::Scatter)
///     .style(Style::new().fg(Color::Red));
/// let chart = Chart::new([dataset])
///     .x_axis(Axis::new().bounds([0.0, 1.0]))
///     .y_axis(Axis::new().bounds([0.0, 1.0]));
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 6));
/// chart.render(Rect::new(0, 0, 12, 6), &mut buffer);
/// // (0, 0) is the bottom-left of the graph, (1, 1) the top-right.
/// assert_eq!(buffer.get(0, 5).unwrap().symbol.as_str(), "•");
/// assert_eq!(buffer.get(11, 0).unwrap().symbol.as_str(), "•");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Dataset {
    /// The series name (kept for future legends and app-side labels).
    pub name: Cow<'static, str>,
    /// The `(x, y)` points, in draw order.
    pub data: Vec<(f64, f64)>,
    /// The glyph painted per point (Braille degrades to a dot here).
    pub marker: Marker,
    /// How consecutive points connect.
    pub graph_type: GraphType,
    /// The style of the painted cells.
    pub style: Style,
}

impl Dataset {
    /// Creates a scatter dataset with the [`Marker::Dot`] glyph.
    pub fn new<T: Into<Cow<'static, str>>, P, I>(name: T, data: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<(f64, f64)>,
    {
        Self {
            name: name.into(),
            data: data.into_iter().map(Into::into).collect(),
            marker: Marker::default(),
            graph_type: GraphType::default(),
            style: Style::new(),
        }
    }

    /// Sets the point glyph.
    pub fn marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
    }

    /// Sets how consecutive points connect.
    pub fn graph_type(mut self, graph_type: GraphType) -> Self {
        self.graph_type = graph_type;
        self
    }

    /// Sets the style of the painted cells.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

/// An axis: bounds plus optional title and tick labels.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Axis {
    /// The axis title (rendered at the axis origin end).
    pub title: Option<Line>,
    /// The `[min, max]` data bounds mapped onto the graph area.
    pub bounds: [f64; 2],
    /// The tick labels, spread evenly along the axis.
    pub labels: Vec<Line>,
}

impl Axis {
    /// Creates an axis with `bounds` `[0.0, 0.0]` and no labels.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the data bounds.
    pub fn bounds(mut self, bounds: [f64; 2]) -> Self {
        self.bounds = bounds;
        self
    }

    /// Sets the tick labels, spread evenly along the axis.
    pub fn labels<T, I>(mut self, labels: I) -> Self
    where
        T: Into<Line>,
        I: IntoIterator<Item = T>,
    {
        self.labels = labels.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the axis title.
    pub fn title<T: Into<Line>>(mut self, title: T) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// A plot of datasets against labeled x/y axes.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::widget::Widget;
/// use tuvix_widgets::{Axis, Chart, Dataset};
///
/// let chart = Chart::new([Dataset::new("run", [(0.0, 0.0), (4.0, 4.0)])])
///     .x_axis(Axis::new().bounds([0.0, 4.0]))
///     .y_axis(Axis::new().bounds([0.0, 4.0]));
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 5));
/// chart.render(Rect::new(0, 0, 10, 5), &mut buffer);
/// // (0, 0) maps to the bottom-left, (4, 4) to the top-right.
/// assert_eq!(buffer.get(0, 4).unwrap().symbol.as_str(), "•");
/// assert_eq!(buffer.get(9, 0).unwrap().symbol.as_str(), "•");
/// ```
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Chart {
    datasets: Vec<Dataset>,
    x_axis: Axis,
    y_axis: Axis,
    block: Option<Block>,
    style: Style,
}

impl Chart {
    /// Creates a chart from the datasets, with default (unlabeled) axes.
    pub fn new<D, I>(datasets: I) -> Self
    where
        D: Into<Dataset>,
        I: IntoIterator<Item = D>,
    {
        Self {
            datasets: datasets.into_iter().map(Into::into).collect(),
            x_axis: Axis::default(),
            y_axis: Axis::default(),
            block: None,
            style: Style::new(),
        }
    }

    /// Sets the x axis.
    pub fn x_axis(mut self, axis: Axis) -> Self {
        self.x_axis = axis;
        self
    }

    /// Sets the y axis.
    pub fn y_axis(mut self, axis: Axis) -> Self {
        self.y_axis = axis;
        self
    }

    /// Wraps the chart in a block; the plot fills the block's inner area.
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base style of the chart area.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Paints the chart into `buffer`, clipped to `area`.
    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        if let Some(block) = &self.block {
            block.render(area, buffer);
        }
        let chart_area = self.block.as_ref().map_or(area, |block| block.inner(area));
        if chart_area.is_empty() {
            return;
        }
        buffer.set_style(chart_area, self.style);

        let Some((graph, y_label_width, x_axis_labeled)) = self.graph_area(chart_area) else {
            return;
        };

        self.render_axis_lines(buffer, graph, x_axis_labeled);
        self.render_y_labels(buffer, graph, chart_area, y_label_width);
        self.render_x_labels(buffer, graph);

        for dataset in &self.datasets {
            self.render_dataset(buffer, graph, dataset);
        }

        self.render_axis_titles(buffer, graph, chart_area, y_label_width, x_axis_labeled);
    }

    /// The graph area with its y-label gutter width; `None` when too
    /// small to plot.
    fn graph_area(&self, chart_area: Rect) -> Option<(Rect, u16, bool)> {
        // Y gutter: widest label + one gap column for the axis line.
        let y_label_width = self
            .y_axis
            .labels
            .iter()
            .map(Line::width)
            .max()
            .unwrap_or(0);
        let y_axis_labeled = !self.y_axis.labels.is_empty();
        let graph_x = if y_axis_labeled {
            chart_area.x + y_label_width + 1
        } else {
            chart_area.x
        };

        // X axis row: one row reserved for the line + labels.
        let x_axis_labeled = !self.x_axis.labels.is_empty();
        let graph_width = chart_area.right().saturating_sub(graph_x);
        let graph_height = usize::from(chart_area.bottom().saturating_sub(chart_area.y))
            .saturating_sub(usize::from(x_axis_labeled));
        if graph_width == 0 || graph_height == 0 {
            return None;
        }
        Some((
            Rect::new(graph_x, chart_area.y, graph_width, graph_height as u16),
            y_label_width,
            x_axis_labeled,
        ))
    }

    /// Draws the axis lines: the y line in the gutter gap column, the x
    /// line across the axis row.
    fn render_axis_lines(&self, buffer: &mut Buffer, graph: Rect, x_axis_labeled: bool) {
        if !self.y_axis.labels.is_empty() {
            for y in graph.y..graph.bottom() {
                if let Some(cell) = buffer.get_mut(graph.x - 1, y) {
                    cell.set_symbol("│").set_style(self.style);
                }
            }
        }
        if x_axis_labeled {
            let axis_row = graph.bottom();
            for x in graph.x..graph.right() {
                if let Some(cell) = buffer.get_mut(x, axis_row) {
                    cell.set_symbol("─").set_style(self.style);
                }
            }
        }
    }

    /// Plots one dataset (scatter points or connected lines).
    fn render_dataset(&self, buffer: &mut Buffer, graph: Rect, dataset: &Dataset) {
        let screen: Vec<(i64, i64)> = dataset
            .data
            .iter()
            .filter_map(|(x, y)| self.map_point(graph, *x, *y))
            .collect();
        let symbol = dataset.marker.cell_symbol();
        match dataset.graph_type {
            GraphType::Scatter => {
                for (px, py) in screen {
                    plot(buffer, graph, px, py, symbol, dataset.style);
                }
            }
            GraphType::Line => {
                for pair in screen.windows(2) {
                    for (px, py) in bresenham(pair[0], pair[1]) {
                        plot(buffer, graph, px, py, symbol, dataset.style);
                    }
                }
            }
        }
    }

    /// Draws the axis titles: y above the gutter, x on the axis row.
    fn render_axis_titles(
        &self,
        buffer: &mut Buffer,
        graph: Rect,
        chart_area: Rect,
        y_label_width: u16,
        x_axis_labeled: bool,
    ) {
        let Some(title) = &self.y_axis.title else {
            return;
        };
        let style = self.style.patch(title.style);
        let width = y_label_width.min(chart_area.right() - chart_area.x);
        let mut x = chart_area.x;
        for span in &title.spans {
            if x >= chart_area.right() || width == 0 {
                break;
            }
            let (end_x, _) =
                buffer.set_stringn(x, chart_area.y, span.content.as_ref(), width, style);
            x = end_x;
        }
        if let (Some(title), true) = (&self.x_axis.title, x_axis_labeled) {
            let style = self.style.patch(title.style);
            let row = graph.bottom();
            let title_width = title.width();
            let mut x = chart_area
                .right()
                .saturating_sub(title_width.min(graph.width));
            for span in &title.spans {
                if x >= chart_area.right() {
                    break;
                }
                let (end_x, _) = buffer.set_stringn(
                    x,
                    row,
                    span.content.as_ref(),
                    chart_area.right() - x,
                    style,
                );
                x = end_x;
            }
        }
    }

    /// Spreads the y labels top-to-bottom over the graph rows,
    /// right-aligned in the gutter.
    fn render_y_labels(
        &self,
        buffer: &mut Buffer,
        graph: Rect,
        chart_area: Rect,
        gutter_width: u16,
    ) {
        let count = self.y_axis.labels.len();
        if count == 0 {
            return;
        }
        for (index, label) in self.y_axis.labels.iter().enumerate() {
            let row = if count == 1 {
                graph.y
            } else {
                graph.y
                    + (index as u32 * u32::from(graph.height.saturating_sub(1))
                        / (count as u32 - 1)) as u16
            };
            let label_width = label.width().min(gutter_width);
            let mut x = graph.x.saturating_sub(1 + label_width);
            if x < chart_area.x {
                continue;
            }
            for span in &label.spans {
                if x >= graph.x.saturating_sub(1) {
                    break;
                }
                let style = self.style.patch(span.style);
                let (end_x, _) = buffer.set_stringn(
                    x,
                    row,
                    span.content.as_ref(),
                    graph.x.saturating_sub(1) - x,
                    style,
                );
                x = end_x;
            }
        }
    }

    /// Spreads the x labels along the axis row; the first label is
    /// left-aligned at the graph start, the last ends at the graph edge,
    /// the middle ones center on their anchor column.
    fn render_x_labels(&self, buffer: &mut Buffer, graph: Rect) {
        let count = self.x_axis.labels.len();
        if count == 0 {
            return;
        }
        let axis_row = graph.bottom();
        for (index, label) in self.x_axis.labels.iter().enumerate() {
            let anchor = if count == 1 {
                u32::from(graph.x)
            } else {
                u32::from(graph.x)
                    + (index as u32 * u32::from(graph.width.saturating_sub(1)) / (count as u32 - 1))
            };
            let label_width = label.width();
            let x = if index == 0 {
                graph.x
            } else if index == count - 1 {
                graph.right().saturating_sub(label_width)
            } else {
                anchor as u16 + graph.x / 2 - label_width / 2
            };
            let mut cursor = x;
            for span in &label.spans {
                if cursor >= graph.right() {
                    break;
                }
                let style = self.style.patch(span.style);
                let (end_x, _) = buffer.set_stringn(
                    cursor,
                    axis_row,
                    span.content.as_ref(),
                    graph.right() - cursor,
                    style,
                );
                cursor = end_x;
            }
        }
    }

    /// Maps a data point to graph cell coordinates; `None` when the point
    /// is outside the bounds or the bounds are degenerate.
    fn map_point(&self, graph: Rect, x: f64, y: f64) -> Option<(i64, i64)> {
        let [x_min, x_max] = self.x_axis.bounds;
        let [y_min, y_max] = self.y_axis.bounds;
        if x.is_nan() || y.is_nan() || x < x_min || x > x_max || y < y_min || y > y_max {
            return None;
        }
        let x_span = x_max - x_min;
        let y_span = y_max - y_min;
        if x_span <= 0.0 || y_span <= 0.0 {
            return None;
        }
        let px = ((x - x_min) / x_span * f64::from(graph.width - 1)).round();
        let py = ((y_max - y) / y_span * f64::from(graph.height - 1)).round();
        let px = px.clamp(0.0, f64::from(graph.width - 1)) as i64;
        let py = py.clamp(0.0, f64::from(graph.height - 1)) as i64;
        Some((i64::from(graph.x) + px, i64::from(graph.y) + py))
    }
}

/// Writes one plot cell (clipped to the graph).
fn plot(buffer: &mut Buffer, graph: Rect, px: i64, py: i64, symbol: &str, style: Style) {
    let Ok(x) = u16::try_from(px) else {
        return;
    };
    let Ok(y) = u16::try_from(py) else {
        return;
    };
    if x < graph.x || y < graph.y || x >= graph.right() || y >= graph.bottom() {
        return;
    }
    if let Some(cell) = buffer.get_mut(x, y) {
        cell.set_symbol(symbol).set_style(style);
    }
}

/// Integer Bresenham walk between two cell coordinates.
pub(crate) fn bresenham(from: (i64, i64), to: (i64, i64)) -> Vec<(i64, i64)> {
    let (mut x, mut y) = from;
    let (x0, y0, x1, y1) = (from.0, from.1, to.0, to.1);
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let step_x = if x0 < x1 { 1 } else { -1 };
    let step_y = if y0 < y1 { 1 } else { -1 };
    let mut error = dx + dy;
    let mut points = Vec::new();
    loop {
        points.push((x, y));
        if x == x1 && y == y1 {
            break;
        }
        let doubled = 2 * error;
        if doubled >= dy {
            error += dy;
            x += step_x;
        }
        if doubled <= dx {
            error += dx;
            y += step_y;
        }
    }
    points
}

impl Widget for Chart {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Chart::render(&self, area, buffer);
    }
}

impl Chart {
    /// Maps the theme onto the chart: the `axis` slot styles the axis
    /// lines and labels.
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.style = theme.axis;
        self
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    #[test]
    fn scatter_maps_points_into_the_graph() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 5));
        Chart::new([Dataset::new("d", [(0.0, 0.0), (1.0, 1.0)])])
            .x_axis(Axis::new().bounds([0.0, 1.0]))
            .y_axis(Axis::new().bounds([0.0, 1.0]))
            .render(Rect::new(0, 0, 10, 5), &mut buffer);
        // Math orientation: y_min at the bottom, y_max at the top.
        assert_eq!(buffer.get(0, 4).unwrap().symbol.as_str(), "•");
        assert_eq!(buffer.get(9, 0).unwrap().symbol.as_str(), "•");
    }

    #[test]
    fn points_outside_bounds_are_skipped() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 5));
        Chart::new([Dataset::new("d", [(2.0, 0.0), (0.5, 99.0), (0.5, 0.5)])])
            .x_axis(Axis::new().bounds([0.0, 1.0]))
            .y_axis(Axis::new().bounds([0.0, 1.0]))
            .render(Rect::new(0, 0, 10, 5), &mut buffer);
        let text = buffer.to_string();
        assert_eq!(text.matches('•').count(), 1);
    }

    #[test]
    fn nan_points_are_skipped() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 5));
        Chart::new([Dataset::new(
            "d",
            [(f64::NAN, 0.0), (0.5, f64::NAN), (0.5, 0.5)],
        )])
        .x_axis(Axis::new().bounds([0.0, 1.0]))
        .y_axis(Axis::new().bounds([0.0, 1.0]))
        .render(Rect::new(0, 0, 10, 5), &mut buffer);
        assert_eq!(buffer.to_string().matches('•').count(), 1);
    }

    #[test]
    fn degenerate_bounds_plot_nothing() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 5));
        Chart::new([Dataset::new("d", [(0.0, 0.0)])])
            .x_axis(Axis::new().bounds([1.0, 1.0]))
            .y_axis(Axis::new().bounds([0.0, 1.0]))
            .render(Rect::new(0, 0, 10, 5), &mut buffer);
        assert_eq!(buffer.to_string().matches('\u{2022}').count(), 0);
    }

    #[test]
    fn line_connects_points_with_bresenham() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 4));
        Chart::new([Dataset::new("d", [(0.0, 0.0), (1.0, 1.0)]).graph_type(GraphType::Line)])
            .x_axis(Axis::new().bounds([0.0, 1.0]))
            .y_axis(Axis::new().bounds([0.0, 1.0]))
            .render(Rect::new(0, 0, 6, 4), &mut buffer);
        // Rising diagonal: (0,0) is the bottom-left corner.
        assert_eq!(buffer.to_string(), "     •\n   •• \n ••   \n•     ");
    }

    #[test]
    fn y_labels_reserve_a_gutter_and_draw_the_axis_line() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 5));
        Chart::new([Dataset::new("d", [(0.5, 0.5)])])
            .x_axis(Axis::new().bounds([0.0, 1.0]))
            .y_axis(Axis::new().bounds([0.0, 1.0]).labels(["0", "50", "100"]))
            .render(Rect::new(0, 0, 12, 5), &mut buffer);
        let rendered = buffer.to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        // Gutter of 3 (widest label "100") + gap column with the axis
        // line; labels are right-aligned, top-to-bottom.
        assert_eq!(rows[0], "  0│        ");
        assert_eq!(rows[2], " 50│    •   ");
        assert_eq!(rows[4], "100│        ");
    }

    #[test]
    fn x_labels_reserve_the_bottom_axis_row() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 5));
        Chart::new([Dataset::new("d", [(0.0, 1.0)])])
            .x_axis(Axis::new().bounds([0.0, 1.0]).labels(["a", "b"]))
            .y_axis(Axis::new().bounds([0.0, 1.0]))
            .render(Rect::new(0, 0, 12, 5), &mut buffer);
        let rendered = buffer.to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        // Graph rows 0-3 (y=1 pins the point to the top row); row 4 is
        // the axis line with labels at the ends.
        assert_eq!(rows[0], "•           ");
        assert_eq!(rows[4], "a──────────b");
    }

    #[test]
    fn dataset_style_is_applied_to_cells() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 5));
        Chart::new([Dataset::new("d", [(0.5, 0.5)]).style(Style::new().fg(Color::Red))])
            .x_axis(Axis::new().bounds([0.0, 1.0]))
            .y_axis(Axis::new().bounds([0.0, 1.0]))
            .render(Rect::new(0, 0, 10, 5), &mut buffer);
        // 0.5 * 9 = 4.5 rounds to 5; (1 - 0.5) * 4 = 2.
        assert_eq!(buffer.get(5, 2).unwrap().style.fg, Some(Color::Red));
    }

    #[test]
    fn block_marker_uses_the_shaded_glyph() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 5));
        Chart::new([Dataset::new("d", [(0.5, 0.5)]).marker(Marker::Block)])
            .x_axis(Axis::new().bounds([0.0, 1.0]))
            .y_axis(Axis::new().bounds([0.0, 1.0]))
            .render(Rect::new(0, 0, 10, 5), &mut buffer);
        assert_eq!(buffer.get(5, 2).unwrap().symbol.as_str(), "▒");
    }

    #[test]
    fn block_wraps_and_the_plot_fills_inner() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 5));
        Chart::new([Dataset::new("d", [(0.5, 1.0)])])
            .x_axis(Axis::new().bounds([0.0, 1.0]))
            .y_axis(Axis::new().bounds([0.0, 1.0]))
            .block(Block::bordered())
            .render(Rect::new(0, 0, 8, 5), &mut buffer);
        let rendered = buffer.to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        assert_eq!(rows[0], "┌──────┐");
        // y = 1 (top of bounds) pins the point to the first graph row.
        assert_eq!(rows[1], "│   •  │");
    }

    #[test]
    fn bresenham_walks_a_straight_diagonal() {
        assert_eq!(
            bresenham((0, 0), (3, 3)),
            vec![(0, 0), (1, 1), (2, 2), (3, 3)]
        );
        assert_eq!(bresenham((0, 0), (0, 0)), vec![(0, 0)]);
        let walk = bresenham((0, 0), (4, 2));
        assert_eq!(walk.first().copied(), Some((0, 0)));
        assert_eq!(*walk.last().unwrap(), (4, 2));
        assert_eq!(walk.len(), 5);
    }

    #[test]
    fn chart_renders_through_the_widget_trait() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 5));
        Widget::render(
            Chart::new([Dataset::new("d", [(0.5, 0.5)])])
                .x_axis(Axis::new().bounds([0.0, 1.0]))
                .y_axis(Axis::new().bounds([0.0, 1.0])),
            Rect::new(0, 0, 10, 5),
            &mut buffer,
        );
        assert_eq!(buffer.get(5, 2).unwrap().symbol.as_str(), "•");
    }
}
