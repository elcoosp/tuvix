//! The C ABI embedding boundary of the tuvix terminal UI framework.
//!
//! Hosts that cannot speak Rust — C, C++, Swift, C#, Python via ctypes,
//! anything with a foreign-function interface — drive a tuvix buffer
//! through this surface: create a handle, feed it mutation packets (the
//! `tuvix-core` `embed` protocol), read the screen back as a snapshot
//! packet or as canonical ANSI, free the handle. One apply call carries a
//! whole batch of mutations — the architecture's §5.1 batching mechanism
//! and the §8 gate 1 budget (more than 50,000 mutations per second) live
//! here, measured by the B15 gate.
//!
//! # The safety argument
//!
//! The workspace forbids `unsafe` code with one exception per audited
//! boundary; this crate is that exception for the C ABI. The discipline:
//!
//! - Every exported function is a thin wrapper: it validates its pointer
//!   arguments (null checks), delegates to a *safe* `tuvix-core` call, and
//!   never holds borrowed data across calls.
//! - A `TvxBuffer` handle owns its [`Buffer`] through a `Box`; the pointer
//!   returned by [`tvx_buffer_new`] stays valid until exactly one
//!   [`tvx_buffer_free`]. After `free`, every use is undefined behavior —
//!   the standard C ownership contract, stated here and in `tuvix.h`.
//! - Handles are not thread-safe: a `TvxBuffer` must be used from one
//!   thread at a time (it is `Send` as a Rust value, but the C ABI hands
//!   out no synchronization — the host's job, like any non-thread-safe C
//!   library).
//! - `data` pointers passed to [`tvx_buffer_apply`] need only be valid for
//!   the duration of the call and for `len` bytes; nothing retains them.
//! - Output functions ([`tvx_buffer_snapshot`], [`tvx_buffer_render_ansi`])
//!   write at most `cap` bytes and never run past the caller's buffer;
//!   pass `cap = 0` (with any non-null or null `out`) to learn the needed
//!   capacity from the negative return value.
//! - Panics do not cross the boundary: every wrapper's payload is
//!   panic-free over validated input (the strict packet parser reports
//!   errors instead of panicking), and allocation failure aborts the
//!   process rather than unwinding into C.
//!
//! # Result codes
//!
//! Functions returning `i32` use [`TVX_OK`] (0), [`TVX_ERR_NULL`] (-1,
//! a required pointer argument was null) and [`TVX_ERR_PACKET`] (1, the
//! packet was rejected; `err_kind`/`err_offset` carry the details, encoded
//! by the `TVX_ERR_KIND_*` constants). Functions returning `i64` deliver
//! byte counts: non-negative for bytes written, `-needed` when `cap` was
//! too small.
//!
//! # Examples
//!
//! The ABI is exercised the same way a C host would call it — through the
//! exported symbols (see `tests/abi.rs`):
//!
//! ```
//! use tuvix_ffi::{
//!     tvx_buffer_apply, tvx_buffer_free, tvx_buffer_new, tvx_buffer_snapshot,
//!     TVX_ERR_KIND_UNKNOWN_VERSION, TVX_OK,
//! };
//!
//! # unsafe {
//! let handle = tvx_buffer_new(20, 3);
//! assert!(!handle.is_null());
//!
//! // A one-op packet: resize is implicit in the handle, so write text.
//! let mut packet = b"TVX".to_vec();
//! packet.push(1); // version
//! packet.extend_from_slice(&1u32.to_le_bytes()); // one op
//! packet.push(0x01); // SET_TEXT
//! packet.extend_from_slice(&0u16.to_le_bytes()); // x
//! packet.extend_from_slice(&0u16.to_le_bytes()); // y
//! packet.extend_from_slice(&2u16.to_le_bytes()); // text length
//! packet.extend_from_slice(b"hi");
//! packet.push(0); // style: nothing set
//!
//! let mut consumed = 0usize;
//! let mut err_kind = 0i32;
//! let mut err_offset = 0usize;
//! let result = tvx_buffer_apply(
//!     handle, packet.as_ptr(), packet.len(),
//!     &mut consumed, &mut err_kind, &mut err_offset,
//! );
//! assert_eq!(result, TVX_OK);
//! assert_eq!(consumed, packet.len());
//!
//! let mut scratch = [0u8; 64];
//! let written = tvx_buffer_snapshot(handle, scratch.as_mut_ptr(), scratch.len());
//! assert!(written > 0);
//!
//! tvx_buffer_free(handle);
//!
//! // A bad version byte is rejected with the evidence offset.
//! let handle = tvx_buffer_new(4, 2);
//! let bad = [b'T', b'V', b'X', 9];
//! let result = tvx_buffer_apply(
//!     handle, bad.as_ptr(), bad.len(),
//!     &mut consumed, &mut err_kind, &mut err_offset,
//! );
//! assert_eq!(result, 1); // TVX_ERR_PACKET
//! assert_eq!(err_kind, TVX_ERR_KIND_UNKNOWN_VERSION);
//! assert_eq!(err_offset, 3);
//! tvx_buffer_free(handle);
//! # }
//! ```

