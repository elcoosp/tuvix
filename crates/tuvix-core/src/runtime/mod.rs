//! The M2 runtime: input-mode stack, async-ready event stream, panic
//! restoration and the application runner.
//!
//! Everything here is std-only and backend-generic: the runner is exercised
//! against [`TestBackend`](crate::backend::TestBackend) in tests, and the
//! crossterm backend supplies the real terminal effects.

pub mod input;
pub mod panic;
pub mod queue;
pub mod runner;

pub use input::{InputMode, InputStack, InputTransition, KittyEnhancementFlags};
pub use panic::{RestoreSpec, install_panic_hook};
pub use queue::{EventSender, EventStream, event_channel};
pub use runner::AppRunner;
