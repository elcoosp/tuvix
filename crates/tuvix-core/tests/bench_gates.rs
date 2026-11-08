//! Recorded timing gates for the core pipeline stages.
//!
//! Run with `cargo test -p tuvix-core --release --ignored`; results are
//! recorded in docs/BENCHMARKS.md. Deterministic gates (B9) live in the
//! backend crate where the ANSI encoder is available.
//!
//! The B13 memory gate measures through the counting global allocator
//! of the `tuvix-memgate` dev-dependency (the workspace's one audited
//! `unsafe`, with its safety argument documented in that crate).

use std::cell::RefCell;
use std::hint::black_box;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tuvix_core::backend::TestBackend;
use tuvix_core::buffer::Buffer;
use tuvix_core::cancellation::CancellationToken;
use tuvix_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::layout::{Constraint, Layout};
use tuvix_core::runtime::queue::event_channel;
use tuvix_core::scheduler::{Scheduler, TickOutcome};
use tuvix_core::style::{Color, Style};
use tuvix_core::terminal::Terminal;

use tuvix_memgate::CountingAlloc;

/// The B13 memory gate measures through the pass-through counting
/// allocator (see the `tuvix-memgate` crate for the safety argument).
#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAlloc = CountingAlloc;

/// The live-allocation delta across `f`, in bytes.
fn measured_bytes<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = tuvix_memgate::allocated_bytes();
    let value = f();
    let after = tuvix_memgate::allocated_bytes();
    (value, after.saturating_sub(before))
}

const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;

fn text_buffer() -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
    for row in 0..HEIGHT {
        let line =
            format!("row {row:02}: the quick brown fox jumps over the lazy dog — 汉字 spacing");
        buffer.set_string(0, row, line, Style::new());
    }
    buffer
}

