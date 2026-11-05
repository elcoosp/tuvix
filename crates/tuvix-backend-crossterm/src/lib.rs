//! Crossterm backend for the tuvix terminal UI framework: ANSI rendering with
//! memoized style state, a lossless input event bridge and the background
//! event reader that feeds the async event stream.

pub mod backend;
pub mod event;
pub mod events;

pub use backend::CrosstermBackend;
pub use event::to_tuvix_event;
pub use events::{CrosstermEvents, CrosstermPoller, EventPoller};
