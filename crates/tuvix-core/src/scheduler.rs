//! The cooperative frame scheduler: input preempts render.
//!
//! This is the M8 slice that turns the M8-B cancellation contract into the
//! architecture's differentiator (§5.7): a single-threaded event loop
//! where a keystroke is never queued behind a stale frame. The
//! [`Scheduler`] wraps a [`Terminal`] and adds three behaviors:
//!
//! - **render coalescing** — `request_render` marks the frame dirty; any
//!   number of requests collapse into *at most one* frame per
//!   [`tick`](Scheduler::tick). State churn between ticks costs nothing
//!   on the terminal.
//! - **input priority** — when a render is pending and the probe says an
//!   input event is waiting, the tick yields instead of rendering
//!   ([`TickOutcome::Yielded`]); the app drains its event queue first and
//!   the render request survives. The probe is only consulted when there
//!   is work to preempt.
//! - **cancellable frames** — [`tick_cancellable`](Scheduler::tick_cancellable)
//!   routes the pass through
//!   [`Terminal::draw_cancellable`](crate::terminal::Terminal::draw_cancellable),
//!   so a preempted render is discarded, not committed.
//!
//! The idle tick (no request pending) is an O(1) decision — no probe
//! call, no diff walk, no backend I/O — which is the static-content
//! budget of the architecture realized as a type.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::backend::TestBackend;
//! use tuvix_core::geometry::Size;
//! use tuvix_core::scheduler::{Scheduler, TickOutcome};
//! use tuvix_core::terminal::Terminal;
//!
//! let mut scheduler = Scheduler::new(Terminal::new(TestBackend::new(Size::new(10, 2))).unwrap());
//!
//! // Nothing requested: the tick is the O(1) idle decision.
//! assert_eq!(scheduler.tick(|_| {}).unwrap(), TickOutcome::Idle);
//!
//! // Five state changes collapse into exactly one frame.
//! for _ in 0..5 {
//!     scheduler.request_render();
//! }
//! assert!(scheduler.is_render_pending());
//! let outcome = scheduler
//!     .tick(|frame| {
//!         frame.buffer.set_string(0, 0, "hi", Default::default());
//!     })
//!     .unwrap();
//! assert_eq!(outcome, TickOutcome::Rendered);
//! assert_eq!(scheduler.frames(), 1);
//! // The next tick has nothing to do, no matter how often the app polls.
//! assert_eq!(scheduler.tick(|_| {}).unwrap(), TickOutcome::Idle);
//! ```

use crate::backend::Backend;
use crate::cancellation::CancellationToken;
use crate::terminal::Terminal;

/// What one scheduler tick decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickOutcome {
    /// Nothing was pending: the tick was the O(1) idle check. No probe
    /// call, no diff walk, no terminal I/O.
    Idle,
    /// Input is waiting: the pending render was *not* started so the app
    /// can drain its event queue first. The render request stays pending.
    Yielded,
    /// The pending render request was satisfied by exactly one frame.
    Rendered,
    /// A render was requested but the pass token was already cancelled:
    /// the frame was abandoned before any I/O and the request is consumed.
    Cancelled,
}

impl TickOutcome {
    /// Whether the tick produced a committed frame.
    pub fn is_rendered(self) -> bool {
        matches!(self, Self::Rendered)
    }
}

/// The non-blocking "is input waiting?" probe.
///
/// Any cheap, allocation-free closure works — for tuvix's own event
/// stream, `|| !stream.is_empty()` (the probe must never consume the
/// event, only report that one exists).
type Probe = Box<dyn Fn() -> bool + 'static>;

/// Input-priority, coalescing frame scheduler over a [`Terminal`].
///
/// See the [module docs](self) for the model; the intended loop shape is:
///
/// ```text
/// loop {
///     match scheduler.tick(render)? {
///         TickOutcome::Idle | TickOutcome::Rendered => { /* wait for input */ }
///         TickOutcome::Yielded => { drain event queue; handle input; }
///         TickOutcome::Cancelled => { /* input preempted the pass */ }
///     }
/// }
/// ```
pub struct Scheduler<B: Backend> {
    terminal: Terminal<B>,
    probe: Option<Probe>,
    render_pending: bool,
    frames: u64,
    yields: u64,
    cancellations: u64,
}

impl<B: Backend> core::fmt::Debug for Scheduler<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Scheduler")
            .field("render_pending", &self.render_pending)
            .field("frames", &self.frames)
            .field("yields", &self.yields)
            .field("cancellations", &self.cancellations)
            .finish_non_exhaustive()
    }
}

impl<B: Backend> Scheduler<B> {
    /// Wraps `terminal` in a scheduler with no probe and no pending work.
    pub fn new(terminal: Terminal<B>) -> Self {
        Self {
            terminal,
            probe: None,
            render_pending: false,
            frames: 0,
            yields: 0,
            cancellations: 0,
        }
    }

