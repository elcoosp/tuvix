//! Cooperative cancellation for render passes.
//!
//! This is the M8 slice behind the architecture's differentiator: input
//! preempts rendering. A [`CancellationToken`] is the coordination
//! primitive — render passes check it at stage boundaries, and a frame
//! that observes cancellation is **discarded, not committed** (see
//! [`Terminal::draw_cancellable`](crate::terminal::Terminal::draw_cancellable)).
//!
//! # Semantics
//!
//! - Cheap to check: one atomic load per token in the chain, no locks, no
//!   allocation on the check path.
//! - Cancellation is a promise, not a request: once cancelled, a token
//!   stays cancelled. There is no un-cancel.
//! - Parent-child propagation: cancelling a token cancels every
//!   [`child`](CancellationToken::child) derived from it (directly or
//!   transitively); cancelling a child never touches its parent. Chains
//!   are walked on check, so depth costs — keep them shallow (two or
//!   three levels: app → view → widget).
//!
//! # Examples
//!
//! ```
//! use tuvix_core::cancellation::CancellationToken;
//!
//! let token = CancellationToken::new();
//! assert!(!token.is_cancelled());
//!
//! let child = token.child();
//! token.cancel();
//! // Cancelling the parent cancels the child...
//! assert!(child.is_cancelled());
//! // ...and cancellation is monotonic.
//! assert!(token.is_cancelled());
//! ```

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Shared state of one token node: its own flag plus the parent it
/// inherits cancellation from.
#[derive(Debug, Default)]
struct CancelState {
    flag: AtomicBool,
    parent: Option<Arc<CancelState>>,
}

/// A cooperative cancellation signal shared by any number of threads.
///
/// Clone the token to hand the *same* signal to another stage or thread;
/// call [`child`](CancellationToken::child) to derive a token that also
/// cancels when the parent does (useful to hand a widget a token the
/// widget cannot use to cancel its siblings).
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    state: Arc<CancelState>,
}

impl CancellationToken {
    /// Creates a token that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Derives a child token: it cancels when `self` cancels, but
    /// cancelling it does not affect `self` or any sibling.
    pub fn child(&self) -> Self {
        Self {
            state: Arc::new(CancelState {
                flag: AtomicBool::new(false),
                parent: Some(Arc::clone(&self.state)),
            }),
        }
    }

    /// Cancels the token: every future [`check`](Self::check) on this
    /// token and every token derived from it fails. Idempotent.
    pub fn cancel(&self) {
        self.state.flag.store(true, Ordering::Release);
    }

    /// Whether this token — or any ancestor — has been cancelled.
    ///
    /// Cost is one atomic load per chain link; the intended chain depths
    /// are two or three links, so the walk is a handful of nanoseconds.
    pub fn is_cancelled(&self) -> bool {
        let mut current: Option<&Arc<CancelState>> = Some(&self.state);
        while let Some(state) = current {
            if state.flag.load(Ordering::Acquire) {
                return true;
            }
            current = state.parent.as_ref();
        }
        false
    }

    /// The stage-boundary form of [`is_cancelled`](Self::is_cancelled):
    /// `Err(Cancelled)` aborts the pass, `Ok(())` continues it.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

/// The error produced by [`CancellationToken::check`] when the pass is
/// abandoned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("render pass cancelled")
    }
}

impl std::error::Error for Cancelled {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_token_is_not_cancelled() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        assert_eq!(token.check(), Ok(()));
    }

    #[test]
    fn cancel_flips_check_to_cancelled() {
        let token = CancellationToken::new();
        token.cancel();
        assert!(token.is_cancelled());
        assert_eq!(token.check(), Err(Cancelled));
    }

    #[test]
    fn cancel_is_idempotent() {
        let token = CancellationToken::new();
        token.cancel();
        token.cancel();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn clones_share_one_signal() {
        let token = CancellationToken::new();
        let clone = token.clone();
        clone.cancel();
        assert!(token.is_cancelled());
        assert!(clone.is_cancelled());
    }

    #[test]
    fn child_cancels_with_the_parent() {
        let parent = CancellationToken::new();
        let child = parent.child();
        assert!(!child.is_cancelled());
        parent.cancel();
        assert!(child.is_cancelled());
        assert_eq!(child.check(), Err(Cancelled));
    }

    #[test]
    fn child_cancel_does_not_touch_the_parent() {
        let parent = CancellationToken::new();
        let child = parent.child();
        child.cancel();
        assert!(child.is_cancelled());
        assert!(!parent.is_cancelled());
        assert_eq!(parent.check(), Ok(()));
    }

    #[test]
    fn propagation_reaches_grandchildren() {
        let root = CancellationToken::new();
        let mid = root.child();
        let leaf = mid.child();
        assert!(!leaf.is_cancelled());
        root.cancel();
        assert!(mid.is_cancelled());
        assert!(leaf.is_cancelled());
    }

    #[test]
    fn a_cancelled_parent_cannot_be_un_cancelled_by_children() {
        let parent = CancellationToken::new();
        let child = parent.child();
        parent.cancel();
        assert!(child.is_cancelled());
        // There is no un-cancel API; the assert documents the monotonic
        // contract the chain walk relies on.
        assert!(parent.is_cancelled());
    }

    #[test]
    fn siblings_are_independent() {
        let parent = CancellationToken::new();
        let first = parent.child();
        let second = parent.child();
        first.cancel();
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled());
        // The parent is still live, so the sibling chain is intact.
        assert!(!second.is_cancelled());
        parent.cancel();
        assert!(second.is_cancelled());
    }

    #[test]
    fn cancellation_crosses_threads() {
        let token = CancellationToken::new();
        let handle = {
            let token = token.clone();
            std::thread::spawn(move || {
                token.cancel();
            })
        };
        handle.join().unwrap();
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancelled_is_displayable_and_an_error() {
        let rendered = Cancelled.to_string();
        assert_eq!(rendered, "render pass cancelled");
        let _: &dyn std::error::Error = &Cancelled;
    }

    #[test]
    fn default_token_matches_new() {
        let token = CancellationToken::default();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn debug_mentions_the_type() {
        let token = CancellationToken::new();
        let rendered = format!("{token:?}");
        assert!(rendered.contains("CancellationToken"));
    }
}
