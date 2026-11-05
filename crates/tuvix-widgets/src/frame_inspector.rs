//! FrameInspector: replay a recorded frame as a picture-in-picture
//! overlay (M9, architecture §5.8).
//!
//! The DevTools scrub view: hand it a [`Screen`] reconstructed from a
//! recorded frame ([`Screen::from_ansi`](tuvix_core::screen::Screen::from_ansi)
//! over a [`SessionRecorder`](tuvix_core::session::SessionRecorder) or
//! [`HistorySink`](tuvix_core::preview::HistorySink) capture) and it
//! paints that historical moment into a bordered box, optionally with a
//! timeline footer — a dot bar of the session's frames, the frame
//! counter and the recorded timestamp. Scrubbing is the caller's job:
//! pick a different frame, render again.
//!
//! The pasted cells carry their styles and hyperlinks verbatim, so an
//! inspected frame looks exactly like the moment it was captured.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::buffer::Buffer;
//! use tuvix_core::geometry::Rect;
//! use tuvix_core::preview::serialize_ansi;
//! use tuvix_core::screen::Screen;
//! use tuvix_core::widget::Widget;
//! use tuvix_widgets::FrameInspector;
//!
//! let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
//! buffer.set_string(0, 0, "hi", Default::default());
//! let screen = Screen::from_ansi(&serialize_ansi(&buffer)).unwrap();
//!
//! let mut frame = Buffer::empty(Rect::new(0, 0, 6, 3));
//! FrameInspector::new(screen)
//!     .timeline(0, 1)
//!     .render(Rect::new(0, 0, 6, 3), &mut frame);
//! // The recorded glyphs reappear inside the border.
//! assert!(frame.to_string().contains("hi"));
//! ```

use std::time::Duration;

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::screen::Screen;
use tuvix_core::style::Style;
use tuvix_core::widget::Widget;

use crate::block::Block;

/// A recorded frame rendered as an inspectable overlay: the historical
/// screen pasted into a bordered box with an optional timeline footer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameInspector {
    screen: Screen,
    title: Option<String>,
    border: bool,
    border_style: Style,
    timeline: Option<(usize, usize)>,
    at: Option<Duration>,
    footer_style: Style,
}

impl FrameInspector {
    /// Inspects `screen` — a frame reconstructed from a recording.
    pub fn new(screen: Screen) -> Self {
        Self {
            screen,
            title: None,
            border: true,
            border_style: Style::new(),
            timeline: None,
            at: None,
            footer_style: Style::new(),
        }
    }

    /// Sets the border title (drawn only when the border is on).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Toggles the surrounding border (on by default).
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Styles the border (and the title).
    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    /// Shows the timeline footer: `current` is the zero-based index of
    /// the displayed frame within a session of `total` frames.
    pub fn timeline(mut self, current: usize, total: usize) -> Self {
        self.timeline = Some((current, total.max(1)));
        self
    }

    /// Shows the frame's recorded timestamp in the footer.
    pub fn at(mut self, at: Duration) -> Self {
        self.at = Some(at);
        self
    }

    /// Styles the footer line.
    pub fn footer_style(mut self, style: Style) -> Self {
        self.footer_style = style;
        self
    }

    /// Renders the timeline footer into the last row of `area`.
    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        let Some((current, total)) = self.timeline else {
            return;
        };
        let mut footer = String::with_capacity(area.width as usize);
        // The dot bar: one dot per frame while they fit, otherwise a
        // proportional position bar over the available slots.
        let label = format!("frame {}/{}", current + 1, total);
        let stamp = self
            .at
            .map(|at| format!(" · {} ms", at.as_millis()))
            .unwrap_or_default();
        let reserved = (label.len() + stamp.len() + 1).min(area.width as usize);
        let slots = (area.width as usize - reserved).min(total);
        if slots > 0 {
            let filled = if total <= slots {
                current + 1
            } else {
                ((current + 1) * slots / total).max(1)
            };
            for dot in 0..slots {
                footer.push(if dot < filled { '●' } else { '○' });
            }
            footer.push(' ');
        }
        footer.push_str(&label);
        footer.push_str(&stamp);
        buf.set_string(area.x, area.y, footer.as_str(), self.footer_style);
    }
}

