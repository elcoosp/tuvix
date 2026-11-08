//! M12-C: the C ABI, exercised the way a C host exercises it — through the
//! exported symbols, with raw pointers and the documented result codes.

use tuvix_core::buffer::Buffer;
use tuvix_core::embed::{Mutation, PacketWriter, decode_snapshot};
use tuvix_core::geometry::Rect;
use tuvix_core::style::{Color, Style};
use tuvix_ffi::{
    TVX_ERR_KIND_BAD_MAGIC, TVX_ERR_KIND_UNKNOWN_VERSION, TVX_ERR_NULL, TVX_ERR_PACKET, TVX_OK,
    error_kind_code, tvx_buffer_apply, tvx_buffer_free, tvx_buffer_new, tvx_buffer_render_ansi,
    tvx_buffer_resize, tvx_buffer_size, tvx_buffer_snapshot, tvx_string_version, tvx_version,
};

/// A one-op packet: write "hello" with a red style at (0, 1).
fn hello_packet() -> Vec<u8> {
    PacketWriter::new()
        .push(&Mutation::SetText {
            x: 0,
            y: 1,
            text: "hello".to_owned(),
            style: Style::new().fg(Color::Red),
        })
        .finish()
}

#[test]
fn apply_then_snapshot_then_read_back() {
    unsafe {
        let handle = tvx_buffer_new(20, 3);
        assert!(!handle.is_null());

        let packet = hello_packet();
        let mut consumed = 0usize;
        let mut err_kind = 0i32;
        let mut err_offset = 0usize;
        let result = tvx_buffer_apply(
            handle,
            packet.as_ptr(),
            packet.len(),
            std::ptr::addr_of_mut!(consumed),
            std::ptr::addr_of_mut!(err_kind),
            std::ptr::addr_of_mut!(err_offset),
        );
        assert_eq!(result, TVX_OK);
        assert_eq!(consumed, packet.len());

        let mut width = 0u16;
        let mut height = 0u16;
        assert_eq!(
            tvx_buffer_size(
                handle,
                std::ptr::addr_of_mut!(width),
                std::ptr::addr_of_mut!(height)
            ),
            TVX_OK
        );
        assert_eq!((width, height), (20, 3));

        // Snapshot: capacity negotiation first, then the full read-back.
        let needed = tvx_buffer_snapshot(handle, std::ptr::null_mut(), 0);
        assert!(needed < 0);
        let needed = usize::try_from(-needed).unwrap();
        let mut scratch = vec![0u8; needed];
        let written = tvx_buffer_snapshot(handle, scratch.as_mut_ptr(), scratch.len());
        assert_eq!(
            written,
            i64::try_from(needed).expect("the scratch length fits an i64")
        );
        let decoded = decode_snapshot(&scratch).unwrap();
        assert_eq!(decoded.area.width, 20);
        assert_eq!(decoded.get(0, 1).unwrap().symbol.as_str(), "h");
        assert_eq!(decoded.get(0, 1).unwrap().style.fg, Some(Color::Red));

        // The ANSI render matches the canonical snapshot encoder's output:
        // the text is on screen, nothing wrote invalid UTF-8.
        let ansi_needed = tvx_buffer_render_ansi(handle, std::ptr::null_mut(), 0);
        let ansi_needed = usize::try_from(-ansi_needed).unwrap();
        let mut ansi = vec![0u8; ansi_needed];
        assert_eq!(
            tvx_buffer_render_ansi(handle, ansi.as_mut_ptr(), ansi.len()),
            i64::try_from(ansi_needed).expect("the ansi length fits an i64")
        );
        let ansi = String::from_utf8(ansi).expect("the encoder emits UTF-8 only");
        assert!(ansi.contains("hello"), "ansi render: {ansi:?}");

        tvx_buffer_free(handle);
    }
}

