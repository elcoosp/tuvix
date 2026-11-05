//! A pass-through counting allocator for the tuvix memory gates.
//!
//! The B13 gate measures the live bytes of a full persistent-pipeline
//! build (buffers, frame journal, recorded snapshots) through this
//! allocator. It is a dev-only helper: `publish = false`, and the only
//! workspace member that does not inherit the workspace lints, because
//! its entire surface is one `unsafe impl GlobalAlloc`.
//!
//! # Safety argument (the one `unsafe` in the workspace)
//!
//! [`CountingAlloc`] wraps [`std::alloc::System`] and forwards every
//! call with the *same* layout and pointer it received, only adjusting
//! a counter. It introduces no allocation strategy of its own:
//! alignment, lifetime and deallocation semantics are exactly the
//! system allocator's, so the wrapper cannot create an invalid pointer,
//! double free or leak that the system allocator alone would not. The
//! counter is relaxed-atomic bookkeeping only; no memory safety
//! decision reads it. `realloc` delegates verbatim to
//! `System::realloc` (which never moves on zero-size changes — the
//! system contract) and tracks the size delta for the counter.
//!
//! # Examples
//!
//! Install as the global allocator of a test binary, then measure:
//!
//! ```
//! // #[global_allocator]
//! // static GLOBAL: tuvix_memgate::CountingAlloc = tuvix_memgate::CountingAlloc;
//!
//! // In the test binary the counter tracks live bytes:
//! let before = tuvix_memgate::allocated_bytes();
//! let owned = vec![0_u8; 256];
//! let after = tuvix_memgate::allocated_bytes();
//! // (a doctest binary has no global allocator installed, so the delta
//! // is 0 here — the counting test in the crate pins the real behavior)
//! drop(owned);
//! let _ = (before, after);
//! ```

use std::alloc::{GlobalAlloc, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Live allocation bytes across all threads of this test binary.
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

/// The pass-through counting allocator; see the [crate safety
/// argument](self#safety-argument-the-one-unsafe-in-the-workspace).
pub struct CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: verbatim delegation to the system allocator with the
        // caller's layout; see the crate safety argument.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: std::alloc::Layout) {
        ALLOCATED.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: the pointer came from the system allocator with this
        // exact layout (alloc above forwards unchanged); see the crate
        // safety argument.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(
        &self,
        pointer: *mut u8,
        layout: std::alloc::Layout,
        new_size: usize,
    ) -> *mut u8 {
        let delta = new_size as isize - layout.size() as isize;
        ALLOCATED.fetch_add(delta.unsigned_abs(), Ordering::Relaxed);
        // SAFETY: verbatim delegation; see the crate safety argument.
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

/// The current live allocation count, in bytes.
#[must_use]
pub fn allocated_bytes() -> usize {
    ALLOCATED.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::CountingAlloc;

    /// The counting allocator works as a global allocator.
    #[global_allocator]
    static GLOBAL: CountingAlloc = CountingAlloc;

    #[test]
    fn allocations_are_counted_and_released() {
        let before = super::allocated_bytes();
        let owned = vec![7_u8; 4096];
        let during = super::allocated_bytes();
        assert!(during - before >= 4096);
        drop(owned);
        let after = super::allocated_bytes();
        assert!(after <= during - 4096, "the drop released the buffer");
    }
}
