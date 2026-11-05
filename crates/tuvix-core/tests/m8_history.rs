//! M8 integration: the time-travel history.
//!
//! The [`HistorySink`](tuvix_core::preview::HistorySink) records the exact
//! byte snapshot of every flushed screen. The differential property proven
//! here mirrors the M4 journal contract: for the same paint sequence, the
//! persistent-DOM path's recorded history is byte-identical, frame for
//! frame, to the immediate-mode path's — time travel shows the same truth
//! no matter which rendering contract produced it.

use tuvix_core::backend::TestBackend;
use tuvix_core::geometry::Size;
use tuvix_core::preview::HistorySink;
use tuvix_core::scheduler::{Scheduler, TickOutcome};
use tuvix_core::style::Style;
use tuvix_core::terminal::Terminal;

const W: u16 = 20;
const H: u16 = 4;

/// A three-widget dashboard whose state changes per step; every frame
/// repaints the full logical screen (the immediate-mode contract).
fn paint_dashboard(frame: &mut tuvix_core::terminal::Frame<'_>, step: usize) {
    frame
        .buffer
        .set_string(0, 0, "CPU SCHEDULER v0.9", Style::new());
    let load = format!("load: {step}0%");
    frame.buffer.set_string(0, 1, load.as_str(), Style::new());
    if step.is_multiple_of(2) {
        frame.buffer.set_string(0, 3, "state: even", Style::new());
    } else {
        frame.buffer.set_string(0, 3, "state: odd ", Style::new());
    }
}

#[test]
fn persistent_and_immediate_histories_are_byte_identical() {
    let mut immediate = Terminal::new(TestBackend::new(Size::new(W, H))).unwrap();
    let mut persistent = Terminal::new_persistent(TestBackend::new(Size::new(W, H))).unwrap();
    let classic_history = HistorySink::unbounded();
    let dom_history = HistorySink::unbounded();
    immediate.set_preview(classic_history.clone());
    persistent.set_preview(dom_history.clone());

    for step in 0..6usize {
        immediate
            .draw(|frame| paint_dashboard(frame, step))
            .unwrap();
        persistent
            .draw_persistent(|frame| paint_dashboard(frame, step))
            .unwrap();
    }

    assert_eq!(classic_history.len(), 6);
    assert_eq!(dom_history.len(), 6);
    for (frame_number, (classic, dom)) in classic_history
        .frames()
        .iter()
        .zip(dom_history.frames().iter())
        .enumerate()
    {
        assert_eq!(
            classic, dom,
            "frame {frame_number}: the persistent history must be byte-identical to the immediate one"
        );
    }
}

#[test]
fn scrubbing_reconstructs_every_screen_of_a_run() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(W, H))).unwrap();
    let history = HistorySink::unbounded();
    terminal.set_preview(history.clone());

    for step in 0..4usize {
        terminal.draw(|frame| paint_dashboard(frame, step)).unwrap();
    }

    assert_eq!(history.len(), 4);
    for (step, frame) in history.frames().iter().enumerate() {
        // Each retained snapshot carries that step's state and nothing later.
        assert!(frame.contains(&format!("load: {step}0%")), "frame {step}");
    }
    // The final frame is the newest screen.
    assert!(history.latest().unwrap().contains("state: odd "));
}

#[test]
fn the_scheduler_and_the_history_tell_one_coherent_story() {
    // A scheduler run recorded through the history: two coalesced render
    // storms yield exactly two frames, and idle ticks record nothing.
    let terminal = Terminal::new(TestBackend::new(Size::new(W, H))).unwrap();
    let mut scheduler = Scheduler::new(terminal);
    let history = HistorySink::unbounded();
    scheduler.terminal_mut().set_preview(history.clone());

    for tick in 0..2usize {
        for _ in 0..4 {
            scheduler.request_render(); // four state changes per storm
        }
        let step = tick;
        let outcome = scheduler
            .tick(|frame| paint_dashboard(frame, step))
            .unwrap();
        assert_eq!(outcome, TickOutcome::Rendered);
        // Idle ticks between storms must not appear in the history.
        assert_eq!(scheduler.tick(|_| {}).unwrap(), TickOutcome::Idle);
        assert_eq!(scheduler.tick(|_| {}).unwrap(), TickOutcome::Idle);
    }

    assert_eq!(scheduler.frames(), 2);
    assert_eq!(history.len(), 2);
    assert!(history.get(0).unwrap().contains("load: 00%"));
    assert!(history.get(1).unwrap().contains("load: 10%"));
}
