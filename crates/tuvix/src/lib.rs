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
//! See `docs/ARCHITECTURE.md` in the repository for the full design and
//! `examples/hello.rs` / `examples/async_events.rs` for runnable apps.

pub use tuvix_backend_ansi as backend_ansi;
pub use tuvix_backend_ansi::AnsiBackend;
pub use tuvix_backend_crossterm as backend_crossterm;
pub use tuvix_backend_crossterm::{CrosstermBackend, CrosstermEvents, to_tuvix_event};
pub use tuvix_backend_termion as backend_termion;
#[cfg(unix)]
pub use tuvix_backend_termion::{TermionBackend, TermionEvents};
pub use tuvix_core::layout::Flex;
pub use tuvix_core::markup::{MarkupError, parse_line, parse_spans, parse_text};
pub use tuvix_core::{line, span, text};
// `text` is imported above with the `line`/`span` macros: one `use`
// brings the module and the macro namespaces in together.
pub use tuvix_core::{
    backend, buffer, cancellation, damage, event, geometry, journal, layout, preview, scheduler,
    style, stylize, terminal, theme, widget,
};
pub use tuvix_widgets::{
    Axis, Bar, BarChart, BarGroup, Block, BorderType, Borders, Canvas, Chart, Clear, Dataset,
    Gauge, GraphType, Input, InputState, LineGauge, List, ListItem, ListState, Marker, Paragraph,
    ScrollState, Scrollbar, ScrollbarOrientation, ScrollbarState, Sparkline, Table, TableCell,
    TableRow, TableState, Tabs, Tree, TreeItem, TreeState, Wrap, wrap_line,
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
    pub use tuvix_core::backend::{Backend, ClearRegion, TestBackend};
    pub use tuvix_core::buffer::{Buffer, Cell, DiffRun, Symbol};
    pub use tuvix_core::cancellation::{CancellationToken, Cancelled};
    pub use tuvix_core::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    pub use tuvix_core::geometry::{Position, Rect, Size};
    pub use tuvix_core::layout::{Constraint, Flex, Layout};
    pub use tuvix_core::markup::{MarkupError, parse_line, parse_spans, parse_text};
    pub use tuvix_core::preview::HistorySink;
    pub use tuvix_core::runtime::input::{InputMode, InputStack, KittyEnhancementFlags};
    pub use tuvix_core::runtime::panic::RestoreSpec;
    pub use tuvix_core::runtime::queue::{EventSender, EventStream, event_channel};
    pub use tuvix_core::runtime::runner::AppRunner;
    pub use tuvix_core::scheduler::{Scheduler, TickOutcome};
    pub use tuvix_core::style::{Color, Hyperlink, Modifier, Style};
    pub use tuvix_core::stylize::Stylize;
    pub use tuvix_core::terminal::{DrawOutcome, Frame, Terminal};
    pub use tuvix_core::text::{Alignment, Line, Span, Text};
    pub use tuvix_core::theme::Theme;
    pub use tuvix_core::widget::{RenderWidget, StatefulWidget, Widget};
    pub use tuvix_core::{line, span, text};
    pub use tuvix_widgets::{
        Axis, Bar, BarChart, BarGroup, Block, BorderType, Borders, Canvas, Chart, Clear, Dataset,
        Gauge, GraphType, Input, InputState, LineGauge, List, ListItem, ListState, Marker,
        Paragraph, ScrollState, Scrollbar, ScrollbarOrientation, ScrollbarState, Sparkline, Table,
        TableCell, TableRow, TableState, Tabs, Tree, TreeItem, TreeState, Wrap, wrap_line,
    };
}
