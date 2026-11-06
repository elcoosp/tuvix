//! M14 mouse benches: the pointer layer under measurement.
//!
//! Targets (docs/BENCHMARKS.md, the M14 scorecard): the hit-test decision
//! against the B17 shape — a 1,000-region dashboard map — plus hit-map
//! construction (what a frame pays to register its regions), one gesture
//! classification, and one mouse-map resolution. The B17 gate itself
//! lives in `tests/m14_mouse.rs` with the other `#[ignore]`d timing gates.

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::event::{MouseButton, MouseEvent, MouseEventKind};
use tuvix_core::geometry::{Position, Rect};
use tuvix_core::gesture::{Gesture, GestureConfig, GestureRecognizer};
use tuvix_core::hit::HitMap;
use tuvix_core::mousemap::{MouseClass, MouseMap, MousePattern};

/// The B17 map shape: a 40x25 grid of 5x2 tiles.
const REGIONS: u32 = 1_000;
/// The mouse map's binding count in the B17 shape.
const BINDINGS: usize = 32;

fn dashboard_hits() -> HitMap<u32> {
    let mut hits: HitMap<u32> = HitMap::new();
    for id in 0..REGIONS {
        let x = ((id % 40) * 5) as u16;
        let y = ((id / 40) * 2) as u16;
        hits.push(id, Rect::new(x, y, 5, 2));
    }
    hits
}

fn bound_map() -> MouseMap<u32, u32> {
    let mut map: MouseMap<u32, u32> = MouseMap::new();
    for id in 0..BINDINGS as u32 {
        map.bind(MousePattern::on(id, MouseClass::AnyClick), id);
    }
    map
}

fn click_event(x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: tuvix_core::event::KeyModifiers::empty(),
    }
}

fn bench_mouse(c: &mut Criterion) {
    let mut group = c.benchmark_group("mouse");
    let hits = dashboard_hits();
    let map = bound_map();
    let click = Gesture::Click {
        button: MouseButton::Left,
        position: Position::new(103, 51),
        clicks: 1,
    };

    // M1 — the B17 decision: hit-test + resolve, the whole per-click
    // cost an app pays outside its handlers.
    group.bench_function("m1_mouse_decision_1000_regions_32_bindings", |b| {
        b.iter(|| {
            let hit = black_box(&hits).hit_test(Position::new(103, 51));
            black_box(map.resolve(hit, &click))
        });
    });

    // M2 — registering the frame's regions: what a rebuilt-from-scratch
    // hit map costs per frame (1,000 pushes).
    group.throughput(Throughput::Elements(u64::from(REGIONS)));
    group.bench_function("m2_hit_map_register_1000_regions", |b| {
        b.iter(|| {
            let mut fresh: HitMap<u32> = HitMap::new();
            for id in 0..REGIONS {
                let x = ((id % 40) * 5) as u16;
                let y = ((id / 40) * 2) as u16;
                fresh.push(id, Rect::new(x, y, 5, 2));
            }
            black_box(fresh)
        });
    });
    group.throughput(Throughput::Elements(1));

    // M3 — one gesture classification: every mouse event the app
    // receives pays this.
    let mut recognizer = GestureRecognizer::new(GestureConfig::default());
    recognizer.feed(click_event(4, 4), Duration::from_millis(0));
    let release = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 4,
        row: 4,
        modifiers: tuvix_core::event::KeyModifiers::empty(),
    };
    group.bench_function("m3_gesture_recognize_release", |b| {
        b.iter(|| {
            let mut fresh = recognizer;
            black_box(fresh.feed(black_box(release), Duration::from_millis(40)))
        });
    });

    group.finish();
}

criterion_group!(benches, bench_mouse);
criterion_main!(benches);
