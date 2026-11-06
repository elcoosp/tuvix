//! A high-performance terminal UI framework with best-in-class developer
//! experience.
//!
//! tuvix renders through double-buffered cell grids and a zero-allocation,
//! run-batched diff engine: every frame repaints the whole screen logically,
//! and only the changed cells reach the terminal.
//!
//! Since M2 the lifecycle is one call: [`runtime::try_init`] returns a
//! [`runtime::DefaultRunner`] with raw mode, the alternate screen and a
//! panic hook installed; the runner restores the terminal on drop, on
//! error and on panic. Events flow through an executor-agnostic async
//! stream (`tuvix_core::runtime::queue`) fed by the crossterm reader
//! ([`CrosstermEvents`]).
//!
//! # Quick start
//!
//! ```
//! use tuvix::prelude::*;
//!
//! let mut terminal = Terminal::new(TestBackend::new(Size::new(20, 3))).unwrap();
//! terminal.draw(|frame| {
//!     frame.buffer.set_string(0, 0, "hello".red().bold(), Style::new());
//! }).unwrap();
//! ```
//!
//! # Runtime quick start
//!
//! ```no_run
//! # use std::io::Result;
//! use tuvix::prelude::*;
//! use tuvix_backend_crossterm::CrosstermEvents;
//!
//! # fn main() -> Result<()> {
//! let mut runner = tuvix::try_init()?;
//! runner.push_input_mode(tuvix::runtime::InputMode::BracketedPaste)?;
//! let (_reader, mut events) = CrosstermEvents::spawn()?;
//!
//! while let Some(event) = events.try_next() {
//!     std::hint::black_box(event);
//! }
//!
//! runner.restore()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Responsiveness: input preempts render (M8)
//!
//! The scheduler is the architecture's differentiator as a type: renders
//! are requested (and coalesced), input is probed without consuming it,
//! and a preempted pass is discarded — never committed:
//!
//! ```
//! use tuvix::prelude::*;
//!
//! let mut scheduler = Scheduler::new(Terminal::new(TestBackend::new(Size::new(20, 3))).unwrap());
//! let input_waiting = std::rc::Rc::new(std::cell::Cell::new(false));
//! scheduler.set_probe({
//!     let input_waiting = std::rc::Rc::clone(&input_waiting);
//!     move || input_waiting.get()
//! });
//!
//! // Five state changes collapse into one frame...
//! for _ in 0..5 {
//!     scheduler.request_render();
//! }
//! // ...but input wins the tick:
//! input_waiting.set(true);
//! assert_eq!(
//!     scheduler.tick(|frame| {
//!         frame.buffer.set_string(0, 0, "stale work", Style::new());
//!     }).unwrap(),
//!     TickOutcome::Yielded
//! );
//!
//! // Input drained: the fresh frame renders, cancellable if needed.
//! input_waiting.set(false);
//! let token = CancellationToken::new();
//! let outcome = scheduler.tick_cancellable(&token, |frame| {
//!     frame.buffer.set_string(0, 0, "fresh", Style::new());
//! }).unwrap();
//! assert_eq!(outcome, TickOutcome::Rendered);
//! ```
//!
//! Time travel comes through the preview hook:
//! [`HistorySink`](preview::HistorySink) records every flushed screen;
//! [`CancellationToken`](cancellation::CancellationToken) /
//! [`DrawOutcome`](terminal::DrawOutcome) round out the primitives.
//!
//! # Time-travel DevTools: record, reconstruct, inspect (M9)
//!
//! The M9 kit turns recordings into first-class objects. A
//! [`SessionRecorder`](session::SessionRecorder) captures a unified
//! timeline of frames *and* input events; [`Screen`](screen::Screen)
//! parses any recorded frame back into cells; [`FrameInspector`]
//! overlays a historical moment on a live app:
//!
//! ```
//! use tuvix::prelude::*;
//!
//! let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 1))).unwrap();
//! let session = SessionRecorder::unbounded();
//! terminal.set_preview(session.clone());
//!
//! for tick in 0..3 {
//!     let label = format!("tick {tick}");
//!     terminal.draw(|frame| {
//!         frame.buffer.set_string(0, 0, label.as_str(), Style::new());
//!     }).unwrap();
//! }
//! session.record_event(Event::FocusGained);
//!
//! // Scrub: any frame reconstructs into a Buffer that diffs and
//! // renders like a live one.
//! let screen = Screen::from_ansi(&session.frame(0).unwrap().ansi).unwrap();
//! assert_eq!(screen.to_buffer(Rect::new(0, 0, 10, 1)).to_string(), "tick 0    ");
//!
//! // Inspect: the historical frame as a picture-in-picture overlay,
//! // with a timeline footer for the scrubber UI.
//! let mut overlay = Buffer::empty(Rect::new(0, 0, 18, 4));
//! FrameInspector::new(screen)
//!     .timeline(0, 3)
//!     .at(session.frame(0).unwrap().at)
//!     .title("scrub")
//!     .render(Rect::new(0, 0, 18, 4), &mut overlay);
//! assert!(overlay.to_string().contains("tick 0"));
//!
//! // Golden sessions: export, replay, diff — an empty diff means the
//! // same screen sequence.
//! let golden = session.to_json();
//! let imported = SessionRecorder::from_json(&golden).unwrap();
//! assert!(session.diff(&imported).is_empty());
//! ```
//!
//! # Motion: time-driven animation (M13)
//!
//! Values travel on the clock, not on the tick counter — the same
//! elapsed time renders identically at 5 fps and 60 fps — and the
//! [`Animator`](anim::Animator) keeps the static-CPU budget honest: when
//! nothing moves,
//! `is_idle()` says so and the app stops scheduling frames:
//!
//! ```
//! use std::time::Duration;
//!
//! use tuvix::prelude::*;
//!
//! let mut animator = Animator::new();
//! animator.start(
//!     "gauge",
//!     Tween::new(0.0_f64, 100.0, Duration::from_secs(1)).easing(Easing::OutCubic),
//! );
//! animator.advance(Duration::from_millis(250));
//! assert!((animator.value(&"gauge").unwrap() - 57.8125).abs() < 1e-9);
//! assert!(!animator.is_idle(), "a gauge is still moving");
//! ```
//!
//! # Mouse depth: hit-testing, gestures, declarative bindings (M14)
//!
//! The mouse gets the same treatment the keyboard got in M11: a
//! [`HitMap`](hit::HitMap) resolves "what did the user click?" the way the frame was
//! painted (topmost registration wins), a
//! [`GestureRecognizer`](gesture::GestureRecognizer) classifies raw events into clicks (with
//! multiplicity), drags (anchored at their origin) and scrolls on an
//! injected clock, and a [`MouseMap`](mousemap::MouseMap) routes the results to commands —
//! later bindings shadowing earlier ones, the painter's algorithm again:
//!
//! ```
//! use std::time::Duration;
//!
//! use tuvix::prelude::*;
//!
//! #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//! enum Command { FocusLog, ScrollLog, Pin }
//!
//! let mut map = MouseMap::new();
//! map.bind(MousePattern::anywhere(MouseClass::Scroll), Command::ScrollLog);
//! map.bind(MousePattern::on("log", MouseClass::Click), Command::FocusLog);
//! map.bind(MousePattern::on("log", MouseClass::DoubleClick), Command::Pin);
//!
//! // Classify the raw event, resolve through the frame's hit map.
//! let mut gestures = GestureRecognizer::new(GestureConfig::default());
//! let down = MouseEvent {
//!     kind: MouseEventKind::Down(MouseButton::Left),
//!     column: 3,
//!     row: 3,
//!     modifiers: KeyModifiers::empty(),
//! };
//! let up = MouseEvent { kind: MouseEventKind::Up(MouseButton::Left), ..down };
//! let _ = gestures.feed(down, Duration::ZERO);
//! let click = gestures.feed(up, Duration::from_millis(40)).unwrap();
//!
//! let mut hits: HitMap<&str> = HitMap::new();
//! hits.push("log", Rect::new(0, 0, 20, 8));
//! let hit = hits.hit_test(anchor(&click));
//! assert_eq!(map.resolve(hit, &click), Some(Command::FocusLog));
//! assert_eq!(map.resolve(None, &click), None);
//! ```
//!
//! See `docs/ARCHITECTURE.md` in the repository for the full design and
//! `examples/hello.rs` / `examples/devtools.rs` for runnable apps.