use std::ffi::c_char;

use tuvix_core::buffer::Buffer;
use tuvix_core::embed::{
    PacketError, PacketErrorKind, decode_snapshot, encode_snapshot, parse_packets,
};
use tuvix_core::geometry::Rect;
use tuvix_core::preview::serialize_ansi;

// Result codes: keep in lockstep with `include/tuvix.h`.

/// Success.
pub const TVX_OK: i32 = 0;
/// A required pointer argument was null.
pub const TVX_ERR_NULL: i32 = -1;
/// The packet was rejected; see the `err_kind` output.
pub const TVX_ERR_PACKET: i32 = 1;

// Packet error kind codes: keep in lockstep with `include/tuvix.h` and
// with `PacketErrorKind`.

/// `PacketErrorKind::BadMagic`.
pub const TVX_ERR_KIND_BAD_MAGIC: i32 = 1;
/// `PacketErrorKind::UnknownVersion`.
pub const TVX_ERR_KIND_UNKNOWN_VERSION: i32 = 2;
/// `PacketErrorKind::UnexpectedEnd`.
pub const TVX_ERR_KIND_UNEXPECTED_END: i32 = 3;
/// `PacketErrorKind::UnknownOp`.
pub const TVX_ERR_KIND_UNKNOWN_OP: i32 = 4;
/// `PacketErrorKind::UnknownColor`.
pub const TVX_ERR_KIND_UNKNOWN_COLOR: i32 = 5;
/// `PacketErrorKind::InvalidUtf8`.
pub const TVX_ERR_KIND_INVALID_UTF8: i32 = 6;
/// `PacketErrorKind::ControlCharacter`.
pub const TVX_ERR_KIND_CONTROL_CHARACTER: i32 = 7;
/// `PacketErrorKind::TrailingBytes`.
pub const TVX_ERR_KIND_TRAILING_BYTES: i32 = 8;
/// `PacketErrorKind::ReservedBits`.
pub const TVX_ERR_KIND_RESERVED_BITS: i32 = 9;
/// `PacketErrorKind::CellOutOfRange`.
pub const TVX_ERR_KIND_CELL_OUT_OF_RANGE: i32 = 10;

/// An owned tuvix buffer, driven through the C ABI.
///
/// The handle is opaque to C hosts; Rust hosts can use the safe inherent
/// methods (`apply_packet`, `snapshot`, `render_ansi`) directly.
#[derive(Debug)]
pub struct TvxBuffer {
    buffer: Buffer,
}

