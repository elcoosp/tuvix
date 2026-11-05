//! The double-buffered terminal runtime.
//!
//! [`Terminal`] owns a [`Backend`] and two [`Buffer`]s. Every
//! [`Terminal::draw`] call:
//!
//! 1. resizes the buffers if the backend size changed (clearing the screen),
//! 2. clears the screen once before the very first frame,
//! 3. lets the render closure paint the current buffer through a [`Frame`],
//! 4. diffs the previous visible buffer against the new one and streams the
//!    resulting diff runs to the backend,
//! 5. shows/hides or moves the cursor, only when its state actually changed,
//! 6. swaps the buffers and flushes the backend.

use crate::backend::{Backend, ClearRegion};
use crate::buffer::Buffer;
use crate::geometry::{Position, Rect, Size};
use crate::preview::FrameSink;

/// A frame handed to the render closure of [`Terminal::draw`].
///
/// Widgets paint into `buffer`; the frame also carries the render area and an
/// optional cursor position request.
#[derive(Debug)]
pub struct Frame<'a> {
    area: Rect,
    /// The buffer to paint into; it is the one about to become visible.
    pub buffer: &'a mut Buffer,
    cursor_position: Option<Position>,
}

impl Frame<'_> {
    /// The area of this frame (the current terminal viewport).
    pub fn area(&self) -> Rect {
        self.area
    }

    /// Requests the cursor to be shown at `position` for this frame.
    ///
    /// When the closure does not call this, the cursor is hidden during
    /// rendering.
    pub fn set_cursor_position(&mut self, position: Position) {
        self.cursor_position = Some(position);
    }

    /// The cursor position requested by this frame, if any.
    pub fn cursor_position(&self) -> Option<Position> {
        self.cursor_position
    }
}

/// The rendering runtime driving a [`Backend`] with double buffering.
///
/// Two construction modes exist:
///
/// - [`Terminal::new`] — the classic immediate-mode contract: every frame
///   repaints the whole screen logically into a freshly reset buffer and
///   the diff minimizes what reaches the terminal.
/// - [`Terminal::new_persistent`] — the persistent-DOM contract (M4): the
///   paint buffer is never reset between frames, widgets see their previous
///   output and repaint only what changed, and a
///   [`FrameJournal`](crate::journal::FrameJournal) records
///   every touched cell. [`Terminal::draw_persistent`] then turns no-op
///   frames into the O(1) decision (no diff walk, no backend draw) and
///   partial frames into banded diffs that touch only the painted bands.
///
/// # Examples
///
/// ```
/// use tuvix_core::backend::TestBackend;
/// use tuvix_core::geometry::Size;
/// use tuvix_core::terminal::Terminal;
///
/// let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 2))).unwrap();
/// terminal.draw(|frame| {
///     frame.buffer.set_string(0, 0, "hello", Default::default());
/// }).unwrap();
/// ```
///
/// ```
/// use tuvix_core::backend::TestBackend;
/// use tuvix_core::geometry::Size;
/// use tuvix_core::terminal::Terminal;
///
/// let mut terminal = Terminal::new_persistent(TestBackend::new(Size::new(10, 2))).unwrap();
/// terminal.draw_persistent(|frame| {
///     frame.buffer.set_string(0, 0, "hello", Default::default());
/// }).unwrap();
/// // A frame that paints nothing costs O(1) and emits nothing.
/// terminal.draw_persistent(|_| {}).unwrap();
/// ```
pub struct Terminal<B: Backend> {
    backend: B,
    buffers: [Buffer; 2],
    current: usize,
    hidden_cursor: bool,
    needs_initial_clear: bool,
    /// Persistent-DOM mode: `buffers[0]` is the fixed paint target (the DOM,
    /// never reset between frames) and `buffers[1]` mirrors what the backend
    /// last received; after every emitted frame the reference is synced from
    /// the DOM — whole-buffer on full diffs, band-cells only on banded ones.
    persistent: bool,
    /// Optional IPC preview sink: every flushed frame is serialized to a
    /// full-frame ANSI snapshot and handed to it (M4).
    preview: Option<Box<dyn FrameSink>>,
}

impl<B: Backend + core::fmt::Debug> core::fmt::Debug for Terminal<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Terminal")
            .field("backend", &self.backend)
            .field("current", &self.current)
            .field("hidden_cursor", &self.hidden_cursor)
            .field("persistent", &self.persistent)
            .finish_non_exhaustive()
    }
}