    /// Shared access to the wrapped terminal.
    pub fn terminal(&self) -> &Terminal<B> {
        &self.terminal
    }

    /// Exclusive access to the wrapped terminal (resize handling, direct
    /// draws outside the scheduler's accounting).
    pub fn terminal_mut(&mut self) -> &mut Terminal<B> {
        &mut self.terminal
    }

    /// Unwraps the scheduler, returning the terminal.
    pub fn into_inner(self) -> Terminal<B> {
        self.terminal
    }

    /// Installs the input probe consulted at preemptable ticks.
    ///
    /// The probe must be non-blocking and must not consume the event it
    /// reports; draining belongs to the app after a
    /// [`TickOutcome::Yielded`].
    pub fn set_probe(&mut self, probe: impl Fn() -> bool + 'static) {
        self.probe = Some(Box::new(probe));
    }

    /// Removes the probe: ticks can no longer be preempted.
    pub fn clear_probe(&mut self) {
        self.probe = None;
    }

    /// Marks a frame as wanted. Idempotent: repeated calls before the next
    /// tick coalesce into that one frame.
    pub fn request_render(&mut self) {
        self.render_pending = true;
    }

    /// Whether a render request is waiting for a tick.
    pub fn is_render_pending(&self) -> bool {
        self.render_pending
    }

    /// Frames committed so far (one request batch = one frame).
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Ticks that yielded to pending input so far.
    pub fn yields(&self) -> u64 {
        self.yields
    }

    /// Ticks abandoned by an already-cancelled pass token so far.
    pub fn cancellations(&self) -> u64 {
        self.cancellations
    }