impl TvxBuffer {
    /// Creates a buffer of blank cells, `width` by `height`.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            buffer: Buffer::empty(Rect::new(0, 0, width, height)),
        }
    }

    /// Applies one mutation packet: parses the whole batch strictly, then
    /// applies every mutation — all-or-nothing, nothing lands on a rejected
    /// packet. Returns the bytes consumed (the whole packet on success, the
    /// error offset otherwise).
    ///
    /// # Errors
    ///
    /// The packet's [`PacketError`] when rejected.
    pub fn apply_packet(&mut self, data: &[u8]) -> Result<usize, PacketError> {
        let mutations = parse_packets(data)?;
        for mutation in &mutations {
            mutation.apply(&mut self.buffer);
        }
        Ok(data.len())
    }

    /// The buffer's width in columns.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.buffer.area.width
    }

    /// The buffer's height in rows.
    #[must_use]
    pub fn height(&self) -> u16 {
        self.buffer.area.height
    }

    /// Resizes the buffer, losing content when the area changes.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.buffer.resize(Rect::new(0, 0, width, height));
    }

    /// Encodes the visible grid as a frame snapshot packet.
    #[must_use]
    pub fn snapshot(&self) -> Vec<u8> {
        encode_snapshot(&self.buffer)
    }

    /// Renders the visible grid as the canonical ANSI snapshot (the M4
    /// preview encoding — the exact bytes a preview tap would send).
    #[must_use]
    pub fn render_ansi(&self) -> Vec<u8> {
        serialize_ansi(&self.buffer).into_bytes()
    }

    /// Reads the buffer back through the snapshot decoder.
    ///
    /// The differential identity `decode(encode(buffer)) == buffer` is a
    /// core gate; this method exposes the decoded read-back to Rust hosts
    /// that prefer it over raw snapshot bytes.
    ///
    /// # Panics
    /// Panics when the self-produced snapshot fails to decode —
    /// unreachable while the differential gate `decode(encode(b)) == b`
    /// holds.
    #[must_use]
    pub fn read_back(&self) -> Buffer {
        decode_snapshot(&self.snapshot()).expect("self-produced snapshot is valid")
    }
}

/// Maps a packet error onto its ABI kind code (the `TVX_ERR_KIND_*`
/// constants).
#[must_use]
pub fn error_kind_code(kind: PacketErrorKind) -> i32 {
    match kind {
        PacketErrorKind::BadMagic => TVX_ERR_KIND_BAD_MAGIC,
        PacketErrorKind::UnknownVersion { .. } => TVX_ERR_KIND_UNKNOWN_VERSION,
        PacketErrorKind::UnexpectedEnd => TVX_ERR_KIND_UNEXPECTED_END,
        PacketErrorKind::UnknownOp { .. } => TVX_ERR_KIND_UNKNOWN_OP,
        PacketErrorKind::UnknownColor { .. } => TVX_ERR_KIND_UNKNOWN_COLOR,
        PacketErrorKind::InvalidUtf8 => TVX_ERR_KIND_INVALID_UTF8,
        PacketErrorKind::ControlCharacter => TVX_ERR_KIND_CONTROL_CHARACTER,
        PacketErrorKind::TrailingBytes => TVX_ERR_KIND_TRAILING_BYTES,
        PacketErrorKind::ReservedBits => TVX_ERR_KIND_RESERVED_BITS,
        PacketErrorKind::CellOutOfRange => TVX_ERR_KIND_CELL_OUT_OF_RANGE,
    }
}

/// Writes `bytes` into the caller's buffer: returns the byte count when it
/// fits, `-(needed)` when the capacity was too small (the caller retries
/// with `cap = needed`). `out` may be null when `cap` is zero.
///
/// # Safety
///
/// `out` must be null or valid for `cap` bytes of writes.
unsafe fn copy_out(bytes: &[u8], out: *mut u8, cap: usize) -> i64 {
    let needed = bytes.len();
    let needed_i64 = i64::try_from(needed).unwrap_or(i64::MAX);
    if cap < needed {
        return -(needed_i64);
    }
    if !out.is_null() {
        // SAFETY: the caller guarantees `out` is valid for `cap >= needed`
        // bytes, and the buffers cannot overlap (`out` belongs to the C
        // host, `bytes` to this call).
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, needed);
        }
    }
    needed_i64
}

/// The ABI version, packed as `(major << 16) | (minor << 8) | patch` —
/// the crate version the host was built against.
#[must_use]
pub extern "C" fn tvx_version() -> u32 {
    // Cargo guarantees the three numeric parts; anything unparsable
    // degrades to zero for that part rather than panicking at the ABI.
    let mut parts = env!("CARGO_PKG_VERSION").split('.');
    let mut next_part = || {
        parts
            .next()
            .and_then(|part| part.parse::<u32>().ok())
            .unwrap_or(0)
    };
    (next_part() << 16) | (next_part() << 8) | next_part()
}

