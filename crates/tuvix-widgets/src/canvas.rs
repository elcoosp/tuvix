//! Canvas: float-space drawing on a sub-cell dot grid.
//!
//! A [`Canvas`] maps a float coordinate space onto the terminal grid.
//! With [`Marker::Braille`] every terminal cell is a 2x4 dot grid (the
//! Unicode braille block), giving an 8x resolution surface; with
//! [`Marker::Dot`] / [`Marker::Block`] the grid is one glyph per cell.
//! Shapes draw through a [`Painter`]; text labels layer on top.

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Color;
use tuvix_core::style::Style;
use tuvix_core::text::Span;
use tuvix_core::widget::Widget;

use crate::block::Block;
use crate::marker::Marker;

/// The base code point of the Unicode braille block.
const BRAILLE_BASE: u32 = 0x2800;

/// A drawable shape: lines and points in canvas coordinates.
pub trait Shape {
    /// Rasterizes the shape into the painter's dot grid.
    fn draw(&self, painter: &mut Painter<'_>);
}

/// A scatter of points, all in one color.
#[derive(Debug, Clone, PartialEq)]
pub struct Points<'a> {
    /// The `(x, y)` canvas coordinates.
    pub coords: &'a [(f64, f64)],
    /// The color of every dot.
    pub color: Color,
}

impl Shape for Points<'_> {
    fn draw(&self, painter: &mut Painter<'_>) {
        for &(x, y) in self.coords {
            if let Some((px, py)) = painter.get_point(x, y) {
                painter.paint(px, py, self.color);
            }
        }
    }
}

/// A straight line between two canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    /// Start x.
    pub x1: f64,
    /// Start y.
    pub y1: f64,
    /// End x.
    pub x2: f64,
    /// End y.
    pub y2: f64,
    /// The color of the line dots.
    pub color: Color,
}

impl Line {
    /// Creates a line between two points.
    #[must_use]
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64, color: Color) -> Self {
        Self {
            x1,
            y1,
            x2,
            y2,
            color,
        }
    }
}

impl Shape for Line {
    fn draw(&self, painter: &mut Painter<'_>) {
        let Some(from) = painter.get_point(self.x1, self.y1) else {
            return;
        };
        let Some(to) = painter.get_point(self.x2, self.y2) else {
            return;
        };
        for (px, py) in crate::chart::bresenham(
            (
                i64::try_from(from.0).unwrap_or(0),
                i64::try_from(from.1).unwrap_or(0),
            ),
            (
                i64::try_from(to.0).unwrap_or(0),
                i64::try_from(to.1).unwrap_or(0),
            ),
        ) {
            let Ok(px) = usize::try_from(px) else {
                continue;
            };
            let Ok(py) = usize::try_from(py) else {
                continue;
            };
            painter.paint(px, py, self.color);
        }
    }
}

/// An axis-aligned rectangle outline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rectangle {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width (extends right).
    pub width: f64,
    /// Height (extends down).
    pub height: f64,
    /// The color of the outline dots.
    pub color: Color,
}

impl Shape for Rectangle {
    fn draw(&self, painter: &mut Painter<'_>) {
        for edge in [
            Line::new(self.x, self.y, self.x + self.width, self.y, self.color),
            Line::new(
                self.x + self.width,
                self.y,
                self.x + self.width,
                self.y + self.height,
                self.color,
            ),
            Line::new(
                self.x + self.width,
                self.y + self.height,
                self.x,
                self.y + self.height,
                self.color,
            ),
            Line::new(self.x, self.y + self.height, self.x, self.y, self.color),
        ] {
            edge.draw(painter);
        }
    }
}

/// The dot grid a [`Shape`] rasterizes into.
#[derive(Debug)]
pub struct Painter<'a> {
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
    resolution: (u16, u16),
    dirty: &'a mut [u8],
    colors: &'a mut [Color],
    marker: Marker,
}