#[test]
fn rejected_packet_reports_kind_and_offset_and_applies_nothing() {
    unsafe {
        let handle = tvx_buffer_new(10, 2);
        let mut bad = hello_packet();
        bad[0] = b'X'; // break the magic
        let mut consumed = 0usize;
        let mut err_kind = 0i32;
        let mut err_offset = 0usize;
        let result = tvx_buffer_apply(
            handle,
            bad.as_ptr(),
            bad.len(),
            std::ptr::addr_of_mut!(consumed),
            std::ptr::addr_of_mut!(err_kind),
            std::ptr::addr_of_mut!(err_offset),
        );
        assert_eq!(result, TVX_ERR_PACKET);
        assert_eq!(err_kind, TVX_ERR_KIND_BAD_MAGIC);
        assert_eq!(err_offset, 0);
        assert_eq!(consumed, 0);

        // Nothing landed: the buffer is still blank.
        let mut scratch = vec![0u8; 256];
        let written = tvx_buffer_snapshot(handle, scratch.as_mut_ptr(), scratch.len());
        let decoded = decode_snapshot(&scratch[..usize::try_from(written).unwrap()]).unwrap();
        assert_eq!(decoded, Buffer::empty(Rect::new(0, 0, 10, 2)));

        // Unknown version carries its offset too.
        let bad_version = [b'T', b'V', b'X', 9u8];
        let result = tvx_buffer_apply(
            handle,
            bad_version.as_ptr(),
            bad_version.len(),
            std::ptr::addr_of_mut!(consumed),
            std::ptr::addr_of_mut!(err_kind),
            std::ptr::addr_of_mut!(err_offset),
        );
        assert_eq!(result, TVX_ERR_PACKET);
        assert_eq!(err_kind, TVX_ERR_KIND_UNKNOWN_VERSION);
        assert_eq!(err_offset, 3);

        tvx_buffer_free(handle);
    }
}

#[test]
fn null_arguments_report_tvx_err_null() {
    unsafe {
        let result = tvx_buffer_apply(
            std::ptr::null_mut(),
            hello_packet().as_ptr(),
            4,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        assert_eq!(result, TVX_ERR_NULL);

        let handle = tvx_buffer_new(4, 2);
        assert_eq!(
            tvx_buffer_apply(
                handle,
                std::ptr::null(),
                8,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            TVX_ERR_NULL
        );
        assert_eq!(tvx_buffer_resize(std::ptr::null_mut(), 4, 2), TVX_ERR_NULL);
        assert_eq!(
            tvx_buffer_size(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut()
            ),
            TVX_ERR_NULL
        );
        assert_eq!(
            tvx_buffer_snapshot(std::ptr::null_mut(), std::ptr::null_mut(), 0),
            i64::from(TVX_ERR_NULL)
        );
        assert_eq!(
            tvx_buffer_render_ansi(std::ptr::null_mut(), std::ptr::null_mut(), 0),
            i64::from(TVX_ERR_NULL)
        );

        // Free tolerates null; the ownership contract (exactly one free of
        // a live handle) is documented, not enforced at runtime.
        tvx_buffer_free(std::ptr::null_mut());
        tvx_buffer_free(handle);
    }
}

#[test]
fn resize_grows_and_shrinks_through_the_abi() {
    unsafe {
        let handle = tvx_buffer_new(10, 2);
        assert_eq!(tvx_buffer_resize(handle, 40, 9), TVX_OK);
        let mut width = 0u16;
        let mut height = 0u16;
        assert_eq!(
            tvx_buffer_size(
                handle,
                std::ptr::addr_of_mut!(width),
                std::ptr::addr_of_mut!(height)
            ),
            TVX_OK
        );
        assert_eq!((width, height), (40, 9));
        tvx_buffer_free(handle);
    }
}

#[test]
fn zero_length_apply_is_rejected_as_truncated() {
    unsafe {
        let handle = tvx_buffer_new(6, 2);
        // A zero-length buffer has no header, so the strict parser rejects
        // it instead of guessing.
        let mut consumed = 0usize;
        let mut err_kind = 0i32;
        let mut err_offset = 0usize;
        let result = tvx_buffer_apply(
            handle,
            std::ptr::null(),
            0,
            std::ptr::addr_of_mut!(consumed),
            std::ptr::addr_of_mut!(err_kind),
            std::ptr::addr_of_mut!(err_offset),
        );
        assert_eq!(result, TVX_ERR_PACKET);
        assert_eq!(
            err_kind,
            error_kind_code(tuvix_core::embed::PacketErrorKind::UnexpectedEnd)
        );
        tvx_buffer_free(handle);
    }
}

#[test]
fn version_queries_are_stable() {
    assert_ne!(tvx_version(), 0);
    let version = unsafe { std::ffi::CStr::from_ptr(tvx_string_version()) };
    assert_eq!(version.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
}