pub use tuvix_backend_ansi as backend_ansi;
pub use tuvix_backend_ansi::AnsiBackend;
pub use tuvix_backend_crossterm as backend_crossterm;
pub use tuvix_backend_crossterm::{CrosstermBackend, CrosstermEvents, to_tuvix_event};
pub use tuvix_backend_termion as backend_termion;
pub use tuvix_backend_termion::{TermionBackend, TermionEvents};
pub use tuvix_core::layout::Flex;
pub use tuvix_core::markup::{MarkupError, parse_line, parse_spans, parse_text};
pub use tuvix_core::{line, span, text};
pub use tuvix_ffi as ffi;
// `text` is imported above with the `line`/`span` macros: one `use`
// brings the module and the macro namespaces in together.
pub use tuvix_core::{
    anim, backend, buffer, cancellation, clipboard, damage, embed, event, focus, geometry, gesture,
    hit, journal, keymap, layout, markup, mousemap, preview, replay, scheduler, screen, session,
    style, stylize, terminal, theme, widget,
};
pub use tuvix_widgets::{
    Axis, Bar, BarChart, BarGroup, Block, BorderType, Borders, Canvas, Chart, Clear, Dataset,
    FrameInspector, Gauge, GraphType, Input, InputState, LineGauge, List, ListItem, ListState,
    Marker, Paragraph, ScrollState, Scrollable, Scrollbar, ScrollbarOrientation, ScrollbarState,
    Sparkline, Table, TableCell, TableRow, TableState, Tabs, TextArea, TextAreaState, Tree,
    TreeItem, TreeState, ViewportState, Wrap, wrap_line,
};