impl Painter<'_> {
    /// The braille bit for a dot at `(column, row)` within a cell.
    fn braille_bit(column: usize, row: usize) -> u8 {
        match (column, row) {
            (0, 0) => 0x01,
            (0, 1) => 0x02,
            (0, 2) => 0x04,
            (1, 0) => 0x08,
            (1, 1) => 0x10,
            (1, 2) => 0x20,
            (0, 3) => 0x40,
            (1, 3) => 0x80,
            _ => 0,
        }
    }

    /// Maps a canvas coordinate to dot coordinates; `None` when the point
    /// is outside the bounds or the bounds are degenerate.
    #[must_use]
    pub fn get_point(&self, x: f64, y: f64) -> Option<(usize, usize)> {
        let [x_min, x_max] = self.x_bounds;
        let [y_min, y_max] = self.y_bounds;
        if x.is_nan() || y.is_nan() || x < x_min || x > x_max || y < y_min || y > y_max {
            return None;
        }
        let x_span = x_max - x_min;
        let y_span = y_max - y_min;
        if x_span <= 0.0 || y_span <= 0.0 {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the point is validated against the bounds above, so the \
                      dot coordinates are non-negative and below the canvas \
                      resolution; std has no lossless f64-to-usize step"
        )]
        let (px, py) = (
            ((x - x_min) / x_span * f64::from(self.resolution.0)) as usize,
            ((y_max - y) / y_span * f64::from(self.resolution.1)) as usize,
        );
        // A point exactly on the max bound clamps to the last dot.
        Some((
            px.min(usize::from(self.resolution.0) - 1),
            py.min(usize::from(self.resolution.1) - 1),
        ))
    }

    /// Paints one dot (or one cell for cell-space markers).
    pub fn paint(&mut self, x: usize, y: usize, color: Color) {
        let cells_w = self.cell_grid().0;
        if self.marker == Marker::Braille {
            let cell = (y / 4) * usize::from(cells_w) + (x / 2);
            if cell >= self.dirty.len() {
                return;
            }
            self.dirty[cell] |= Self::braille_bit(x % 2, y % 4);
            self.colors[cell] = color;
        } else {
            let cell = y * usize::from(cells_w) + x;
            if cell >= self.dirty.len() {
                return;
            }
            self.dirty[cell] = u8::MAX;
            self.colors[cell] = color;
        }
    }

    /// The `(width, height)` cell grid the dots rasterize into.
    fn cell_grid(&self) -> (u16, u16) {
        (
            self.resolution.0 / if self.marker == Marker::Braille { 2 } else { 1 },
            self.resolution.1 / if self.marker == Marker::Braille { 4 } else { 1 },
        )
    }

    /// Draws a shape into the grid.
    pub fn draw(&mut self, shape: &impl Shape) {
        shape.draw(self);
    }
}

/// A text label anchored at a canvas coordinate.
#[derive(Debug, Clone, PartialEq)]
struct Layer {
    anchor: (f64, f64),
    spans: Vec<Span>,
}

/// The drawing context a [`Canvas::paint`] closure receives.
#[derive(Debug)]
pub struct Context {
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
    resolution: (u16, u16),
    marker: Marker,
    dirty: Vec<u8>,
    colors: Vec<Color>,
    layers: Vec<Layer>,
}

impl Context {
    /// A painter over the accumulated dot grid.
    fn painter(&mut self) -> Painter<'_> {
        Painter {
            x_bounds: self.x_bounds,
            y_bounds: self.y_bounds,
            resolution: self.resolution,
            dirty: &mut self.dirty,
            colors: &mut self.colors,
            marker: self.marker,
        }
    }

    /// Draws a shape into the canvas.
    pub fn draw(&mut self, shape: &impl Shape) {
        self.painter().draw(shape);
    }

    /// Draws a text label anchored at a canvas coordinate (the label
    /// extends right from the anchor; points outside the bounds are
    /// skipped).
    pub fn print<T: Into<Vec<Span>>>(&mut self, x: f64, y: f64, spans: T) {
        self.layers.push(Layer {
            anchor: (x, y),
            spans: spans.into(),
        });
    }
}

/// A float-coordinate drawing surface rasterized onto the terminal grid.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::style::Color;
/// use tuvix_core::widget::Widget;
/// use tuvix_widgets::canvas::{Canvas, Line};
///
/// let canvas = Canvas::new()
///     .x_bounds([0.0, 2.0])
///     .y_bounds([0.0, 2.0])
///     .paint(|ctx| {
///         ctx.draw(&Line::new(0.0, 0.0, 2.0, 2.0, Color::Red));
///     });
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 3));
/// canvas.render(Rect::new(0, 0, 6, 3), &mut buffer);
/// // The rising diagonal enters the bottom-left cell as "⡠" (dots
/// // (0,11)+(1,10)) and leaves the top-right cell as "⠊".
/// assert_eq!(buffer.get(0, 2).unwrap().symbol.as_str(), "⡠");
/// assert_eq!(buffer.get(5, 0).unwrap().symbol.as_str(), "⠊");
/// ```
#[derive(Clone)]
pub struct Canvas {
    block: Option<Block>,
    marker: Marker,
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
    paint: std::sync::Arc<dyn Fn(&mut Context)>,
}

