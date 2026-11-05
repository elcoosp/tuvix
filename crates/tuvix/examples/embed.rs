//! The M12 embedding boundary in action: a host drives a tuvix buffer
//! through mutation packets — one batched packet per frame, the screen
//! read back as snapshot packets — no per-property calls, no second
//! renderer. This is the architecture's §5.1 batching mechanism, with the
//! §8 gate 1 budget (50,000 mutations/sec) measured by the B15 gate at the
//! C ABI (`tuvix-ffi/tests/bench_gates.rs`).
//!
//! The host side here plays the role of a foreign-language program: it
//! speaks only flat byte packets. It drives the boundary through
//! `tuvix_ffi::TvxBuffer`'s safe inherent methods — the exact functions a
//! C host calls through `tuvix.h` (`tests/abi.rs` exercises the raw
//! symbols; the facade keeps its `forbid(unsafe_code)`).
//!
//! This example is headless-safe: the "terminal" is a `TestBackend`.
//!
//! Run with: cargo run -p tuvix --example embed

use tuvix::prelude::*;
use tuvix_ffi::TvxBuffer;

/// The host side of the boundary: builds mutation packets the way a
/// foreign-language host would — flat byte strings, no Rust rendering
/// types cross the line.
fn host_frame_packets() -> Vec<Vec<u8>> {
    let title = Mutation::SetText {
        x: 1,
        y: 0,
        text: "tuvix — driven through the embedding boundary".to_owned(),
        style: Style::new().add_modifier(Modifier::BOLD),
    };
    let body = Mutation::SetText {
        x: 1,
        y: 2,
        text: "one packet, three mutations, one boundary crossing".to_owned(),
        style: Style::new().fg(Color::Cyan),
    };
    let link = Mutation::SetHyperlink {
        area: Rect::new(1, 4, 17, 1),
        uri: "https://tuvix.dev".to_owned(),
    };
    let link_text = Mutation::SetText {
        x: 1,
        y: 4,
        text: "the linked line".to_owned(),
        style: Style::new()
            .fg(Color::Blue)
            .add_modifier(Modifier::UNDERLINED),
    };

    let mut first = PacketWriter::new();
    for mutation in [title, body, link, link_text] {
        first = first.push(&mutation);
    }

    // A second frame: the host redraws the body line in place.
    let redraw = Mutation::SetText {
        x: 1,
        y: 2,
        text: "the second packet replaced this line".to_owned(),
        style: Style::new().fg(Color::Green),
    };
    vec![first.finish(), PacketWriter::new().push(&redraw).finish()]
}

/// Reads the screen back through the snapshot ABI and prints what the
/// host would see.
fn print_read_back(handle: &TvxBuffer) {
    let snapshot = handle.snapshot();
    let decoded = tuvix::embed::decode_snapshot(&snapshot).expect("self-produced snapshot");
    println!(
        "-- read back {}x{} through the snapshot ABI --",
        decoded.area.width, decoded.area.height
    );
    for y in 0..decoded.area.height {
        let mut row = String::new();
        for x in 0..decoded.area.width {
            row.push_str(decoded.get(x, y).map_or(" ", |cell| cell.symbol.as_str()));
        }
        println!("{row}");
    }
    let ansi = handle.render_ansi();
    println!("-- canonical ANSI snapshot: {} bytes --", ansi.len());
}

fn main() {
    // The embedding host: a handle holding a 60x6 buffer.
    let mut handle = TvxBuffer::new(60, 6);

    for (frame, packet) in host_frame_packets().into_iter().enumerate() {
        let consumed = handle
            .apply_packet(&packet)
            .unwrap_or_else(|error| panic!("frame {frame} rejected: {error}"));
        assert_eq!(consumed, packet.len(), "frame {frame}");
        println!("frame {frame}: {consumed} bytes applied");
        if frame == 1 {
            // The snapshot decodes into a Buffer that diffs and renders
            // like any other (the M9 Screen discipline, at the boundary).
            print_read_back(&handle);
        }
    }

    // Hand the final frame to a real tuvix Terminal for a last render.
    let snapshot = handle.snapshot();
    let buffer = tuvix::embed::decode_snapshot(&snapshot).expect("self-produced snapshot");
    let mut terminal = Terminal::new(TestBackend::new(Size::new(60, 6))).expect("test backend");
    terminal
        .draw(|frame| {
            frame.buffer.content.clone_from(&buffer.content);
        })
        .expect("draw");

    println!("-- boundary contract held; handle dropped --");
}