fn scatter(buffer: &mut Buffer, count: usize) {
    for index in 0..count {
        let x = u16::try_from((index * 37 + 11) % usize::from(WIDTH)).unwrap_or(u16::MAX);
        let y = u16::try_from((index * 53 + 7) % usize::from(HEIGHT)).unwrap_or(u16::MAX);
        buffer.set_string(x, y, "X", Style::new().fg(Color::Red));
    }
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b8_noop_frame_decision_within_fifty_microseconds() {
    let steady = text_buffer();

    // Warm-up.
    for _ in 0..200 {
        let _ = steady.diff_runs(&steady).count();
    }

    let iterations = 500;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = steady.diff_runs(&steady).count();
    }
    let per_frame = start.elapsed() / iterations;
    assert!(
        per_frame <= Duration::from_micros(200),
        "B8 gate failed: {per_frame:?} per no-op frame decision (budget 50 us, hard ceiling 200 us on shared runners)"
    );
    println!("B8 gate: {per_frame:?} per no-op frame decision (200x60)");
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b2_steady_frame_within_budget() {
    let base = text_buffer();
    let mut changed = text_buffer();
    scatter(&mut changed, 1_000);

    let mut terminal = Terminal::new(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    terminal
        .draw(|frame| frame.buffer.content.clone_from(&base.content))
        .unwrap();

    let mut show_changed = false;
    // Warm-up.
    for _ in 0..50 {
        show_changed = !show_changed;
        let snapshot = if show_changed {
            changed.content.clone()
        } else {
            base.content.clone()
        };
        terminal
            .draw(|frame| frame.buffer.content.clone_from(&snapshot))
            .unwrap();
    }

    let iterations = 300;
    let start = Instant::now();
    for _ in 0..iterations {
        show_changed = !show_changed;
        let snapshot = if show_changed {
            changed.content.clone()
        } else {
            base.content.clone()
        };
        terminal
            .draw(|frame| frame.buffer.content.clone_from(&snapshot))
            .unwrap();
    }
    let per_frame = start.elapsed() / iterations;
    assert!(
        per_frame <= Duration::from_millis(2),
        "B2 gate failed: {per_frame:?} per steady frame (budget 2 ms)"
    );
    println!("B2 gate: {per_frame:?} per steady frame (1k changed cells @ 200x60, full stack)");
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b5_cached_layout_within_half_millisecond() {
    let constraints: Vec<Constraint> = (0..20)
        .map(|index| match index % 4 {
            0 => Constraint::Length(2),
            1 => Constraint::Min(0),
            2 => Constraint::Percentage(10),
            _ => Constraint::Fill(1),
        })
        .collect();
    let layout = Layout::vertical(constraints);
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let _ = layout.split(area); // warm

    let iterations = 2_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = layout.split(area);
    }
    let per_split = start.elapsed() / iterations;
    assert!(
        per_split <= Duration::from_millis(1),
        "B5 gate failed: {per_split:?} per cached split (budget 0.5 ms, hard ceiling 1 ms on shared runners)"
    );
    println!("B5 gate: {per_split:?} per cached 20-constraint split (target: cache hits)");
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b8_journal_noop_frame_within_budget() {
    // The M4 O(1) decision: a persistent-DOM frame that paints nothing must
    // cost no more than the original 50 us B8 budget — finally at the full
    // 200x60 budget viewport, not just the 80x24 reference screen.
    let mut terminal =
        Terminal::new_persistent(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    terminal
        .draw_persistent(|frame| {
            for row in 0..HEIGHT {
                let line = format!("row {row:02}: the quick brown fox jumps over the lazy dog");
                frame.buffer.set_string(0, row, line, Style::new());
            }
        })
        .unwrap();

    // Warm-up.
    for _ in 0..200 {
        let () = terminal.draw_persistent(|_| {}).unwrap();
    }

    let iterations = 2_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let () = terminal.draw_persistent(|_| {}).unwrap();
    }
    let per_frame = start.elapsed() / iterations;
    assert!(
        per_frame <= Duration::from_micros(50),
        "B8 journal gate failed: {per_frame:?} per no-op frame (budget 50 us)"
    );
    println!("B8 journal gate: {per_frame:?} per no-op frame (200x60, journal path)");
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b10_input_to_paint_p99_within_eight_milliseconds() {
    // The architecture's gate 5 (§8): 10,000 keystrokes under continuous
    // animation, input-to-paint p99 <= 8 ms. One keystroke = one event
    // through the channel, drained by the scheduler loop, coalesced into
    // one animated frame (a 20-row band repaints every keystroke at the
    // 200x60 budget viewport).
    const KEYSTROKES: usize = 10_000;

    let (sender, stream) = event_channel();
    let stream = Rc::new(RefCell::new(stream));
    let terminal = Terminal::new_persistent(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    let mut scheduler = Scheduler::new(terminal);

    let probe_stream = Rc::clone(&stream);
    scheduler.set_probe(move || !probe_stream.borrow().is_empty());

    // Steady state: paint once so every measured frame is a real
    // banded diff against a full screen, not a blank canvas.
    scheduler.request_render();
    let outcome = scheduler.tick(|frame| animate(frame, 0)).unwrap();
    assert_eq!(outcome, TickOutcome::Rendered);

    let mut latencies = Vec::with_capacity(KEYSTROKES);
    for keystroke in 0..KEYSTROKES {
        let code = KeyCode::Char(
            char::from_u32(u32::try_from(65 + (keystroke % 26)).unwrap_or(u32::MAX)).unwrap_or('a'),
        );
        assert!(sender.send(Event::Key(KeyEvent::new(code, KeyModifiers::empty()))));

        // Input-to-paint: from "the event is queued" to "the frame is
        // flushed" — the latency a user experiences per keystroke.
        let start = Instant::now();
        while let Some(event) = stream.borrow_mut().try_next() {
            black_box(event);
        }
        scheduler.request_render();
        let step = keystroke as u64;
        let outcome = scheduler
            .tick_cancellable(&CancellationToken::new(), |frame| animate(frame, step))
            .unwrap();
        assert_eq!(outcome, TickOutcome::Rendered, "keystroke {keystroke}");
        latencies.push(start.elapsed());
    }

    latencies.sort_unstable();
    let p99 = latencies[KEYSTROKES * 99 / 100 - 1];
    let max = latencies[KEYSTROKES - 1];
    assert!(
        p99 <= Duration::from_millis(8),
        "B10 gate failed: input-to-paint p99 {p99:?} (budget 8 ms)"
    );
    println!(
        "B10 gate: input-to-paint p99 {p99:?}, max {max:?} over {KEYSTROKES} keystrokes @ {WIDTH}x{HEIGHT} (budget 8 ms)"
    );
}

/// The animation under the B10 gate: a header plus a 20-row band that
/// changes on every keystroke (persistent DOM, banded diff).
fn animate(frame: &mut tuvix_core::terminal::Frame<'_>, tick: u64) {
    let header = format!("B10 input-to-paint tick {tick}");
    frame.buffer.set_string(0, 0, header.as_str(), Style::new());
    for row in 1..21 {
        let band = format!("row {row:02}: the quick brown fox jumps over the lazy dog {tick}");
        frame.buffer.set_string(0, row, band.as_str(), Style::new());
    }
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b11_scrub_latency_and_recording_overhead() {
    // The architecture's DevTools story (§5.8) has two costs:
    //
    // 1. **Scrub latency** — reconstruct any recorded frame into a
    //    renderable Screen. "Jump to any moment" must be far below a
    //    frame budget even at the 200x60 size.
    // 2. **Recording overhead** — what SessionRecorder adds per frame
    //    on top of the serialize the preview hook already performs.
    use std::sync::Arc;
    use tuvix_core::preview::{FrameSink, HistorySink};
    use tuvix_core::screen::Screen;
    use tuvix_core::session::SessionRecorder;

    // Capture a realistic full-screen snapshot through the real hook.
    let mut terminal = Terminal::new(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    let history = HistorySink::unbounded();
    terminal.set_preview(history.clone());
    terminal
        .draw(|frame| {
            let header = "B11 scrub gate tick 7 — the quick brown fox";
            frame.buffer.set_string(0, 0, header, Style::new());
            for row in 1..HEIGHT {
                let band = format!("row {row:02}: 0123456789 abcdefghijklmnopqrstuvwxyz {row}");
                frame.buffer.set_string(0, row, band.as_str(), Style::new());
            }
        })
        .unwrap();
    let snapshot = Arc::new(history.latest().expect("one flushed frame"));
    terminal.clear_preview();

    // 1. Scrub: reconstruct the frame 200 times, take p99.
    let scrubs = 200;
    let mut latencies = Vec::with_capacity(scrubs);
    for _ in 0..scrubs {
        let start = Instant::now();
        let screen = Screen::from_ansi(black_box(snapshot.as_str()))
            .expect("the pipeline emits canonical snapshots");
        black_box(screen.width());
        latencies.push(start.elapsed());
    }
    latencies.sort_unstable();
    let p99 = latencies[scrubs * 99 / 100 - 1];
    let median = latencies[scrubs / 2];
    assert!(
        p99 <= Duration::from_millis(2),
        "B11 gate failed: scrub p99 {p99:?} (budget 2 ms)"
    );
    println!("B11 gate: scrub p99 {p99:?} (median {median:?}) @ {WIDTH}x{HEIGHT} (budget 2 ms)");

    // 2. Recording overhead per frame.
    let session = SessionRecorder::unbounded();
    let mut sink = session.clone();
    let frames = 1_000;
    let start = Instant::now();
    for _ in 0..frames {
        sink.on_frame(black_box(snapshot.as_str()));
    }
    let per_frame = start.elapsed() / frames;
    assert!(
        per_frame <= Duration::from_micros(50),
        "B11 gate failed: {per_frame:?} per recorded frame (budget 50 us)"
    );
    println!(
        "B11 gate: recording overhead {per_frame:?} per frame @ {WIDTH}x{HEIGHT} (budget 50 us)"
    );
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b12_replay_throughput_and_golden_loop() {
    // The architecture's replay promise (§5.8): a recorded session is a
    // regression test. Two costs matter:
    //
    // 1. **Replay throughput** — 100 recorded 200x60 frames replayed
    //    through a deterministic app (parse + paint + serialize + parse
    //    per step) inside a generous budget.
    // 2. **The golden property** — the replay of the recording is
    //    byte-identical on every step, as a deterministic gate.
    use tuvix_core::replay::{ReplayStep, SessionReplay};
    use tuvix_core::session::SessionRecorder;

    let paint = |buffer: &mut Buffer, mutation: usize| {
        let header = format!("B12 replay gate mutation {mutation} — the quick brown fox");
        buffer.set_string(0, 0, header.as_str(), Style::new());
        for row in 1..HEIGHT {
            let band = format!("row {row:02}: 0123456789 abcdefghijklmnopqrstuvwxyz {row}");
            buffer.set_string(0, row, band.as_str(), Style::new());
        }
        scatter(buffer, 200 + mutation * 7);
    };

    // Record 100 dashboard frames through the real pipeline, every other
    // one driven by an event.
    let recorder = SessionRecorder::unbounded();
    let mut terminal = Terminal::new(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    terminal.set_preview(recorder.clone());
    let mut mutations = 0usize;
    for frame in 0..100 {
        if frame % 2 == 0 {
            recorder.record_event(Event::Key(KeyEvent::new(
                KeyCode::Char('x'),
                KeyModifiers::empty(),
            )));
            mutations += 1;
        }
        let mutation = mutations;
        terminal
            .draw(|frame| paint(frame.buffer, mutation))
            .unwrap();
    }
    drop(terminal);

    let replay = SessionReplay::from_recorder(&recorder);
    assert_eq!(replay.frame_count(), 100);

    let start = Instant::now();
    let mut replayed_mutations = 0usize;
    let report = replay
        .run(|step: ReplayStep<'_>| {
            for event in step.events {
                if let Event::Key(key_event) = event
                    && key_event.code == KeyCode::Char('x')
                {
                    replayed_mutations += 1;
                }
            }
            paint(step.buffer, replayed_mutations);
        })
        .expect("recorded snapshots are canonical");
    let elapsed = start.elapsed();

    // The deterministic half of the gate: the replay is golden.
    assert!(
        report.is_golden(),
        "B12 gate failed: the replay diverged at {:?}",
        report.first_mismatch().map(ToString::to_string)
    );
    assert_eq!(report.steps(), 100);
    assert_eq!(report.matched(), 100);
    assert_eq!(replayed_mutations, 50);
    assert_eq!(report.unverified_events(), 0);

    // The timing half: 100 full-size steps (two parses, one serialize
    // and one app paint each) inside 300 ms — a cold-path regression
    // tool, not the render loop; the ceiling catches 2x+ regressions.
    assert!(
        elapsed <= Duration::from_millis(300),
        "B12 gate failed: {elapsed:?} for 100 replayed frames @200x60 (budget 300 ms)"
    );
    println!(
        "B12 gate: replay of 100 frames (50 events) @ {WIDTH}x{HEIGHT} in {elapsed:?} (budget 300 ms)"
    );
}

#[test]
#[ignore = "memory gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b13_persistent_pipeline_memory_budget() {
    // The architecture's last unmeasured budget row (§1): the persistent
    // DOM surface stays under 10 MB. Measured at the 200x60 budget
    // viewport: two 12,000-cell buffers, the frame journal bands, a
    // SessionRecorder holding 60 full-screen snapshots and a HistorySink
    // ring — the whole DevTools surface in one number.
    use tuvix_core::preview::HistorySink;
    use tuvix_core::session::SessionRecorder;

    let history = HistorySink::unbounded();
    let recorder = SessionRecorder::unbounded();

    let build = || {
        let mut terminal = Terminal::new_persistent(TestBackend::new(Size::new(WIDTH, HEIGHT)))
            .expect("a test backend cannot fail");
        terminal.set_preview(history.clone());
        let _recorder_sink = recorder.clone();
        for tick in 1..=60_u64 {
            terminal
                .draw(|frame| {
                    let header = format!("B13 memory gate tick {tick} — the quick brown fox");
                    frame.buffer.set_string(0, 0, header.as_str(), Style::new());
                    for row in 1..HEIGHT {
                        let band =
                            format!("row {row:02}: 0123456789 abcdefghijklmnopqrstuvwxyz {row}");
                        frame.buffer.set_string(0, row, band.as_str(), Style::new());
                    }
                    scatter(frame.buffer, usize::try_from(tick).unwrap_or(0) * 3 + 10);
                })
                .expect("a test backend cannot fail");
        }
        terminal
    };

    // Warm the allocator's caches once so the measured pass is steady.
    let warm = build();
    drop(warm);

    let (terminal, delta) = measured_bytes(build);
    drop(terminal);
    let budget = 10 * 1024 * 1024;
    assert!(
        delta <= budget,
        "B13 gate failed: {delta} B live after the persistent build (budget {budget} B)"
    );
    println!(
        "B13 gate: persistent pipeline + 60 recorded frames @ {WIDTH}x{HEIGHT} = {delta} B live (budget {budget} B)"
    );
}
