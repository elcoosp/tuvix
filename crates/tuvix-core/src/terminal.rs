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
//!
//! The cancellable variants ([`Terminal::draw_cancellable`],
//! [`Terminal::draw_persistent_cancellable`]) add the M8 contract between
//! steps 1 and 4: a [`CancellationToken`]
//! is checked at the stage boundaries, and an abandoned frame is
//! **discarded, not committed** — the terminal shows exactly what it showed
//! before and the buffers return to their pre-frame state.

use crate::backend::{Backend, ClearRegion};
use crate::buffer::Buffer;
use crate::cancellation::CancellationToken;
use crate::geometry::{Position, Rect, Size};
use crate::preview::FrameSink;

/// What a cancellable draw did with its frame.
///
/// `Committed` means the frame went through the full pipeline (diff, draw,
/// cursor, flush). `Cancelled` means a checkpoint observed the
/// [`CancellationToken`] and the
/// frame was abandoned: **no terminal output happened**, and the buffers
/// are exactly as they were before the draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawOutcome {
    /// The frame was rendered and flushed to the backend.
    Committed,
    /// The frame was abandoned at a stage boundary; nothing reached the
    /// terminal and no buffer state survived it.
    Cancelled,
}

impl DrawOutcome {
    /// Whether the frame reached the terminal.
    pub fn is_committed(self) -> bool {
        matches!(self, Self::Committed)
    }

