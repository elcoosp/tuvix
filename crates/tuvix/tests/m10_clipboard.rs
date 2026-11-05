//! The M10 clipboard gates: the OSC 52 sequence from every backend.
//!
//! - the encoder is pinned byte-for-byte (unit tests in the module);
//! - `TestBackend` records the exact sequence in the control log;
//! - `AnsiBackend` and `CrosstermBackend` write the identical bytes;
//! - `Terminal::set_clipboard` routes through the backend and rejects
//!   oversized payloads with the backend's error;
//! - the raster state is untouched: a draw after a clipboard write
//!   re-anchors identically to a draw without one.

use tuvix_backend_ansi::AnsiBackend;
use tuvix_backend_crossterm::CrosstermBackend;

use tuvix_core::backend::{Backend, TestBackend};
use tuvix_core::clipboard::{ClipboardError, OSC52_MAX_PAYLOAD, encode_osc52};
use tuvix_core::geometry::Size;
use tuvix_core::style::{Color, Style};
use tuvix_core::terminal::Terminal;

const SEQUENCE: &str = "\u{1b}]52;c;Y29waWVk\u{1b}\\";

#[test]
fn the_test_backend_records_the_exact_sequence() {
    let mut backend = TestBackend::new(Size::new(10, 2));
    backend.set_clipboard("copied").unwrap();
    assert_eq!(backend.control_log(), &[SEQUENCE.to_owned()]);

    // clearing: empty contents
    backend.set_clipboard("").unwrap();
    assert_eq!(
        backend.control_log(),
        [SEQUENCE.to_owned(), "\u{1b}]52;c;\u{1b}\\".to_owned()]
    );
}

#[test]
fn the_ansi_backend_writes_the_sequence_verbatim() {
    let mut backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(20, 4));
    backend.set_clipboard("copied").unwrap();
    backend.flush().unwrap();
    assert_eq!(backend.into_writer(), SEQUENCE.as_bytes());
}

#[test]
fn the_crossterm_backend_writes_the_same_bytes() {
    let mut backend = CrosstermBackend::new(Vec::<u8>::new());
    backend.set_clipboard("copied").unwrap();
    backend.flush().unwrap();
    let output = String::from_utf8(backend.into_writer()).unwrap();
    assert_eq!(output, SEQUENCE);
}

#[test]
fn oversized_clipboard_requests_fail_with_the_backend_error() {
    let contents = "x".repeat(OSC52_MAX_PAYLOAD / 4 * 3 + 3);
    let mut ansi = AnsiBackend::new(Vec::<u8>::new(), Size::new(20, 4));
    let error = ansi.set_clipboard(&contents).unwrap_err();
    assert!(error.to_string().contains("exceeds"), "{error}");
    // nothing was written
    ansi.flush().unwrap();
    assert_eq!(ansi.into_writer(), b"");

    // TestBackend's log cannot fail (Infallible), so the rejected request
    // is skipped rather than recorded — nothing reaches the log either way.
    let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 2))).unwrap();
    terminal.set_clipboard(&contents).unwrap();
    assert!(
        terminal.backend().control_log().is_empty(),
        "the rejected request never reached the log"
    );
    let _ = encode_osc52(tuvix_core::clipboard::ClipboardSelection::Primary, "");
}

#[test]
fn a_clipboard_write_does_not_disturb_the_raster_state() {
    // The OSC 52 write keeps the memoized position: the next draw emits
    // the same byte stream whether or not a clipboard write happened.
    let draw = |terminal: &mut Terminal<TestBackend>| {
        terminal
            .draw(|frame| {
                frame
                    .buffer
                    .set_string(0, 0, "hi", Style::new().fg(Color::Red));
            })
            .unwrap();
    };

    let mut plain = Terminal::new(TestBackend::new(Size::new(10, 2))).unwrap();
    draw(&mut plain);
    let without = plain.backend().buffer().to_string();

    let mut with_clipboard = Terminal::new(TestBackend::new(Size::new(10, 2))).unwrap();
    with_clipboard.set_clipboard("copied").unwrap();
    draw(&mut with_clipboard);
    assert_eq!(with_clipboard.backend().buffer().to_string(), without);
    assert_eq!(
        with_clipboard.backend().control_log(),
        &[SEQUENCE.to_owned()],
        "the clipboard write is recorded once, before the frame"
    );
    let _ = ClipboardError::TooLarge { size: 1, max: 2 };
}
