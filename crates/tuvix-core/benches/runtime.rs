//! M2 benchmarks: the runtime hot paths.
//!
//! The draw path is unchanged from M1 (see `gates.rs`); M2 adds the event
//! queue and the input-stack transitions as measured surfaces. Targets are
//! recorded in docs/BENCHMARKS.md.

use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tuvix_core::runtime::input::{InputMode, InputStack, KittyEnhancementFlags};
use tuvix_core::runtime::queue::event_channel;

fn key_event(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::empty()))
}

/// R1 batch size: events per send+drain iteration.
const BATCH: u64 = 1_000;

/// A no-op waker for poll-driven benchmarks.
#[derive(Debug)]
struct SpinWake(AtomicUsize);

impl Wake for SpinWake {
    fn wake(self: Arc<Self>) {
        let _ = self.0.fetch_add(1, Ordering::Relaxed);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let _ = self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn bench_runtime(c: &mut Criterion) {
    let mut group = c.benchmark_group("runtime");

    // R1 — queue throughput: send + drain 1,000 events through one channel.
    group.throughput(Throughput::Elements(BATCH));
    group.bench_function("r1_send_drain_1k", |b| {
        b.iter(|| {
            let (sender, mut stream) = event_channel();
            for code in 0..BATCH {
                let ch = char::from_u32((65 + (code % 26)) as u32).unwrap_or('a');
                black_box(sender.send(key_event(KeyCode::Char(ch))));
            }
            let mut received = 0_u64;
            while let Some(event) = stream.try_next() {
                black_box(event);
                received += 1;
            }
            assert_eq!(received, BATCH);
        });
    });

    // R2 — waker round trip: poll (registers the waker), send (wakes),
    // re-poll (ready). The per-event cost an async loop pays under load.
    group.throughput(Throughput::Elements(1));
    group.bench_function("r2_waker_roundtrip", |b| {
        let (sender, mut stream) = event_channel();
        let wake = Arc::new(SpinWake(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&wake));
        let mut cx = Context::from_waker(&waker);
        b.iter(|| {
            assert_eq!(stream.poll_next(&mut cx), Poll::Pending);
            black_box(sender.send(key_event(KeyCode::Enter)));
            assert_eq!(
                stream.poll_next(&mut cx),
                Poll::Ready(Some(key_event(KeyCode::Enter)))
            );
        });
    });

    // R3 — input stack: push four modes and restore them (cold-path cost
    // paid once per mode change, kept tiny on purpose).
    group.bench_function("r3_input_stack_push_restore", |b| {
        b.iter(|| {
            let mut stack = InputStack::new();
            black_box(stack.push(InputMode::FocusChange));
            black_box(stack.push(InputMode::BracketedPaste));
            black_box(stack.push(InputMode::KittyKeyboard(
                KittyEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
            )));
            black_box(stack.push(InputMode::MouseCapture));
            let restore = stack.restore();
            black_box(restore.disable_sequences().len());
        });
    });

    // R4 — single mouse-capture enable transition (sequence construction).
    group.bench_function("r4_mouse_enable_sequences", |b| {
        b.iter(|| {
            let sequences = InputMode::MouseCapture.enable_sequences();
            black_box(sequences.len());
        });
    });

    group.finish();
}

criterion_group!(benches, bench_runtime);
criterion_main!(benches);