/// Creates a buffer of blank cells, `width` by `height`.
///
/// Free the handle with exactly one [`tvx_buffer_free`]. Never returns
/// null (allocation failure aborts the process).
///
/// # Safety
///
/// This function is safe to call from any thread; the returned handle is
/// not (see the crate-level thread contract).
#[must_use = "the handle must be freed with tvx_buffer_free"]
pub unsafe extern "C" fn tvx_buffer_new(width: u16, height: u16) -> *mut TvxBuffer {
    // `Box::into_raw` hands out a valid, uniquely owned pointer; the
    // allocation failure case aborts instead of returning null.
    Box::into_raw(Box::new(TvxBuffer::new(width, height)))
}

/// Frees a handle created by [`tvx_buffer_new`].
///
/// # Safety
///
/// `handle` must be null (nothing happens) or a pointer returned by
/// [`tvx_buffer_new`] that has not been freed yet. After this call the
/// handle is dangling: every further use is undefined behavior.
pub unsafe extern "C" fn tvx_buffer_free(handle: *mut TvxBuffer) {
    if !handle.is_null() {
        // SAFETY: the contract (crate docs, `tuvix.h`) is that `handle`
        // came from `tvx_buffer_new` and has not been freed before; the
        // box is reconstructed exactly once and dropped.
        unsafe {
            drop(Box::from_raw(handle));
        }
    }
}

/// Applies one mutation packet to the handle's buffer, all-or-nothing.
///
/// On success returns [`TVX_OK`] and sets `*consumed` to `len`. On a
/// rejected packet returns [`TVX_ERR_PACKET`] with `*err_kind` set to the
/// `TVX_ERR_KIND_*` code and `*err_offset` to the offending byte; nothing
/// is applied and `*consumed` receives the error offset (the packet is
/// valid up to there). Null `err_kind`/`err_offset`/`consumed` skip the
/// corresponding output.
///
/// # Safety
///
/// - `handle` must be a live handle from [`tvx_buffer_new`].
/// - `data` must be valid for `len` bytes of reads for the duration of
///   the call; nothing retains it.
/// - `consumed`, `err_kind` and `err_offset` must each be null or valid
///   for one write of their type.
pub unsafe extern "C" fn tvx_buffer_apply(
    handle: *mut TvxBuffer,
    data: *const u8,
    len: usize,
    consumed: *mut usize,
    err_kind: *mut i32,
    err_offset: *mut usize,
) -> i32 {
    if handle.is_null() || (data.is_null() && len > 0) {
        return TVX_ERR_NULL;
    }
    // SAFETY: `handle` is a live handle per the caller contract; `data` is
    // valid for `len` reads for the duration of the call (or null with
    // `len == 0`, which reads nothing) and nothing retains it.
    let bytes = unsafe {
        if data.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(data, len)
        }
    };
    let outcome = unsafe { (*handle).apply_packet(bytes) };
    match outcome {
        Ok(applied) => {
            // SAFETY: `consumed` is null (skipped) or valid for one write.
            unsafe {
                if !consumed.is_null() {
                    *consumed = applied;
                }
            }
            TVX_OK
        }
        Err(error) => {
            // SAFETY: each pointer is null (skipped) or valid for one write.
            unsafe {
                if !consumed.is_null() {
                    *consumed = error.offset;
                }
                if !err_kind.is_null() {
                    *err_kind = error_kind_code(error.kind);
                }
                if !err_offset.is_null() {
                    *err_offset = error.offset;
                }
            }
            TVX_ERR_PACKET
        }
    }
}

/// Resizes the handle's buffer, losing content when the area changes.
///
/// # Safety
///
/// `handle` must be a live handle from [`tvx_buffer_new`].
pub unsafe extern "C" fn tvx_buffer_resize(handle: *mut TvxBuffer, width: u16, height: u16) -> i32 {
    if handle.is_null() {
        return TVX_ERR_NULL;
    }
    // SAFETY: `handle` is a live handle per the caller contract.
    unsafe {
        (*handle).resize(width, height);
    }
    TVX_OK
}

