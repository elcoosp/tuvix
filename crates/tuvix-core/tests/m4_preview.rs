//! The M4 IPC preview gate: the terminal streams full-frame ANSI snapshots
//! to any attached [`FrameSink`], and a [`PreviewTap`] broadcasts them to
//! connected clients over TCP (all platforms) and unix sockets (unix).

use std::io::Read;
use std::net::TcpStream;
use std::time::Duration;

use tuvix_core::backend::TestBackend;
use tuvix_core::geometry::Size;
use tuvix_core::preview::{PreviewTap, RecordingSink, serialize_ansi};
use tuvix_core::style::{Color, Style};
use tuvix_core::terminal::Terminal;

#[test]
fn every_flushed_frame_reaches_the_recording_sink() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 2))).unwrap();
    let sink = RecordingSink::new();
    let frames = sink.frames();
    terminal.set_preview(sink);

    terminal
        .draw(|frame| {
            frame
                .buffer
                .set_string(0, 0, "hello", Style::new().fg(Color::Red));
        })
        .unwrap();
    terminal.draw(|_| {}).unwrap();
    terminal
        .draw(|frame| {
            frame.buffer.set_string(0, 1, "world", Style::new());
        })
        .unwrap();

    let recorded = frames.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        3,
        "one snapshot per flushed frame, no-op frames included"
    );
    // Frame 1 shows "hello".
    assert!(recorded[0].contains("hello"));
    assert!(!recorded[0].contains("world"));
    // Frame 2 (empty closure): the immediate-mode screen wipes blank.
    assert!(!recorded[1].contains("hello"));
    // Frame 3 shows "world".
    assert!(recorded[2].contains("world"));
}

#[test]
fn persistent_terminal_previews_match_the_dom() {
    let mut terminal = Terminal::new_persistent(TestBackend::new(Size::new(10, 2))).unwrap();
    let sink = RecordingSink::new();
    let frames = sink.frames();
    terminal.set_preview(sink);

    terminal
        .draw_persistent(|frame| {
            frame.buffer.set_string(0, 0, "static", Style::new());
        })
        .unwrap();
    // A no-op frame still previews — the screen is unchanged, and the
    // snapshot shows it.
    terminal.draw_persistent(|_| {}).unwrap();

    let recorded = frames.lock().unwrap().clone();
    assert_eq!(recorded.len(), 2);
    assert!(recorded[0].contains("static"));
    assert!(recorded[1].contains("static"), "DOM persists in previews");

    // The snapshot equals a direct serialization of the visible buffer.
    let dom = terminal.backend().buffer();
    let direct = serialize_ansi(dom);
    // Compare ignoring trailing reset (identical by construction anyway).
    assert_eq!(recorded[1], direct);
}

#[test]
fn detaching_the_preview_stops_the_stream() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 1))).unwrap();
    let sink = RecordingSink::new();
    let frames = sink.frames();
    terminal.set_preview(sink);
    terminal.draw(|_| {}).unwrap();
    terminal.clear_preview();
    terminal.draw(|_| {}).unwrap();
    assert_eq!(frames.lock().unwrap().len(), 1);
}

#[test]
fn tcp_tap_streams_terminal_frames_to_a_client() {
    let mut tap = PreviewTap::tcp(0).unwrap();
    let port = tap.local_addr().unwrap().port();

    let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    std::thread::sleep(Duration::from_millis(150));

    let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 1))).unwrap();
    terminal.set_preview(tap.sink());
    terminal
        .draw(|frame| {
            frame.buffer.set_string(0, 0, "preview!", Style::new());
        })
        .unwrap();

    // The client receives the frame as ANSI bytes. The tap stays alive, so
    // read once with the timeout rather than waiting for an EOF.
    let mut buffer = [0u8; 512];
    let read = client.read(&mut buffer).unwrap_or(0);
    let received = String::from_utf8_lossy(&buffer[..read]);
    assert!(
        received.contains("preview!"),
        "client received: {received:?}"
    );

    tap.stop();
}

#[test]
fn preview_survives_a_live_tap_being_stopped_early() {
    // Stopping the tap early must not poison the terminal: the sink keeps
    // accepting frames (they go nowhere), and draws keep succeeding.
    let mut tap = PreviewTap::tcp(0).unwrap();
    let sink = tap.sink();
    let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 1))).unwrap();
    terminal.set_preview(sink);
    tap.stop();
    terminal.draw(|_| {}).unwrap();
    terminal.draw_persistent(|_| {}).unwrap();
    // Reaching here is the assertion.
}

#[test]
fn snapshot_helpers_cup_coordinates_match_terminal_geometry() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(4, 2))).unwrap();
    terminal
        .draw(|frame| {
            frame.buffer.set_string(2, 1, "ab", Style::new());
        })
        .unwrap();
    let snapshot = serialize_ansi(terminal.backend().buffer());
    // Row 2 addressed at column 1 before the first glyph of that row.
    assert!(snapshot.contains("\x1b[2;1H"));
}