impl std::fmt::Debug for Canvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Canvas")
            .field("marker", &self.marker)
            .field("x_bounds", &self.x_bounds)
            .field("y_bounds", &self.y_bounds)
            .finish_non_exhaustive()
    }
}

impl Canvas {
    /// Creates a canvas with default bounds `[0.0, 0.0]` and the braille
    /// marker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            block: None,
            marker: Marker::Braille,
            x_bounds: [0.0, 0.0],
            y_bounds: [0.0, 0.0],
            paint: std::sync::Arc::new(|_| {}),
        }
    }

    /// Sets the x bounds.
    #[must_use]
    pub fn x_bounds(mut self, bounds: [f64; 2]) -> Self {
        self.x_bounds = bounds;
        self
    }

    /// Sets the y bounds.
    #[must_use]
    pub fn y_bounds(mut self, bounds: [f64; 2]) -> Self {
        self.y_bounds = bounds;
        self
    }

    /// Sets the marker (glyph mode) of the grid.
    #[must_use]
    pub fn marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
    }

    /// Wraps the canvas in a block; the grid fills the block's inner area.
    #[must_use]
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the drawing closure; it runs on every render.
    #[must_use]
    pub fn paint<F>(mut self, paint: F) -> Self
    where
        F: Fn(&mut Context) + 'static,
    {
        self.paint = std::sync::Arc::new(paint);
        self
    }

    /// Paints the canvas into `buffer`, clipped to `area`.
    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        if let Some(block) = &self.block {
            block.render(area, buffer);
        }
        let canvas_area = self.block.as_ref().map_or(area, |block| block.inner(area));
        if canvas_area.is_empty() {
            return;
        }

        // Dot resolution: 2x4 dots per cell for braille, 1 per cell else.
        let (dots_w, dots_h) = if self.marker == Marker::Braille {
            (
                u32::from(canvas_area.width) * 2,
                u32::from(canvas_area.height) * 4,
            )
        } else {
            (u32::from(canvas_area.width), u32::from(canvas_area.height))
        };

        let mut context = Context {
            x_bounds: self.x_bounds,
            y_bounds: self.y_bounds,
            resolution: (
                u16::try_from(dots_w).unwrap_or(u16::MAX),
                u16::try_from(dots_h).unwrap_or(u16::MAX),
            ),
            marker: self.marker,
            dirty: vec![0_u8; usize::try_from(dots_w * dots_h).unwrap_or(usize::MAX)],
            colors: vec![Color::Reset; usize::try_from(dots_w * dots_h).unwrap_or(usize::MAX)],
            layers: Vec::new(),
        };
        (self.paint)(&mut context);

        // Rasterize the dot grid into cells.
        let cells_w = usize::from(canvas_area.width);
        let dots_per_cell = if self.marker == Marker::Braille { 8 } else { 1 };
        for cell_index in 0..cells_w * usize::from(canvas_area.height) {
            let bits = context.dirty[cell_index];
            let symbol = if self.marker == Marker::Braille {
                if bits == 0 {
                    continue;
                }
                String::from_utf8_lossy(
                    &char::from_u32(BRAILLE_BASE + u32::from(bits))
                        .map(char_to_utf8)
                        .unwrap_or_default(),
                )
                .to_string()
            } else if bits != 0 {
                self.marker.cell_symbol().to_string()
            } else {
                continue;
            };
            let x = u16::try_from(cell_index % cells_w).unwrap_or(u16::MAX) + canvas_area.x;
            let y = u16::try_from(cell_index / cells_w).unwrap_or(u16::MAX) + canvas_area.y;
            if let Some(cell) = buffer.get_mut(x, y) {
                cell.set_symbol(symbol)
                    .set_style(Style::new().fg(context.colors[cell_index]));
            }
        }
        let _ = dots_per_cell;

        // Labels layer on top of the grid.
        for layer in &context.layers {
            let painter = Painter {
                x_bounds: self.x_bounds,
                y_bounds: self.y_bounds,
                resolution: context.resolution,
                dirty: &mut [],
                colors: &mut [],
                marker: self.marker,
            };
            let Some((px, py)) = painter.get_point(layer.anchor.0, layer.anchor.1) else {
                continue;
            };
            let (col, row) = if self.marker == Marker::Braille {
                (px / 2, py / 4)
            } else {
                (px, py)
            };
            if col >= usize::from(canvas_area.width) || row >= usize::from(canvas_area.height) {
                continue;
            }
            let x = u16::try_from(col).unwrap_or(u16::MAX) + canvas_area.x;
            let y = u16::try_from(row).unwrap_or(u16::MAX) + canvas_area.y;
            let mut cursor = x;
            for span in &layer.spans {
                if cursor >= canvas_area.right() {
                    break;
                }
                let (end_x, _) = buffer.set_stringn(
                    cursor,
                    y,
                    span.content.as_ref(),
                    canvas_area.right() - cursor,
                    span.style,
                );
                cursor = end_x;
            }
        }
    }
}