    /// Runs one scheduler step with `render` as the frame closure.
    ///
    /// The decision order is the architecture's: input first, then the
    /// idle check, then exactly one frame.
    pub fn tick(
        &mut self,
        render: impl FnOnce(&mut crate::terminal::Frame<'_>),
    ) -> Result<TickOutcome, B::Error> {
        self.tick_inner(None, render)
    }

    /// Runs one scheduler step whose frame is cancellable: when `token`
    /// was cancelled (by input, on another thread) before the pass, the
    /// frame is abandoned with zero terminal I/O.
    pub fn tick_cancellable(
        &mut self,
        token: &CancellationToken,
        render: impl FnOnce(&mut crate::terminal::Frame<'_>),
    ) -> Result<TickOutcome, B::Error> {
        self.tick_inner(Some(token), render)
    }

    /// The shared tick body: input priority, idle decision, one frame.
    fn tick_inner(
        &mut self,
        token: Option<&CancellationToken>,
        render: impl FnOnce(&mut crate::terminal::Frame<'_>),
    ) -> Result<TickOutcome, B::Error> {
        if !self.render_pending {
            // Idle: nothing to schedule and nothing to preempt — the probe
            // is not even consulted.
            return Ok(TickOutcome::Idle);
        }

        // Input has absolute priority over a pending render.
        if self.probe.as_ref().is_some_and(|probe| probe()) {
            self.yields += 1;
            return Ok(TickOutcome::Yielded);
        }

        self.render_pending = false;
        let Some(token) = token else {
            self.terminal.draw(render)?;
            self.frames += 1;
            return Ok(TickOutcome::Rendered);
        };
        if self
            .terminal
            .draw_cancellable(token, render)?
            .is_committed()
        {
            self.frames += 1;
            Ok(TickOutcome::Rendered)
        } else {
            self.cancellations += 1;
            Ok(TickOutcome::Cancelled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::TestBackend;
    use crate::geometry::Size;
    use crate::runtime::queue::event_channel;
    use crate::style::Style;
    use std::cell::Cell;
    use std::rc::Rc;

    fn scheduler() -> Scheduler<TestBackend> {
        Scheduler::new(Terminal::new(TestBackend::new(Size::new(10, 2))).unwrap())
    }

    #[test]
    fn idle_tick_without_a_request_does_nothing() {
        let mut scheduler = scheduler();
        assert_eq!(scheduler.tick(|_| {}).unwrap(), TickOutcome::Idle);
        assert_eq!(scheduler.frames(), 0);
        // The terminal was never touched: the backend is blank and the
        // cursor was never hidden (that only happens on a draw).
        assert_eq!(
            scheduler.terminal().backend().buffer().to_string(),
            "          \n          "
        );
    }

    #[test]
    fn a_request_renders_exactly_one_frame() {
        let mut scheduler = scheduler();
        scheduler.request_render();
        let outcome = scheduler
            .tick(|frame| {
                frame.buffer.set_string(0, 0, "hi", Style::new());
            })
            .unwrap();
        assert_eq!(outcome, TickOutcome::Rendered);
        assert_eq!(
            scheduler.terminal().backend().buffer().to_string(),
            "hi        \n          "
        );
        assert_eq!(scheduler.frames(), 1);
        assert!(!scheduler.is_render_pending());
    }

    #[test]
    fn repeated_requests_coalesce_into_one_frame() {
        let mut scheduler = scheduler();
        for _ in 0..5 {
            scheduler.request_render();
        }
        assert!(scheduler.is_render_pending());
        scheduler
            .tick(|frame| {
                frame.buffer.set_string(0, 0, "once", Style::new());
            })
            .unwrap();
        // The follow-up ticks have nothing left to do.
        assert_eq!(scheduler.tick(|_| {}).unwrap(), TickOutcome::Idle);
        assert_eq!(scheduler.tick(|_| {}).unwrap(), TickOutcome::Idle);
        assert_eq!(scheduler.frames(), 1);
    }

    #[test]
    fn input_preempts_a_pending_render() {
        let mut scheduler = scheduler();
        scheduler.set_probe(|| true);
        scheduler.request_render();
        assert_eq!(
            scheduler
                .tick(|frame| {
                    frame.buffer.set_string(0, 0, "late", Style::new());
                })
                .unwrap(),
            TickOutcome::Yielded
        );
        assert_eq!(scheduler.yields(), 1);
        // The render request survives the preemption...
        assert!(scheduler.is_render_pending());
        // ...and the terminal was not touched.
        assert_eq!(
            scheduler.terminal().backend().buffer().to_string(),
            "          \n          "
        );

        // Input drained: the next tick renders.
        scheduler.clear_probe();
        scheduler
            .tick(|frame| {
                frame.buffer.set_string(0, 0, "late", Style::new());
            })
            .unwrap();
        assert_eq!(scheduler.frames(), 1);
        assert_eq!(
            scheduler.terminal().backend().buffer().to_string(),
            "late      \n          "
        );
    }

    #[test]
    fn the_probe_is_not_consulted_without_pending_work() {
        let calls = Rc::new(Cell::new(0u32));
        let mut scheduler = scheduler();
        {
            let calls = Rc::clone(&calls);
            scheduler.set_probe(move || {
                calls.set(calls.get() + 1);
                true
            });
        }
        for _ in 0..3 {
            assert_eq!(scheduler.tick(|_| {}).unwrap(), TickOutcome::Idle);
        }
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn tick_cancellable_routes_through_the_cancellable_path() {
        let mut scheduler = scheduler();
        let token = CancellationToken::new();
        let pass = token.child();

        scheduler.request_render();
        let outcome = scheduler
            .tick_cancellable(&pass, |frame| {
                frame.buffer.set_string(0, 0, "kept", Style::new());
            })
            .unwrap();
        assert_eq!(outcome, TickOutcome::Rendered);
        assert_eq!(scheduler.frames(), 1);

        token.cancel();
        scheduler.request_render();
        let outcome = scheduler
            .tick_cancellable(&pass, |frame| {
                frame.buffer.set_string(0, 0, "dropped", Style::new());
            })
            .unwrap();
        assert_eq!(outcome, TickOutcome::Cancelled);
        assert_eq!(scheduler.cancellations(), 1);
        assert_eq!(scheduler.frames(), 1);
        assert_eq!(
            scheduler.terminal().backend().buffer().to_string(),
            "kept      \n          "
        );
        // The request was consumed by the abandoned pass.
        assert!(!scheduler.is_render_pending());
        assert_eq!(scheduler.tick(|_| {}).unwrap(), TickOutcome::Idle);
    }

    #[test]
    fn the_event_stream_probe_shapes_the_priority_loop() {
        let (sender, stream) = event_channel();
        let mut scheduler = scheduler();
        scheduler.set_probe(move || !stream.is_empty());

        // Input arrives while a render is pending: yield first.
        scheduler.request_render();
        assert!(sender.send(crate::event::Event::FocusGained));
        assert_eq!(scheduler.tick(|_| {}).unwrap(), TickOutcome::Yielded);

        // The app drains the queue; the probe goes quiet.
        scheduler.clear_probe();
        scheduler.set_probe(|| false);
        let outcome = scheduler
            .tick(|frame| {
                frame.buffer.set_string(0, 0, "after input", Style::new());
            })
            .unwrap();
        assert_eq!(outcome, TickOutcome::Rendered);
    }

    #[test]
    fn terminal_access_survives_the_scheduler() {
        let mut scheduler = scheduler();
        scheduler.request_render();
        scheduler
            .tick(|frame| {
                frame.buffer.set_string(0, 0, "kept", Style::new());
            })
            .unwrap();
        let terminal = scheduler.into_inner();
        assert_eq!(
            terminal.backend().buffer().to_string(),
            "kept      \n          "
        );
    }

    #[test]
    fn debug_mentions_the_accounting() {
        let scheduler = scheduler();
        let rendered = format!("{scheduler:?}");
        assert!(rendered.contains("Scheduler"));
        assert!(rendered.contains("frames"));
    }
}
