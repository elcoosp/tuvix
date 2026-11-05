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
pub use tuvix_core::{
    backend, buffer, damage, event, geometry, journal, layout, preview, style, stylize, terminal,
    text, theme, widget,
};
pub use tuvix_widgets::{
    Axis, Block, BorderType, Borders, Canvas, Chart, Clear, Dataset, Gauge, GraphType, Input,
    InputState, LineGauge, List, ListItem, ListState, Marker, Paragraph, ScrollState, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Sparkline, Table, TableCell, TableRow, TableState, Tabs,
    Wrap, wrap_line,
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
    pub use tuvix_core::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    pub use tuvix_core::geometry::{Position, Rect, Size};
    pub use tuvix_core::layout::{Constraint, Flex, Layout};
    pub use tuvix_core::runtime::input::{InputMode, InputStack, KittyEnhancementFlags};
    pub use tuvix_core::runtime::panic::RestoreSpec;
    pub use tuvix_core::runtime::queue::{EventSender, EventStream, event_channel};
    pub use tuvix_core::runtime::runner::AppRunner;
    pub use tuvix_core::style::{Color, Hyperlink, Modifier, Style};
    pub use tuvix_core::stylize::Stylize;
    pub use tuvix_core::terminal::{Frame, Terminal};
    pub use tuvix_core::text::{Alignment, Line, Span, Text};
    pub use tuvix_core::theme::Theme;
    pub use tuvix_core::widget::{RenderWidget, StatefulWidget, Widget};
    pub use tuvix_widgets::{
        Axis, Block, BorderType, Borders, Canvas, Chart, Clear, Dataset, Gauge, GraphType, Input,
        InputState, LineGauge, List, ListItem, ListState, Marker, Paragraph, ScrollState,
        Scrollbar, ScrollbarOrientation, ScrollbarState, Sparkline, Table, TableCell, TableRow,
        TableState, Tabs, Wrap, wrap_line,
    };
}
