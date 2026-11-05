//! M8 integration: cancellable frames discard abandoned work.
//!
//! The architecture's differentiator — input preempts rendering, and the
//! abandoned render is *discarded, not committed* — is proven here at the
//! same rigor the M4 journal differential established: an interrupted
//! paint sequence (committed frames interleaved with abandoned ones and a
//! re-render after the "input was handled") must produce exactly the
//! screen an uninterrupted sequence produces, wide characters and
//! persistent DOMs included.
//!
//! The paint sequences honor the immediate-mode contract: every frame
//! repaints the full logical screen (all widgets painted so far), which
//! is what a real application's render closure does.

use tuvix_core::backend::TestBackend;
use tuvix_core::cancellation::CancellationToken;
use tuvix_core::geometry::Size;
use tuvix_core::style::Style;
use tuvix_core::terminal::{DrawOutcome, Terminal};

const W: u16 = 20;
const H: u16 = 6;

/// One "widget" of the deterministic dashboard, by index.
fn paint_widget(frame: &mut tuvix_core::terminal::Frame<'_>, step: usize) {
    let buffer = &mut *frame.buffer;
    match step {
        0 => {
            buffer.set_string(0, 0, "HEADER  v0.9", Style::new());
        }
        1 => {
            buffer.set_string(0, 1, "汉字 wide glyphs", Style::new());
        }
        2 => {
            buffer.set_string(0, 3, "list item 1", Style::new());
        }
        3 => {
            buffer.set_string(0, 4, "list item 2", Style::new());
        }
        4 => {
            buffer.set_string(0, 5, "status: ok", Style::new());
        }
        _ => {}
    }
}

/// The full logical screen after `last` widgets: what a real render
/// closure paints every frame (immediate-mode contract).
fn paint_screen(frame: &mut tuvix_core::terminal::Frame<'_>, last: usize) {
    for step in 0..=last.min(4) {
        paint_widget(frame, step);
    }
}

/// Runs the five-widget sequence; `abort_at` abandons that step mid-way
/// (after painting extra widgets), then re-renders the step from a fresh
/// pass — exactly what the scheduler does after preempting input.
/// Returns the terminal's final screen.
fn run_sequence(persistent: bool, abort_at: Option<usize>) -> String {
    let backend = TestBackend::new(Size::new(W, H));
    let mut terminal = if persistent {
        Terminal::new_persistent(backend).unwrap()
    } else {
        Terminal::new(backend).unwrap()
    };

    for step in 0..5usize {
        if abort_at.is_none_or(|abort| abort != step) {
            // Uninterrupted step: the classic path.
            if persistent {
                terminal
                    .draw_persistent(|frame| paint_screen(frame, step))
                    .unwrap();
            } else {
                terminal.draw(|frame| paint_screen(frame, step)).unwrap();
            }
            continue;
        }

        let token = CancellationToken::new();
        let pass = token.child();
        let outcome = if persistent {
            terminal
                .draw_persistent_cancellable(&pass, |frame| {
                    paint_screen(frame, step);
                    // Extra widgets painted before the pass is abandoned
                    // between stage boundaries.
                    paint_widget(frame, 3);
                    paint_widget(frame, 4);
                    if let Some(stage) = frame.cancellation() {
                        stage.cancel();
                    }
                })
                .unwrap()
        } else {
            terminal
                .draw_cancellable(&pass, |frame| {
                    paint_screen(frame, step);
                    paint_widget(frame, 3);
                    paint_widget(frame, 4);
                    if let Some(stage) = frame.cancellation() {
                        stage.cancel();
                    }
                })
                .unwrap()
        };
        assert_eq!(outcome, DrawOutcome::Cancelled, "step {step} must abort");

        // Input was handled; the scheduler schedules a fresh full render.
        if persistent {
            terminal
                .draw_persistent(|frame| paint_screen(frame, step))
                .unwrap();
        } else {
            terminal.draw(|frame| paint_screen(frame, step)).unwrap();
        }
    }

    terminal.backend().buffer().to_string()
}

#[test]
fn interrupted_immediate_sequence_ends_where_the_uninterrupted_one_ends() {
    let clean = run_sequence(false, None);
    for abort_at in 0..5 {
        assert_eq!(
            run_sequence(false, Some(abort_at)),
            clean,
            "aborting step {abort_at} must not change the final screen"
        );
    }
}

#[test]
fn interrupted_persistent_sequence_ends_where_the_uninterrupted_one_ends() {
    let clean = run_sequence(true, None);
    for abort_at in 0..5 {
        assert_eq!(
            run_sequence(true, Some(abort_at)),
            clean,
            "aborting step {abort_at} must not change the final screen (persistent DOM)"
        );
    }
}

#[test]
fn committed_cancellable_persistent_output_matches_the_classic_path_frame_by_frame() {
    let backend_classic = TestBackend::new(Size::new(W, H));
    let backend_cancel = TestBackend::new(Size::new(W, H));
    let mut classic = Terminal::new(backend_classic).unwrap();
    let mut cancellable = Terminal::new_persistent(backend_cancel).unwrap();

    let token = CancellationToken::new();
    let pass = token.child();
    for step in 0..5usize {
        classic.draw(|frame| paint_screen(frame, step)).unwrap();
        let outcome = cancellable
            .draw_persistent_cancellable(&pass, |frame| paint_screen(frame, step))
            .unwrap();
        assert_eq!(outcome, DrawOutcome::Committed);
        assert_eq!(
            classic.backend().buffer().to_string(),
            cancellable.backend().buffer().to_string(),
            "after step {step}: a committed cancellable frame must be byte-identical to the classic path"
        );
    }
}

#[test]
fn an_abandoned_frame_between_committed_ones_is_invisible_on_the_wire() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(W, H))).unwrap();
    terminal
        .draw(|frame| {
            frame.buffer.set_string(0, 0, "before", Style::new());
        })
        .unwrap();

    let token = CancellationToken::new();
    let outcome = terminal
        .draw_cancellable(&token, |frame| {
            frame
                .buffer
                .set_string(0, 0, "WILL BE DISCARDED", Style::new());
            frame.buffer.set_string(0, 2, "so will this", Style::new());
            token.cancel();
        })
        .unwrap();
    assert_eq!(outcome, DrawOutcome::Cancelled);
    assert_eq!(
        terminal.backend().buffer().to_string(),
        "before              \n                    \n                    \n                    \n                    \n                    "
    );
}