    /// Whether the frame was abandoned before any terminal output.
    pub fn is_cancelled(self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

/// A frame handed to the render closure of [`Terminal::draw`].
///
/// Widgets paint into `buffer`; the frame also carries the render area, an
/// optional cursor position request and — on the cancellable draw paths —
/// the pass's [`CancellationToken`].
#[derive(Debug)]
pub struct Frame<'a> {
    area: Rect,
    /// The buffer to paint into; it is the one about to become visible.
    pub buffer: &'a mut Buffer,
    cursor_position: Option<Position>,
    /// The cancellation signal of a cancellable draw; `None` on the classic
    /// paths, whose frames can never be abandoned mid-paint.
    cancellation: Option<CancellationToken>,
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

    /// The cancellation token of this render pass, when it is cancellable.
    ///
    /// Long paints should check it between widgets (the stage boundaries of
    /// the architecture's cooperative-cancellation model): once the token
    /// is cancelled, everything painted so far will be discarded, so
    /// continuing to paint is wasted work.
    pub fn cancellation(&self) -> Option<&CancellationToken> {
        self.cancellation.as_ref()
    }

    /// Whether this render pass has been abandoned.
    ///
    /// `false` on the classic (non-cancellable) paths, whose frames can
    /// never be abandoned.
    pub fn is_cancelled(&self) -> bool {
        self.cancellation
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
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
        self.draw_inner(render, None).map(|outcome| {
            debug_assert!(outcome.is_committed());
        })
    }

    /// Renders one frame that input is allowed to preempt.
    ///
    /// The pass is checked against `token` at the two stage boundaries
    /// where aborting is safe and cheap:
    ///
    /// 1. **before the pass** — an already-cancelled token returns
    ///    [`DrawOutcome::Cancelled`] with *zero* terminal I/O (no resize
    ///    probe, no clear, no flush);
    /// 2. **after the paint closure** — the closure observes the same token
    ///    through [`Frame::cancellation`] and may abandon the pass between
    ///    widgets; every cell it painted is discarded, the buffers return
    ///    to their pre-frame state and nothing reaches the terminal.
    ///
    /// The diff/draw/flush tail is never interrupted: it is the cheap,
    /// allocation-free part of the pipeline, and abandoning it after the
    /// backend already received bytes would corrupt the raster state.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuvix_core::backend::TestBackend;
    /// use tuvix_core::cancellation::CancellationToken;
    /// use tuvix_core::geometry::Size;
    /// use tuvix_core::terminal::{DrawOutcome, Terminal};
    ///
    /// let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 2))).unwrap();
    ///
    /// // A committed frame paints as usual.
    /// let token = CancellationToken::new();
    /// let pass = token.child();
    /// let outcome = terminal.draw_cancellable(&pass, |frame| {
    ///     frame.buffer.set_string(0, 0, "hi", Default::default());
    /// }).unwrap();
    /// assert_eq!(outcome, DrawOutcome::Committed);
    ///
    /// // A pre-cancelled token abandons the frame before any I/O.
    /// token.cancel();
    /// let outcome = terminal
    ///     .draw_cancellable(&token, |frame| {
    ///         frame.buffer.set_string(0, 0, "junk", Default::default());
    ///     })
    ///     .unwrap();
    /// assert_eq!(outcome, DrawOutcome::Cancelled);
    /// assert_eq!(terminal.backend().buffer().to_string(), "hi        \n          ");
    /// ```
    pub fn draw_cancellable(
        &mut self,
        token: &CancellationToken,
        render: impl FnOnce(&mut Frame<'_>),
    ) -> Result<DrawOutcome, B::Error> {
        self.draw_inner(render, Some(token))
    }

    /// The shared body of [`Terminal::draw`] and
    /// [`Terminal::draw_cancellable`]. `token` is `None` for the classic
    /// path: its checkpoints never fire and the outcome is always
    /// committed.
    fn draw_inner(
        &mut self,
        render: impl FnOnce(&mut Frame<'_>),
        token: Option<&CancellationToken>,
    ) -> Result<DrawOutcome, B::Error> {
        // Stage boundary 1: before the pass. An already-cancelled token
        // costs the frame nothing at all — not even the resize probe.
        if token.is_some_and(CancellationToken::is_cancelled) {
            return Ok(DrawOutcome::Cancelled);
        }

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
                cancellation: token.cloned(),
            };
            render(&mut frame);
            frame.cursor_position
        };

        // Stage boundary 2: after the paint closure, before anything is
        // emitted. Abandoned work is discarded, not committed.
        if token.is_some_and(CancellationToken::is_cancelled) {
            self.discard_painted_frame(paint_index);
            return Ok(DrawOutcome::Cancelled);
        }

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
        Ok(DrawOutcome::Committed)
    }

    /// Rolls a partially painted frame back: persistent mode restores the
    /// DOM's journaled bands from the reference (the whole buffer when the
    /// journal flipped full), immediate mode resets the paint target to
    /// the blank canvas the next frame expects. Either way the terminal
    /// keeps showing exactly what it showed before.
    fn discard_painted_frame(&mut self, paint_index: usize) {
        if self.persistent {
            // Persistent layout is fixed: the DOM is `buffers[0]` and the
            // reference is `buffers[1]`, so array destructuring gives the
            // borrow checker two provably disjoint halves.
            let [dom, reference] = &mut self.buffers;
            let full = dom.journal_is_full();
            if full {
                dom.content.clone_from(&reference.content);
            } else {
                // Bands are `Copy` and few; copy them out first so the copy
                // loop does not hold a borrow across the two halves.
                let bands: Vec<crate::damage::Band> = dom.journal_bands().to_vec();
                let width = dom.area.width as usize;
                for band in bands {
                    let start = band.row as usize * width + band.start as usize;
                    let end = band.row as usize * width + band.end as usize;
                    dom.content[start..end].clone_from_slice(&reference.content[start..end]);
                }
            }
            dom.clear_journal();
            reference.clear_journal();
        } else {
            // Immediate mode: the paint target must be a blank canvas again.
            // `reset` also flips the journal to full, matching what a
            // committed frame leaves behind for the next paint.
            self.buffers[paint_index].reset();
        }
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
        self.draw_persistent_inner(render, None).map(|outcome| {
            debug_assert!(outcome.is_committed());
        })
    }

    /// The journal path with the M8 cancellation contract.
    ///
    /// The same two stage boundaries as [`Terminal::draw_cancellable`], and
    /// one extra guarantee on the discard path: because the paint buffer is
    /// the persistent DOM, an abandoned frame must not leak its partial
    /// paint into the next frame. The journaled bands are restored from the
    /// reference buffer (which models the screen), so the DOM is exactly
    /// what it was before the abandoned closure ran.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuvix_core::backend::TestBackend;
    /// use tuvix_core::cancellation::CancellationToken;
    /// use tuvix_core::geometry::Size;
    /// use tuvix_core::terminal::{DrawOutcome, Terminal};
    ///
    /// let mut terminal = Terminal::new_persistent(TestBackend::new(Size::new(10, 2))).unwrap();
    /// terminal.draw_persistent(|frame| {
    ///     frame.buffer.set_string(0, 0, "hello", Default::default());
    /// }).unwrap();
    ///
    /// // The closure paints, then abandons the pass between widgets.
    /// let token = CancellationToken::new();
    /// let outcome = terminal
    ///     .draw_persistent_cancellable(&token, |frame| {
    ///         frame.buffer.set_string(0, 0, "junk", Default::default());
    ///         if let Some(pass) = frame.cancellation() {
    ///             pass.cancel();
    ///         }
    ///     })
    ///     .unwrap();
    /// assert!(outcome.is_cancelled());
    /// // The DOM survived: the next committed frame repaints over "hello".
    /// terminal.draw_persistent(|frame| {
    ///     frame.buffer.set_string(0, 0, "HELLO", Default::default());
    /// }).unwrap();
    /// assert!(terminal
    ///     .backend()
    ///     .buffer()
    ///     .to_string()
    ///     .starts_with("HELLO"));
    /// ```
    pub fn draw_persistent_cancellable(
        &mut self,
        token: &CancellationToken,
        render: impl FnOnce(&mut Frame<'_>),
    ) -> Result<DrawOutcome, B::Error> {
        self.draw_persistent_inner(render, Some(token))
    }

    /// The shared body of [`Terminal::draw_persistent`] and
    /// [`Terminal::draw_persistent_cancellable`].
    fn draw_persistent_inner(
        &mut self,
        render: impl FnOnce(&mut Frame<'_>),
        token: Option<&CancellationToken>,
    ) -> Result<DrawOutcome, B::Error> {
        // Stage boundary 1: before the pass — zero terminal I/O.
        if token.is_some_and(CancellationToken::is_cancelled) {
            return Ok(DrawOutcome::Cancelled);
        }

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
                cancellation: token.cloned(),
            };
            render(&mut frame);
            frame.cursor_position
        };

        // Stage boundary 2: after the paint closure. The DOM is the paint
        // buffer here, so the discard must undo the partial paint.
        if token.is_some_and(CancellationToken::is_cancelled) {
            self.discard_painted_frame(paint_index);
            return Ok(DrawOutcome::Cancelled);
        }

        // The O(1) no-op decision: nothing painted this frame. Roles stay
        // stable — the reference must keep modeling what the backend shows,
        // and the (blank or unchanged) paint target is exactly what the next
        // frame paints over.
        if self.buffers[paint_index].journal_is_empty() {
            self.apply_cursor(cursor_position)?;
            self.buffers[paint_index].clear_journal();
            self.backend.flush()?;
            self.emit_preview();
            return Ok(DrawOutcome::Committed);
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
        Ok(DrawOutcome::Committed)
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

    mod cancellable {
        use super::*;
        use crate::cancellation::CancellationToken;

        fn paint(frame: &mut Frame<'_>, text: &str) {
            frame.buffer.set_string(0, 0, text, Style::new());
        }

        #[test]
        fn classic_frames_carry_no_cancellation() {
            let mut terminal = terminal_10x3();
            let mut observed = (None, true);
            terminal
                .draw(|frame| {
                    observed = (frame.cancellation().map(|_| ()), frame.is_cancelled());
                })
                .unwrap();
            assert_eq!(observed, (None, false));
        }

        #[test]
        fn committed_cancellable_frame_matches_draw_byte_for_byte() {
            let mut classic = terminal_10x3();
            classic.draw(|frame| paint(frame, "hello")).unwrap();
            classic.draw(|frame| paint(frame, "HELL")).unwrap();

            let mut cancellable = terminal_10x3();
            let token = CancellationToken::new();
            let pass = token.child();
            let first = cancellable
                .draw_cancellable(&pass, |frame| paint(frame, "hello"))
                .unwrap();
            let second = cancellable
                .draw_cancellable(&pass, |frame| paint(frame, "HELL"))
                .unwrap();

            assert_eq!(first, DrawOutcome::Committed);
            assert_eq!(second, DrawOutcome::Committed);
            assert_eq!(
                classic.backend().buffer().to_string(),
                cancellable.backend().buffer().to_string()
            );
        }

        #[test]
        fn pre_cancelled_frame_performs_no_terminal_output() {
            let mut terminal = terminal_10x3();
            terminal.draw(|frame| paint(frame, "stable")).unwrap();
            let before = terminal.backend().buffer().to_string();

            let token = CancellationToken::new();
            token.cancel();
            let outcome = terminal
                .draw_cancellable(&token, |frame| paint(frame, "junk"))
                .unwrap();
            assert_eq!(outcome, DrawOutcome::Cancelled);
            assert_eq!(terminal.backend().buffer().to_string(), before);
        }

        #[test]
        fn pre_cancelled_persistent_frame_performs_no_terminal_output() {
            let mut terminal =
                Terminal::new_persistent(TestBackend::new(Size::new(10, 3))).unwrap();
            terminal
                .draw_persistent(|frame| paint(frame, "stable"))
                .unwrap();
            let before = terminal.backend().buffer().to_string();

            let token = CancellationToken::new();
            token.cancel();
            let outcome = terminal
                .draw_persistent_cancellable(&token, |frame| paint(frame, "junk"))
                .unwrap();
            assert_eq!(outcome, DrawOutcome::Cancelled);
            assert_eq!(terminal.backend().buffer().to_string(), before);
        }

        #[test]
        fn mid_paint_abandon_discards_the_frame_and_repairs_the_paint_target() {
            let mut terminal = terminal_10x3();
            let token = CancellationToken::new();
            let pass = token.child();

            terminal
                .draw_cancellable(&pass, |frame| paint(frame, "hello"))
                .unwrap();
            let screen = terminal.backend().buffer().to_string();

            // Paint over two rows, then abandon between "widgets".
            let outcome = terminal
                .draw_cancellable(&pass, |frame| {
                    paint(frame, "junk");
                    frame.buffer.set_string(0, 2, "more junk", Style::new());
                    if let Some(stage) = frame.cancellation() {
                        stage.cancel();
                    }
                })
                .unwrap();
            assert_eq!(outcome, DrawOutcome::Cancelled);
            assert_eq!(terminal.backend().buffer().to_string(), screen);

            // The next committed frame must see a clean paint target: the
            // abandoned paint may not leak into the diff or the canvas. A
            // fresh token models the next render pass (the abandoned one
            // stays cancelled — cancellation is monotonic).
            let next = CancellationToken::new();
            terminal
                .draw_cancellable(&next, |frame| paint(frame, "HELLO"))
                .unwrap();
            assert_eq!(
                terminal.backend().buffer().to_string(),
                "HELLO     \n          \n          "
            );
        }

        #[test]
        fn persistent_abandoned_frame_restores_the_dom() {
            let mut terminal =
                Terminal::new_persistent(TestBackend::new(Size::new(10, 3))).unwrap();
            let token = CancellationToken::new();
            let pass = token.child();

            terminal
                .draw_persistent_cancellable(&pass, |frame| paint(frame, "hello"))
                .unwrap();
            let screen = terminal.backend().buffer().to_string();

            // Paint several bands over the DOM, then abandon the pass.
            let outcome = terminal
                .draw_persistent_cancellable(&pass, |frame| {
                    paint(frame, "junk");
                    frame.buffer.set_string(0, 2, "more junk", Style::new());
                    if let Some(stage) = frame.cancellation() {
                        stage.cancel();
                    }
                })
                .unwrap();
            assert_eq!(outcome, DrawOutcome::Cancelled);
            assert_eq!(terminal.backend().buffer().to_string(), screen);

            // The DOM is exactly what it was: a no-op frame stays a no-op
            // (the journal was cleared by the discard), and repainting the
            // original content changes nothing on the wire.
            terminal.draw_persistent(|_| {}).unwrap();
            assert_eq!(terminal.backend().buffer().to_string(), screen);
            terminal
                .draw_persistent(|frame| paint(frame, "hello"))
                .unwrap();
            assert_eq!(terminal.backend().buffer().to_string(), screen);
        }

        #[test]
        fn persistent_abandoned_full_journal_frame_restores_everything() {
            let mut terminal =
                Terminal::new_persistent(TestBackend::new(Size::new(10, 3))).unwrap();
            let token = CancellationToken::new();
            let pass = token.child();

            terminal
                .draw_persistent_cancellable(&pass, |frame| paint(frame, "hello"))
                .unwrap();
            let screen = terminal.backend().buffer().to_string();

            // Paint the whole viewport so the journal flips to full; the
            // discard must then restore the entire buffer, band by band
            // would miss nothing but the code takes the whole-buffer path.
            let outcome = terminal
                .draw_persistent_cancellable(&pass, |frame| {
                    for row in 0..3 {
                        frame.buffer.set_string(0, row, "0123456789", Style::new());
                    }
                    if let Some(stage) = frame.cancellation() {
                        stage.cancel();
                    }
                })
                .unwrap();
            assert_eq!(outcome, DrawOutcome::Cancelled);
            assert_eq!(terminal.backend().buffer().to_string(), screen);

            let next = CancellationToken::new();
            terminal
                .draw_persistent_cancellable(&next, |frame| paint(frame, "hello"))
                .unwrap();
            assert_eq!(terminal.backend().buffer().to_string(), screen);
        }

        #[test]
        fn persistent_abandoned_frame_repairs_wide_pairs() {
            let mut terminal =
                Terminal::new_persistent(TestBackend::new(Size::new(10, 3))).unwrap();
            let token = CancellationToken::new();
            let pass = token.child();

            terminal
                .draw_persistent_cancellable(&pass, |frame| {
                    frame.buffer.set_string(0, 0, "汉a", Style::new());
                })
                .unwrap();
            let screen = terminal.backend().buffer().to_string();

            // "汉" covers two cells (the second carries the skip flag); the
            // band restore must bring both cells back, not orphan the pair.
            let outcome = terminal
                .draw_persistent_cancellable(&pass, |frame| {
                    frame.buffer.set_string(0, 0, "汉汉", Style::new());
                    if let Some(stage) = frame.cancellation() {
                        stage.cancel();
                    }
                })
                .unwrap();
            assert_eq!(outcome, DrawOutcome::Cancelled);
            assert_eq!(terminal.backend().buffer().to_string(), screen);

            terminal
                .draw_persistent(|frame| paint(frame, "汉a"))
                .unwrap();
            assert_eq!(terminal.backend().buffer().to_string(), screen);
        }

        #[test]
        fn outcome_accessors_agree_with_the_variants() {
            assert!(DrawOutcome::Committed.is_committed());
            assert!(!DrawOutcome::Committed.is_cancelled());
            assert!(DrawOutcome::Cancelled.is_cancelled());
            assert!(!DrawOutcome::Cancelled.is_committed());
        }
    }
}