pub mod runtime;

pub use runtime::{DefaultRunner, init, restore_global, try_init, try_restore_global};
#[cfg(unix)]
pub use runtime::{TermionRunner, init_termion, try_init_termion};

/// The most common tuvix imports in one glob.
pub mod prelude {
    pub use tuvix_backend_ansi::AnsiBackend;
    pub use tuvix_backend_crossterm::CrosstermBackend;
    #[cfg(unix)]
    pub use tuvix_backend_termion::TermionBackend;
    pub use tuvix_core::anim::{Animator, Easing, Interpolate, Progress, Repeat, Tween};
    pub use tuvix_core::backend::{Backend, ClearRegion, TestBackend};
    pub use tuvix_core::buffer::{Buffer, Cell, DiffRun, Symbol};
    pub use tuvix_core::cancellation::{CancellationToken, Cancelled};
    pub use tuvix_core::clipboard::{
        ClipboardError, ClipboardSelection, OSC52_MAX_PAYLOAD, encode_osc52,
    };
    pub use tuvix_core::embed::{
        Mutation, MutationReader, PacketError, PacketErrorKind, PacketWriter, decode_snapshot,
        encode_snapshot, parse_packets,
    };
    pub use tuvix_core::event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    };
    pub use tuvix_core::focus::FocusRing;
    pub use tuvix_core::geometry::{Position, Rect, Size};
    pub use tuvix_core::gesture::{Gesture, GestureConfig, GestureRecognizer, ScrollKind};
    pub use tuvix_core::hit::{Hit, HitMap};
    pub use tuvix_core::keymap::{KeyPattern, Keymap, KeymapOutcome};
    pub use tuvix_core::layout::{Constraint, Flex, Layout};
    pub use tuvix_core::markup::{MarkupError, parse_line, parse_spans, parse_text};
    pub use tuvix_core::mousemap::{
        ButtonPattern, MouseClass, MouseMap, MousePattern, MouseTarget, anchor,
    };
    pub use tuvix_core::preview::HistorySink;
    pub use tuvix_core::replay::{ReplayMismatch, ReplayReport, ReplayStep, SessionReplay};
    pub use tuvix_core::runtime::input::{InputMode, InputStack, KittyEnhancementFlags};
    pub use tuvix_core::runtime::panic::RestoreSpec;
    pub use tuvix_core::runtime::queue::{EventSender, EventStream, event_channel};
    pub use tuvix_core::runtime::runner::AppRunner;
    pub use tuvix_core::scheduler::{Scheduler, TickOutcome};
    pub use tuvix_core::screen::{ParseError, ParseErrorKind, Screen};
    pub use tuvix_core::session::{
        Entry, FrameRecord, InputRecord, SessionDiff, SessionError, SessionErrorKind,
        SessionRecorder,
    };
    pub use tuvix_core::style::{Color, Hyperlink, Modifier, Style};
    pub use tuvix_core::stylize::Stylize;
    pub use tuvix_core::terminal::{DrawOutcome, Frame, Terminal};
    pub use tuvix_core::text::{Alignment, Line, Span, Text};
    pub use tuvix_core::theme::Theme;
    pub use tuvix_core::widget::{RenderWidget, StatefulWidget, Widget};
    pub use tuvix_core::{line, span, text};
    pub use tuvix_widgets::{
        Axis, Bar, BarChart, BarGroup, Block, BorderType, Borders, Canvas, Chart, Clear, Dataset,
        FrameInspector, Gauge, GraphType, Input, InputState, LineGauge, List, ListItem, ListState,
        Marker, Paragraph, ScrollState, Scrollable, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Sparkline, Table, TableCell, TableRow, TableState, Tabs, TextArea,
        TextAreaState, Tree, TreeItem, TreeState, ViewportState, Wrap, wrap_line,
    };
}