impl Widget for FrameInspector {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        // The border (when it fits) carves out the content area.
        let content = if self.border && area.width >= 2 && area.height >= 2 {
            let mut block = Block::bordered().border_style(self.border_style);
            if let Some(title) = &self.title {
                block = block.title(title.clone());
            }
            let inner = block.inner(area);
            block.render(area, buf);
            inner
        } else {
            area
        };
        if content.is_empty() {
            return;
        }
        // The footer claims the last content row (leaving at least one
        // row for the screen itself); the screen pastes above it.
        let (screen_area, footer_area) = match self.timeline {
            Some(_) if content.height >= 2 => (
                Rect::new(content.x, content.y, content.width, content.height - 1),
                Rect::new(content.x, content.y + content.height - 1, content.width, 1),
            ),
            _ => (content, Rect::new(content.x, content.y, 0, 0)),
        };
        if !screen_area.is_empty() {
            let copy_width = self.screen.width().min(screen_area.width);
            let copy_height = self.screen.height().min(screen_area.height);
            for row in 0..copy_height {
                for col in 0..copy_width {
                    if let Some(cell) = self.screen.cell(col, row)
                        && let Some(target) = buf.get_mut(screen_area.x + col, screen_area.y + row)
                    {
                        *target = cell.clone();
                    }
                }
            }
        }
        if !footer_area.is_empty() {
            self.render_footer(footer_area, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::preview::serialize_ansi;
    use tuvix_core::style::{Color, Modifier};

    fn screen_of(text: &str, width: u16, height: u16) -> Screen {
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
        buffer.set_string(0, 0, text, Style::new());
        Screen::from_ansi(&serialize_ansi(&buffer)).unwrap()
    }

    #[test]
    fn bordered_inspector_replays_the_screen() {
        let screen = screen_of("hi", 2, 1);
        let mut frame = Buffer::empty(Rect::new(0, 0, 6, 3));
        FrameInspector::new(screen).render(Rect::new(0, 0, 6, 3), &mut frame);
        let rendered = frame.to_string();
        // Border drawn...
        assert!(rendered.contains("┌────┐"));
        assert!(rendered.contains("└────┘"));
        // ...and the historical glyphs inside it.
        assert!(rendered.contains("│hi  │"));
    }

    #[test]
    fn borderless_pastes_at_the_origin() {
        let screen = screen_of("hey", 3, 1);
        let mut frame = Buffer::empty(Rect::new(0, 0, 5, 1));
        FrameInspector::new(screen)
            .border(false)
            .render(Rect::new(0, 0, 5, 1), &mut frame);
        assert_eq!(frame.to_string(), "hey  ");
    }

    #[test]
    fn styles_and_links_travel_with_the_cells() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        buffer.set_string(
            0,
            0,
            "red",
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        );
        let screen = Screen::from_ansi(&serialize_ansi(&buffer)).unwrap();
        let mut frame = Buffer::empty(Rect::new(0, 0, 3, 1));
        FrameInspector::new(screen)
            .border(false)
            .render(Rect::new(0, 0, 3, 1), &mut frame);
        let cell = frame.get(1, 0).unwrap();
        assert_eq!(cell.style.fg, Some(Color::Red));
        assert!(cell.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn larger_screens_clip_to_the_area() {
        let screen = screen_of("0123456789", 10, 1);
        let mut frame = Buffer::empty(Rect::new(0, 0, 4, 1));
        FrameInspector::new(screen)
            .border(false)
            .render(Rect::new(0, 0, 4, 1), &mut frame);
        assert_eq!(frame.to_string(), "0123");
    }

    #[test]
    fn timeline_footer_shows_dots_and_counter() {
        let screen = screen_of("x", 1, 1);
        let mut frame = Buffer::empty(Rect::new(0, 0, 20, 4));
        FrameInspector::new(screen)
            .timeline(1, 3)
            .render(Rect::new(0, 0, 20, 4), &mut frame);
        let rendered = frame.to_string();
        // One dot per frame: the first two filled.
        assert!(rendered.contains("●●○ frame 2/3"), "{rendered}");
    }

    #[test]
    fn timeline_dots_compress_when_frames_exceed_width() {
        let screen = screen_of("x", 1, 1);
        // 10 frames into 4 dot slots (15 wide - 11 reserved): frame 9
        // (index 8) fills (8+1)*4/10 = 3 of them.
        let mut frame = Buffer::empty(Rect::new(0, 0, 15, 2));
        FrameInspector::new(screen)
            .border(false)
            .timeline(8, 10)
            .render(Rect::new(0, 0, 15, 2), &mut frame);
        let rendered = frame.to_string();
        assert!(rendered.contains("●●●○ frame 9/10"), "{rendered}");
    }

    #[test]
    fn footer_shows_the_timestamp() {
        let screen = screen_of("x", 1, 1);
        let mut frame = Buffer::empty(Rect::new(0, 0, 30, 2));
        FrameInspector::new(screen)
            .border(false)
            .timeline(0, 2)
            .at(Duration::from_millis(128))
            .render(Rect::new(0, 0, 30, 2), &mut frame);
        let rendered = frame.to_string();
        assert!(rendered.contains("●○ frame 1/2 · 128 ms"), "{rendered}");
    }

    #[test]
    fn footer_sits_inside_the_border() {
        let screen = screen_of("x", 1, 1);
        let mut frame = Buffer::empty(Rect::new(0, 0, 20, 4));
        FrameInspector::new(screen)
            .timeline(0, 2)
            .render(Rect::new(0, 0, 20, 4), &mut frame);
        let rendered = frame.to_string();
        // The footer is the last inner row, inside the border.
        assert!(rendered.contains("│●○ frame 1/2"), "{rendered}");
        // The screen pastes above the footer, not over it.
        let lines: Vec<&str> = rendered.split('\n').collect();
        assert!(lines[1].contains('x'));
        assert!(!lines[3].contains("x frame"), "{rendered}");
    }

    #[test]
    fn title_renders_in_the_border() {
        let screen = screen_of("x", 1, 1);
        let mut frame = Buffer::empty(Rect::new(0, 0, 16, 3));
        FrameInspector::new(screen)
            .title("time travel")
            .render(Rect::new(0, 0, 16, 3), &mut frame);
        let rendered = frame.to_string();
        let first_line = rendered.lines().next().unwrap();
        assert!(first_line.starts_with('┌'));
        assert!(first_line.contains("time travel"), "{rendered}");
    }

    #[test]
    fn tiny_areas_degrade_gracefully() {
        let screen = screen_of("hello", 5, 1);
        // One row: no border, no footer — just the paste.
        let mut frame = Buffer::empty(Rect::new(0, 0, 5, 1));
        FrameInspector::new(screen.clone())
            .timeline(0, 1)
            .render(Rect::new(0, 0, 5, 1), &mut frame);
        assert_eq!(frame.to_string(), "hello");
        // Zero-size areas do nothing.
        let mut empty = Buffer::empty(Rect::new(0, 0, 0, 0));
        FrameInspector::new(screen).render(Rect::new(0, 0, 0, 0), &mut empty);
        assert_eq!(empty.to_string(), "");
    }

    #[test]
    fn footer_style_applies() {
        let screen = screen_of("x", 1, 1);
        let mut frame = Buffer::empty(Rect::new(0, 0, 16, 2));
        FrameInspector::new(screen)
            .border(false)
            .timeline(0, 1)
            .footer_style(Style::new().fg(Color::Cyan))
            .render(Rect::new(0, 0, 16, 2), &mut frame);
        let cell = frame.get(0, 1).unwrap();
        assert_eq!(cell.style.fg, Some(Color::Cyan));
    }

    #[test]
    fn inspector_is_debug_clone_eq() {
        let screen = screen_of("x", 1, 1);
        let inspector = FrameInspector::new(screen.clone());
        let clone = inspector.clone();
        assert_eq!(inspector, clone);
        assert!(format!("{inspector:?}").contains("FrameInspector"));
    }
}