/// Writes the handle's width and height through `width`/`height` (each may
/// be null to skip).
///
/// # Safety
///
/// - `handle` must be a live handle from [`tvx_buffer_new`].
/// - `width` and `height` must each be null or valid for one `u16` write.
pub unsafe extern "C" fn tvx_buffer_size(
    handle: *mut TvxBuffer,
    width: *mut u16,
    height: *mut u16,
) -> i32 {
    if handle.is_null() {
        return TVX_ERR_NULL;
    }
    // SAFETY: `handle` is a live handle per the caller contract; the
    // output pointers are null (skipped) or valid for one write.
    unsafe {
        if !width.is_null() {
            *width = (*handle).width();
        }
        if !height.is_null() {
            *height = (*handle).height();
        }
    }
    TVX_OK
}

/// Encodes the visible grid as a frame snapshot packet into `out`.
///
/// Returns the byte count written, or `-(needed)` when `cap` was too
/// small — call again with `cap = needed`. `out` may be null when `cap`
/// is zero.
///
/// # Safety
///
/// - `handle` must be a live handle from [`tvx_buffer_new`].
/// - `out` must be null or valid for `cap` bytes of writes.
pub unsafe extern "C" fn tvx_buffer_snapshot(
    handle: *mut TvxBuffer,
    out: *mut u8,
    cap: usize,
) -> i64 {
    if handle.is_null() {
        return i64::from(TVX_ERR_NULL);
    }
    // SAFETY: `handle` is a live handle per the caller contract.
    let snapshot = unsafe { (*handle).snapshot() };
    // SAFETY: the caller guarantees `out` is null or valid for `cap`
    // bytes; `copy_out` writes at most `snapshot.len()` bytes and only
    // through a non-null `out` with sufficient capacity.
    unsafe { copy_out(&snapshot, out, cap) }
}

/// Renders the visible grid as the canonical ANSI snapshot into `out`,
/// with the same capacity negotiation as [`tvx_buffer_snapshot`].
///
/// # Safety
///
/// - `handle` must be a live handle from [`tvx_buffer_new`].
/// - `out` must be null or valid for `cap` bytes of writes.
pub unsafe extern "C" fn tvx_buffer_render_ansi(
    handle: *mut TvxBuffer,
    out: *mut u8,
    cap: usize,
) -> i64 {
    if handle.is_null() {
        return i64::from(TVX_ERR_NULL);
    }
    // SAFETY: `handle` is a live handle per the caller contract.
    let ansi = unsafe { (*handle).render_ansi() };
    // SAFETY: the caller guarantees `out` is null or valid for `cap`
    // bytes; `copy_out` writes at most `ansi.len()` bytes and only
    // through a non-null `out` with sufficient capacity.
    unsafe { copy_out(&ansi, out, cap) }
}

/// Reads the crate's semantic version as a C string (`major.minor.patch`).
///
/// The string is static — no allocation, valid for the process lifetime,
/// never freed.
///
/// # Safety
///
/// The returned pointer is valid forever and must not be written to.
#[must_use]
pub extern "C" fn tvx_string_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0")
        .as_ptr()
        .cast::<c_char>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_packs_major_minor_patch() {
        // The packed value decodes back to the crate version string.
        let version = tvx_version();
        let packed = format!(
            "{}.{}.{}",
            version >> 16,
            (version >> 8) & 0xff,
            version & 0xff
        );
        assert_eq!(packed, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn error_kind_codes_are_exhaustive_and_positive() {
        for code in [
            error_kind_code(PacketErrorKind::BadMagic),
            error_kind_code(PacketErrorKind::UnknownVersion { found: 0 }),
            error_kind_code(PacketErrorKind::UnexpectedEnd),
            error_kind_code(PacketErrorKind::UnknownOp { op: 0 }),
            error_kind_code(PacketErrorKind::UnknownColor { kind: 0 }),
            error_kind_code(PacketErrorKind::InvalidUtf8),
            error_kind_code(PacketErrorKind::ControlCharacter),
            error_kind_code(PacketErrorKind::TrailingBytes),
            error_kind_code(PacketErrorKind::ReservedBits),
            error_kind_code(PacketErrorKind::CellOutOfRange),
        ] {
            assert!(code > 0);
        }
    }
}