impl<B: Backend> Terminal<B> {
    /// Creates a terminal for `backend`, querying its initial size.
    pub fn new(backend: B) -> Result<Self, B::Error> {
        let size = backend.size()?;
        let area = Rect::from_size(size);
        Ok(Self {
            backend,
            buffers: [Buffer::empty(area), Buffer::empty(area)],
            current: 0,
            hidden_cursor: false,
            needs_initial_clear: true,
            persistent: false,
            preview: None,
        })
    }

    /// Creates a persistent-DOM terminal for `backend`.
    ///
    /// The paint buffer carries its content across frames (widgets repaint
    /// only their changed regions), and [`Terminal::draw_persistent`] gets
    /// the O(1) no-op frame decision from the frame journal.
    pub fn new_persistent(backend: B) -> Result<Self, B::Error> {
        let size = backend.size()?;
        let area = Rect::from_size(size);
        let mut paint = Buffer::empty(area);
        paint.enable_journal();
        Ok(Self {
            backend,
            buffers: [paint, Buffer::empty(area)],
            current: 0,
            hidden_cursor: false,
            needs_initial_clear: true,
            persistent: true,
            preview: None,
        })
    }

    /// Shared access to the backend (e.g. to assert on a test backend).
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Exclusive access to the backend.
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Streams every flushed frame to `sink` as a full-frame ANSI snapshot
    /// (the M4 IPC preview). Pass a [`PreviewSink`](crate::preview::PreviewSink)
    /// from a [`PreviewTap`](crate::preview::PreviewTap), or any
    /// [`FrameSink`] implementation (recorders, debug dumps).
    ///
    /// The preview is a cold path by construction: one full-frame
    /// serialization per draw, opt-in. Call with `None`-typed helper
    /// [`Terminal::clear_preview`] to detach.
    pub fn set_preview(&mut self, sink: impl FrameSink + 'static) {
        self.preview = Some(Box::new(sink));
    }

    /// Detaches the preview sink.
    pub fn clear_preview(&mut self) {
        self.preview = None;
    }

    /// The buffer modeling what the terminal currently shows. Immediate
    /// mode swaps after every emit, so the displayed content lives in the
    /// *other* half; persistent mode keeps the DOM in `buffers[0]`.
    fn visible_buffer(&self) -> &Buffer {
        if self.persistent {
            &self.buffers[0]
        } else {
            &self.buffers[1 - self.current]
        }
    }

    /// Hands the visible state to the preview sink, if one is attached.
    /// Sink failures are swallowed: a dead preview must never kill a draw.
    fn emit_preview(&mut self) {
        if self.preview.is_some() {
            // Compute first, then hand over: two disjoint field borrows
            // cannot overlap across the method call.
            let snapshot = crate::preview::serialize_ansi(self.visible_buffer());
            if let Some(sink) = self.preview.as_mut() {
                sink.on_frame(&snapshot);
            }
        }
    }

    /// The live size reported by the backend.
    pub fn size(&self) -> Result<Size, B::Error> {
        self.backend.size()
    }

    /// Whether this terminal keeps a persistent DOM across frames.
    pub fn is_persistent(&self) -> bool {
        self.persistent
    }

