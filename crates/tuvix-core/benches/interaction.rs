//! Benchmarks for the M11 interaction primitives: the keymap matcher and
//! the focus ring. The input-dispatch budget these back is the B10
//! input-to-paint pipeline — a keymap step or a focus cycle must stay a
//! rounding error next to the ~295 us p99 that budget measured.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::event::{KeyCode, KeyEvent, KeyModifiers};
use tuvix_core::focus::FocusRing;
use tuvix_core::keymap::{KeyPattern, Keymap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Quit,
    GotoTop,
    DeleteWord,
    Save,
    Next,
    Previous,
}

fn dashboard_keymap() -> Keymap<Command> {
    let mut keymap = Keymap::new();
    // Singles.
    keymap.bind(&[KeyPattern::new(KeyCode::Char('c')).ctrl()], Command::Quit);
    keymap.bind(&[KeyPattern::new(KeyCode::Char('s')).ctrl()], Command::Save);
    keymap.bind(&[KeyPattern::new(KeyCode::Down)], Command::Next);
    keymap.bind(&[KeyPattern::new(KeyCode::Up)], Command::Previous);
    // Chords sharing a prefix.
    keymap.bind(
        &[
            KeyPattern::new(KeyCode::Char('g')),
            KeyPattern::new(KeyCode::Char('g')),
        ],
        Command::GotoTop,
    );
    keymap.bind(
        &[
            KeyPattern::new(KeyCode::Char('d')),
            KeyPattern::new(KeyCode::Char('d')),
        ],
        Command::DeleteWord,
    );
    keymap
}

fn bench_interaction_keymap(c: &mut Criterion) {
    let mut group = c.benchmark_group("interaction");
    group.throughput(Throughput::Elements(1));

    group.bench_function("keymap_single_key_step", |b| {
        let mut keymap = dashboard_keymap();
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::empty());
        b.iter(|| {
            black_box(keymap.step(black_box(&down)));
        });
    });

    // The chord walk: the two steps alternate between opening the prefix
    // (Pending) and completing it (Matched + reset).
    group.bench_function("keymap_chord_prefix_step", |b| {
        let mut keymap = dashboard_keymap();
        let g = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::empty());
        b.iter(|| {
            let outcome = keymap.step(black_box(&g));
            black_box(outcome);
        });
    });

    // The worst case at the root: an unbound key scanning every sibling.
    group.bench_function("keymap_miss_step", |b| {
        let mut keymap = dashboard_keymap();
        let x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty());
        b.iter(|| {
            black_box(keymap.step(black_box(&x)));
        });
    });

    group.finish();
}

fn bench_interaction_focus(c: &mut Criterion) {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Field {
        A,
        B,
        C,
        D,
        E,
        F,
        G,
        H,
    }

    let mut group = c.benchmark_group("interaction");
    group.throughput(Throughput::Elements(1));

    group.bench_function("focus_cycle_8_fields", |b| {
        let mut ring = FocusRing::with_items([
            Field::A,
            Field::B,
            Field::C,
            Field::D,
            Field::E,
            Field::F,
            Field::G,
            Field::H,
        ]);
        b.iter(|| {
            black_box(ring.advance());
        });
    });

    group.finish();
}

criterion_group!(
    interaction,
    bench_interaction_keymap,
    bench_interaction_focus
);
criterion_main!(interaction);