/// Converts a char to its UTF-8 bytes (helper for the braille raster).
fn char_to_utf8(c: char) -> Vec<u8> {
    let mut buf = [0_u8; 4];
    c.encode_utf8(&mut buf).as_bytes().to_vec()
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Canvas {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Canvas::render(&self, area, buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    #[test]
    fn braille_diagonal_walks_the_grid() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 3));
        Canvas::new()
            .x_bounds([0.0, 2.0])
            .y_bounds([0.0, 2.0])
            .paint(|ctx| {
                ctx.draw(&Line::new(0.0, 0.0, 2.0, 2.0, Color::Red));
            })
            .render(Rect::new(0, 0, 6, 3), &mut buffer);
        // The rising diagonal enters the bottom-left cell through dots
        // (0,11)=0x40 and (1,10)=0x20 -> bits 0x60, and leaves the
        // top-right cell as dot (11,0) = 0x08.
        assert_eq!(buffer.get(0, 2).unwrap().symbol.as_str(), "\u{2860}");
        assert_eq!(buffer.get(5, 0).unwrap().symbol.as_str(), "\u{280A}");
        assert_eq!(buffer.get(0, 2).unwrap().style.fg, Some(Color::Red));
    }

    #[test]
    fn points_outside_bounds_are_skipped() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        Canvas::new()
            .x_bounds([0.0, 1.0])
            .y_bounds([0.0, 1.0])
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &[(0.5, 0.5), (2.0, 0.5), (0.5, -1.0), (f64::NAN, 0.5)],
                    color: Color::Blue,
                });
            })
            .render(Rect::new(0, 0, 4, 2), &mut buffer);
        let text = buffer.to_string();
        let dots: usize = text
            .chars()
            .filter(|c| ('\u{2801}'..='\u{28FF}').contains(c))
            .count();
        assert_eq!(dots, 1);
    }

    #[test]
    fn degenerate_bounds_draw_nothing() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        Canvas::new()
            .x_bounds([1.0, 1.0])
            .y_bounds([0.0, 1.0])
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &[(1.0, 0.5)],
                    color: Color::Red,
                });
            })
            .render(Rect::new(0, 0, 4, 2), &mut buffer);
        assert!(
            buffer
                .content
                .iter()
                .all(tuvix_core::buffer::Cell::is_empty)
        );
    }

    #[test]
    fn rectangle_draws_its_outline() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 4));
        Canvas::new()
            .x_bounds([0.0, 4.0])
            .y_bounds([0.0, 4.0])
            .paint(|ctx| {
                ctx.draw(&Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: 4.0,
                    height: 4.0,
                    color: Color::Green,
                });
            })
            .render(Rect::new(0, 0, 8, 4), &mut buffer);
        // Every cell on the border carries braille dots.
        for x in 0..8_u16 {
            assert!(
                buffer.get(x, 0).unwrap().symbol.as_str() != " ",
                "top edge at x={x}"
            );
            assert!(
                buffer.get(x, 3).unwrap().symbol.as_str() != " ",
                "bottom edge at x={x}"
            );
        }
        for y in 0..4_u16 {
            assert!(
                buffer.get(0, y).unwrap().symbol.as_str() != " ",
                "left edge at y={y}"
            );
            assert!(
                buffer.get(7, y).unwrap().symbol.as_str() != " ",
                "right edge at y={y}"
            );
        }
    }

    #[test]
    fn labels_layer_on_top_of_the_grid() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 2));
        Canvas::new()
            .x_bounds([0.0, 1.0])
            .y_bounds([0.0, 1.0])
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &[(0.0, 0.0)],
                    color: Color::Red,
                });
                ctx.print(0.5, 0.5, [Span::raw("hi")]);
            })
            .render(Rect::new(0, 0, 8, 2), &mut buffer);
        let rendered = buffer.to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        // The (0,0) point lands bottom-left as "⡀"; the label anchors at
        // cell (4, 1).
        assert_eq!(rows[0], "        ");
        assert_eq!(rows[1], "⡀   hi  ");
    }

    #[test]
    fn labels_outside_bounds_are_skipped() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 2));
        Canvas::new()
            .x_bounds([0.0, 1.0])
            .y_bounds([0.0, 1.0])
            .paint(|ctx| {
                ctx.print(2.0, 0.5, [Span::raw("gone")]);
            })
            .render(Rect::new(0, 0, 8, 2), &mut buffer);
        assert!(
            buffer
                .content
                .iter()
                .all(tuvix_core::buffer::Cell::is_empty)
        );
    }

    #[test]
    fn dot_marker_uses_cell_resolution() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        Canvas::new()
            .marker(Marker::Dot)
            .x_bounds([0.0, 4.0])
            .y_bounds([0.0, 2.0])
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &[(0.0, 0.0)],
                    color: Color::Red,
                });
            })
            .render(Rect::new(0, 0, 4, 2), &mut buffer);
        // Bottom-left cell gets the dot glyph.
        assert_eq!(buffer.get(0, 1).unwrap().symbol.as_str(), "\u{2022}");
    }

    #[test]
    fn block_marker_uses_the_shaded_glyph() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        Canvas::new()
            .marker(Marker::Block)
            .x_bounds([0.0, 4.0])
            .y_bounds([0.0, 2.0])
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &[(0.0, 0.0)],
                    color: Color::Red,
                });
            })
            .render(Rect::new(0, 0, 4, 2), &mut buffer);
        assert_eq!(buffer.get(0, 1).unwrap().symbol.as_str(), "\u{2592}");
    }

    #[test]
    fn block_wraps_and_the_grid_fills_inner() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 3));
        Canvas::new()
            .x_bounds([0.0, 1.0])
            .y_bounds([0.0, 1.0])
            .block(Block::bordered())
            .paint(|ctx| {
                ctx.draw(&Points {
                    coords: &[(0.5, 1.0)],
                    color: Color::Red,
                });
            })
            .render(Rect::new(0, 0, 6, 3), &mut buffer);
        let rendered = buffer.to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        assert_eq!(rows[0], "┌────┐");
        assert!(rows[1].starts_with('│'));
        assert!(rows[1].contains('⠁'));
    }

    #[test]
    fn braille_bits_follow_the_unicode_dot_order() {
        // Dot (0,0) -> 0x01; dot (1,3) -> 0x80; dot (0,3) -> 0x40.
        assert_eq!(Painter::braille_bit(0, 0), 0x01);
        assert_eq!(Painter::braille_bit(1, 0), 0x08);
        assert_eq!(Painter::braille_bit(0, 3), 0x40);
        assert_eq!(Painter::braille_bit(1, 3), 0x80);
        assert_eq!(Painter::braille_bit(2, 0), 0x00);
    }

    #[test]
    fn canvas_renders_through_the_widget_trait() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        Widget::render(
            Canvas::new()
                .x_bounds([0.0, 1.0])
                .y_bounds([0.0, 1.0])
                .paint(|ctx| {
                    ctx.draw(&Points {
                        coords: &[(0.0, 0.0)],
                        color: Color::Red,
                    });
                }),
            Rect::new(0, 0, 4, 2),
            &mut buffer,
        );
        assert_eq!(buffer.get(0, 1).unwrap().symbol.as_str(), "\u{2840}");
    }
}