    /// Renders one frame with `render` and flushes only the changed runs.
    ///
    /// In immediate mode the render closure repaints the whole screen into
    /// a blank canvas. In persistent mode the closure paints over the
    /// previous frame's output; the diff still covers the full screen.
    /// Use [`Terminal::draw_persistent`] for the journal-driven path.
    pub fn draw(&mut self, render: impl FnOnce(&mut Frame<'_>)) -> Result<(), B::Error> {
        self.autoresize()?;
        if self.needs_initial_clear {
            self.backend.clear(ClearRegion::All)?;
            self.needs_initial_clear = false;
        }

        let paint_index = self.current;
        let area = self.buffers[paint_index].area;
        let cursor_position = {
            let mut frame = Frame {
                area,
                buffer: &mut self.buffers[paint_index],
                cursor_position: None,
            };
            render(&mut frame);
            frame.cursor_position
        };

        let previous_index = 1 - paint_index;
        let (previous, next) = (&self.buffers[previous_index], &self.buffers[paint_index]);
        let runs = previous.diff_runs(next);
        self.backend.draw(runs)?;
        self.apply_cursor(cursor_position)?;

        if self.persistent {
            // The backend now shows the DOM; the reference must model it.
            // The DOM buffer (buffers[0]) keeps its content — that IS the
            // persistent DOM — and the reference is synced from it.
            self.sync_reference_from_dom();
        } else {
            // Reset the buffer that was displayed until now: it becomes the
            // render target of the next frame, while the just-painted buffer
            // keeps this frame's content as the next diff reference.
            self.buffers[1 - self.current].reset();
            self.current = 1 - self.current;
        }
        self.backend.flush()?;
        self.emit_preview();
        Ok(())
    }

    /// Renders one frame through the frame journal (the M4 path).
    ///
    /// The closure paints over the previous frame's output; the journal
    /// decides how much work the frame costs:
    ///
    /// - painted nothing: **O(1)** — no diff walk, no backend draw; only
    ///   cursor memoization and the flush happen;
    /// - painted part of the screen: the diff is restricted to the painted
    ///   row bands (byte-identical output to the full walk);
    /// - painted everything (or the journal flipped to full): the classic
    ///   full diff runs.
    ///
    /// On immediate-mode terminals this method still works — an empty
    /// closure is a true no-op — but any paint falls back to the full diff,
    /// because the blank canvas differs from the displayed buffer outside
    /// the painted bands. Persistent terminals ([`Terminal::new_persistent`]
    /// + [`Terminal::draw_persistent`]) get the full benefit.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuvix_core::backend::TestBackend;
    /// use tuvix_core::geometry::Size;
    /// use tuvix_core::terminal::Terminal;
    ///
    /// let mut terminal = Terminal::new_persistent(TestBackend::new(Size::new(8, 2))).unwrap();
    /// terminal.draw_persistent(|frame| {
    ///     frame.buffer.set_string(0, 0, "static", Default::default());
    /// })?;
    /// // The DOM carries over: repainting the same content marks the
    /// // journal, but the diff emits nothing — the screen is unchanged.
    /// terminal.draw_persistent(|frame| {
    ///     frame.buffer.set_string(0, 0, "static", Default::default());
    /// })?;
    /// // A closure that paints nothing skips the diff walk entirely.
    /// terminal.draw_persistent(|_| {})?;
    /// # Ok::<(), core::convert::Infallible>(())
    /// ```
    pub fn draw_persistent(&mut self, render: impl FnOnce(&mut Frame<'_>)) -> Result<(), B::Error> {
        self.autoresize()?;
        if self.needs_initial_clear {
            self.backend.clear(ClearRegion::All)?;
            self.needs_initial_clear = false;
        }

        let paint_index = self.current;
        if !self.buffers[paint_index].has_journal() {
            self.buffers[paint_index].enable_journal();
        }
        let area = self.buffers[paint_index].area;
        let cursor_position = {
            let mut frame = Frame {
                area,
                buffer: &mut self.buffers[paint_index],
                cursor_position: None,
            };
            render(&mut frame);
            frame.cursor_position
        };

        // The O(1) no-op decision: nothing painted this frame. Roles stay
        // stable — the reference must keep modeling what the backend shows,
        // and the (blank or unchanged) paint target is exactly what the next
        // frame paints over.
        if self.buffers[paint_index].journal_is_empty() {
            self.apply_cursor(cursor_position)?;
            self.buffers[paint_index].clear_journal();
            self.backend.flush()?;
            self.emit_preview();
            return Ok(());
        }

        // Immediate mode cannot trust bands: the blank canvas differs from
        // the displayed buffer outside the painted bands. Persistent mode
        // keeps every mutation journaled, so bands are exhaustive there.
        if !self.persistent && !self.buffers[paint_index].journal_is_full() {
            self.buffers[paint_index].mark_fully_painted();
        }

        let previous_index = 1 - paint_index;
        let (previous, next) = (&self.buffers[previous_index], &self.buffers[paint_index]);
        let runs = if next.journal_is_full() {
            previous.diff_runs(next)
        } else {
            previous.diff_runs_banded(next, next.journal_bands())
        };
        self.backend.draw(runs)?;
        self.apply_cursor(cursor_position)?;

        if self.persistent {
            // Reference sync: banded frames copy only the painted bands,
            // full frames copy the whole buffer. Either way the reference
            // models exactly what the backend now shows, and the DOM buffer
            // keeps its content for the next frame.
            self.sync_reference_from_dom();
        } else {
            self.buffers[1 - self.current].reset();
            self.current = 1 - self.current;
        }
        self.backend.flush()?;
        self.emit_preview();
        Ok(())
    }

    /// Syncs the reference buffer (`buffers[1]`) from the DOM buffer
    /// (`buffers[0]`) after an emit, copying only the journaled bands when
    /// the frame was partial and the whole buffer when it was full. Both
    /// journals end the frame cleared.
    fn sync_reference_from_dom(&mut self) {
        // Array destructuring gives the borrow checker two provably disjoint
        // halves; method calls on `self.buffers` cannot prove disjointness.
        let [dom, reference] = &mut self.buffers;
        let full = dom.journal_is_full();
        if full {
            reference.clone_from(dom);
        } else {
            // Bands are `Copy` and few; copy them out first so the copy loop
            // does not hold a borrow across the two buffer halves.
            let bands: Vec<crate::damage::Band> = dom.journal_bands().to_vec();
            let width = dom.area.width as usize;
            for band in bands {
                let start = band.row as usize * width + band.start as usize;
                let end = band.row as usize * width + band.end as usize;
                let source = &dom.content[start..end];
                reference.content[start..end].clone_from_slice(source);
            }
        }
        reference.clear_journal();
        dom.clear_journal();
    }

    /// Applies the frame's cursor request with memoization: the cursor is
    /// shown/moved only when its state actually changed.
    fn apply_cursor(&mut self, cursor_position: Option<Position>) -> Result<(), B::Error> {
        match cursor_position {
            Some(position) => {
                if self.hidden_cursor {
                    self.backend.show_cursor()?;
                    self.hidden_cursor = false;
                }
                self.backend.set_cursor_position(position)?;
            }
            None => {
                if !self.hidden_cursor {
                    self.backend.hide_cursor()?;
                    self.hidden_cursor = true;
                }
            }
        }
        Ok(())
    }

    /// Clears the screen and forgets the buffered content.
    ///
    /// In persistent mode this wipes the DOM: both buffers reset and their
    /// journals flip to full, so the next frame repaints everything.
    pub fn clear(&mut self) -> Result<(), B::Error> {
        self.backend.clear(ClearRegion::All)?;
        self.buffers[0].reset();
        self.buffers[1].reset();
        Ok(())
    }

    /// Hides the cursor (memoized: a no-op when already hidden).
    pub fn hide_cursor(&mut self) -> Result<(), B::Error> {
        if !self.hidden_cursor {
            self.backend.hide_cursor()?;
            self.hidden_cursor = true;
        }
        Ok(())
    }

    /// Shows the cursor (memoized: a no-op when already visible).
    pub fn show_cursor(&mut self) -> Result<(), B::Error> {
        if self.hidden_cursor {
            self.backend.show_cursor()?;
            self.hidden_cursor = false;
        }
        Ok(())
    }

    /// Resizes the buffers when the backend size changed, clearing the screen
    /// so no stale content survives the repaint.
    fn autoresize(&mut self) -> Result<(), B::Error> {
        let size = self.backend.size()?;
        if self.buffers[self.current].size() != size {
            let area = Rect::from_size(size);
            self.buffers[0].resize(area);
            self.buffers[1].resize(area);
            self.backend.clear(ClearRegion::All)?;
            self.needs_initial_clear = false;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::TestBackend;
    use crate::geometry::Position;
    use crate::style::{Color, Style};

    fn terminal_10x3() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(Size::new(10, 3))).unwrap()
    }

    #[test]
    fn draw_renders_content_into_backend() {
        let mut terminal = terminal_10x3();
        terminal
            .draw(|frame| {
                frame
                    .buffer
                    .set_string(0, 1, "hi", Style::new().fg(Color::Red));
            })
            .unwrap();
        let rendered = terminal.backend().buffer().to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        assert_eq!(rows[1], "hi        ");
    }

    #[test]
    fn successive_draws_only_send_changed_cells() {
        let mut terminal = terminal_10x3();
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, "hi", Style::new());
            })
            .unwrap();
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, "hey", Style::new());
            })
            .unwrap();
        let rendered = terminal.backend().buffer().to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        assert_eq!(rows[0], "hey       ");
    }

    #[test]
    fn repainting_identical_content_leaves_screen_stable() {
        // tuvix is immediate-mode: every frame paints the whole screen; the
        // diff engine then minimizes what reaches the terminal.
        let mut terminal = terminal_10x3();
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, "stable", Style::new());
            })
            .unwrap();
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, "stable", Style::new());
                frame.buffer.set_string(8, 2, "x", Style::new());
            })
            .unwrap();
        let rendered = terminal.backend().buffer().to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        assert_eq!(rows[0], "stable    ");
        assert_eq!(rows[2], "        x ");
    }

    #[test]
    fn clearing_a_region_in_frame_repaints_it_blank() {
        let mut terminal = terminal_10x3();
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, "old", Style::new());
            })
            .unwrap();
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, "n", Style::new());
            })
            .unwrap();
        let rendered = terminal.backend().buffer().to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        assert_eq!(rows[0], "n         ");
    }

    #[test]
    fn cursor_hidden_by_default_after_first_draw() {
        let mut terminal = terminal_10x3();
        terminal.draw(|_| {}).unwrap();
        assert!(terminal.backend().is_cursor_hidden());
    }

    #[test]
    fn requested_cursor_position_is_shown() {
        let mut terminal = terminal_10x3();
        terminal
            .draw(|frame| frame.set_cursor_position(Position::new(4, 2)))
            .unwrap();
        assert!(!terminal.backend().is_cursor_hidden());
        assert_eq!(terminal.backend().cursor_position(), Position::new(4, 2));
    }

    #[test]
    fn cursor_commands_are_memoized() {
        let mut terminal = terminal_10x3();
        terminal
            .draw(|frame| frame.set_cursor_position(Position::new(1, 1)))
            .unwrap();
        // A second frame with the same position still works and keeps state
        // consistent (the backend flags would drift if double-issued).
        terminal
            .draw(|frame| frame.set_cursor_position(Position::new(1, 1)))
            .unwrap();
        assert_eq!(terminal.backend().cursor_position(), Position::new(1, 1));
    }

    #[test]
    fn wide_characters_render_through_the_full_stack() {
        let mut terminal = terminal_10x3();
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, "汉a", Style::new());
            })
            .unwrap();
        let rendered = terminal.backend().buffer().to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        assert_eq!(rows[0], "汉·a       ");
    }

    #[test]
    fn clear_empties_the_screen() {
        let mut terminal = terminal_10x3();
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, "junk", Style::new());
            })
            .unwrap();
        terminal.clear().unwrap();
        assert_eq!(
            terminal.backend().buffer().to_string(),
            "          \n          \n          "
        );
    }

    #[test]
    fn autoresize_rebuilds_and_repaints() {
        let mut terminal = Terminal::new(TestBackend::new(Size::new(6, 1))).unwrap();
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, "hi", Style::new());
            })
            .unwrap();
        terminal.backend_mut().resize(Size::new(8, 2));
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, "hello", Style::new());
            })
            .unwrap();
        assert_eq!(
            terminal.backend().buffer().to_string(),
            "hello   \n        "
        );
    }

    #[test]
    fn hide_and_show_cursor_are_memoized_between_draws() {
        let mut terminal = terminal_10x3();
        terminal.draw(|_| {}).unwrap(); // hides
        terminal.hide_cursor().unwrap();
        assert!(terminal.backend().is_cursor_hidden());
        terminal.show_cursor().unwrap();
        assert!(!terminal.backend().is_cursor_hidden());
    }

    #[test]
    fn frame_area_matches_backend_size() {
        let mut terminal = terminal_10x3();
        let mut observed = None;
        terminal
            .draw(|frame| {
                observed = Some(frame.area());
            })
            .unwrap();
        assert_eq!(observed, Some(Rect::new(0, 0, 10, 3)));
    }

    #[test]
    fn draw_returns_backend_result() {
        let mut terminal = terminal_10x3();
        let result = terminal.draw(|_| {});
        assert!(result.is_ok());
    }

    #[test]
    fn diff_runs_stream_to_backend_without_allocation_leaks() {
        // Regression guard: a frame painting the full screen must leave the
        // visible state exactly equal to the logical frame content.
        let mut terminal = Terminal::new(TestBackend::new(Size::new(8, 2))).unwrap();
        terminal
            .draw(|frame| {
                frame
                    .buffer
                    .set_string(0, 0, "abcdefgh", Style::new().fg(Color::Blue));
                frame.buffer.set_string(0, 1, "ijklmnop", Style::new());
            })
            .unwrap();
        assert_eq!(
            terminal.backend().buffer().to_string(),
            "abcdefgh\nijklmnop"
        );
    }
}
