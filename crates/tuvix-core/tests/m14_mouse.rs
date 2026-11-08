//! M14 mouse tests: the hit map, the gesture recognizer and the mouse
//! map — the framework surface the mouse events never had.
//!
//! Semantics pinned here (mirrored into docs/ARCHITECTURE.md §21):
//!
//! - the `HitMap` resolves a click the way the painter painted it: the
//!   topmost (last registered) region containing the position wins, local
//!   coordinates are relative to the region origin, and `push_clipped`
//!   applies the buffer's clip discipline (paint the part that lands
//!   inside, register nothing for an empty intersection);
//! - the `GestureRecognizer` classifies raw mouse events on an injected
//!   clock — clicks count on release-to-release within the interval at the
//!   same cell, drags start when a held button moves to a different cell,
//!   and scroll/move pass through without disturbing press state;
//! - the `MouseMap` is the `Keymap`'s mouse twin: patterns resolve to
//!   commands, later bindings shadow earlier ones (the painter's-algorithm
//!   intuition, one rule shared with the `HitMap`), and resolution is
//!   stateless — a gesture is atomic, so there is no `Pending`.
//!
//! The B17 timing gate at the bottom is `#[ignore]`d: run it with
//! `cargo test -p tuvix-core --release --ignored` and record the numbers
//! in docs/BENCHMARKS.md.

use std::time::Duration;

use tuvix_core::event::{MouseButton, MouseEvent, MouseEventKind};
use tuvix_core::geometry::{Position, Rect};
use tuvix_core::gesture::{Gesture, GestureConfig, GestureRecognizer, ScrollKind};
use tuvix_core::hit::HitMap;
use tuvix_core::mousemap::{MouseClass, MouseMap, MousePattern};

const MS: u64 = 1_000_000;
fn ms(n: u64) -> Duration {
    Duration::from_nanos(n * MS)
}

fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: tuvix_core::event::KeyModifiers::empty(),
    }
}

// ---------------------------------------------------------------------------
// HitMap: the click surface as a pure, per-frame data structure.
// ---------------------------------------------------------------------------

#[test]
fn hit_test_resolves_the_topmost_region() {
    let mut hits: HitMap<&str> = HitMap::new();
    hits.push("backdrop", Rect::new(0, 0, 40, 20));
    hits.push("panel", Rect::new(5, 5, 10, 6));
    // Overlapping registration: the panel painted last wins the overlap.
    assert_eq!(
        hits.hit_test(Position::new(6, 6)).map(|hit| hit.id),
        Some("panel")
    );
    // Outside the panel but inside the backdrop.
    assert_eq!(
        hits.hit_test(Position::new(2, 2)).map(|hit| hit.id),
        Some("backdrop")
    );
    // Outside everything.
    assert_eq!(hits.hit_test(Position::new(50, 25)), None);
}

#[test]
fn local_coordinates_are_relative_to_the_region_origin() {
    let mut hits: HitMap<u8> = HitMap::new();
    hits.push(7, Rect::new(10, 20, 5, 5));
    let hit = hits.hit_test(Position::new(11, 22)).expect("inside");
    assert_eq!(hit.id, 7);
    assert_eq!(hit.area, Rect::new(10, 20, 5, 5));
    assert_eq!(hit.local, Position::new(1, 2));
}

#[test]
fn region_edges_are_inclusive_on_every_side() {
    let mut hits: HitMap<u8> = HitMap::new();
    hits.push(1, Rect::new(2, 3, 4, 5));
    for (x, y) in [(2, 3), (5, 3), (2, 7), (5, 7), (3, 5)] {
        assert!(hits.hit_test(Position::new(x, y)).is_some(), "{x},{y}");
    }
    for (x, y) in [(1, 3), (6, 3), (2, 2), (2, 8)] {
        assert!(hits.hit_test(Position::new(x, y)).is_none(), "{x},{y}");
    }
}

#[test]
fn push_clipped_intersects_and_skips_empty() {
    let clip = Rect::new(0, 0, 20, 10);
    let mut hits: HitMap<&str> = HitMap::new();
    // Fully inside: registered as-is.
    hits.push_clipped("inner", Rect::new(1, 1, 4, 4), clip);
    assert_eq!(hits.len(), 1);
    // Half outside: only the inside part is registered — the same
    // discipline the applier (M12) and every paint pass follow.
    hits.push_clipped("edge", Rect::new(18, 8, 10, 10), clip);
    assert_eq!(hits.len(), 2);
    let hit = hits.hit_test(Position::new(19, 9)).expect("clipped in");
    assert_eq!(hit.id, "edge");
    assert_eq!(hit.area, Rect::new(18, 8, 2, 2));
    // Fully outside: registered nothing.
    hits.push_clipped("ghost", Rect::new(100, 100, 5, 5), clip);
    assert_eq!(hits.len(), 2);
    assert!(hits.hit_test(Position::new(101, 101)).is_none());
}

#[test]
fn zero_sized_regions_are_never_hit() {
    let mut hits: HitMap<u8> = HitMap::new();
    hits.push(1, Rect::ZERO);
    hits.push(2, Rect::new(0, 0, 0, 5));
    hits.push(3, Rect::new(0, 0, 5, 0));
    assert_eq!(hits.len(), 3, "push registers what the app declared");
    assert_eq!(hits.hit_test(Position::new(0, 0)), None);
}

#[test]
fn clear_reuses_the_map_across_frames() {
    let mut hits: HitMap<u8> = HitMap::new();
    hits.push(1, Rect::new(0, 0, 4, 4));
    hits.clear();
    assert!(hits.is_empty());
    assert_eq!(hits.len(), 0);
    hits.push(2, Rect::new(1, 1, 2, 2));
    assert_eq!(hits.hit_test(Position::new(0, 0)), None);
    assert_eq!(
        hits.hit_test(Position::new(1, 1)).map(|hit| hit.id),
        Some(2)
    );
}

/// An independent brute-force resolver: scan the registered regions in
/// reverse (the painter's algorithm) with plain `Rect::contains`. The
/// randomized differential test proves `hit_test` and this agree on
/// every position, clipped or not.
fn brute_force<ID: Copy + PartialEq>(regions: &[(ID, Rect)], at: Position) -> Option<ID> {
    regions
        .iter()
        .rev()
        .find(|(_, area)| area.contains(at))
        .map(|(id, _)| *id)
}

#[test]
fn hit_map_is_differential_sound_against_brute_force() {
    // A deterministic LCG (the same generator the diff-soundness suite
    // uses) drives 2,000 mixed steps: register plain, register clipped,
    // clear, and probe random positions.
    let mut state: u64 = 0x5EED_1234_ABCD_0002;
    let mut next = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) as u32
    };

    let mut hits: HitMap<u32> = HitMap::new();
    let mut reference: Vec<(u32, Rect)> = Vec::new();
    for step in 0..2_000u32 {
        match next() % 10 {
            0..=4 => {
                let id = next() % 32;
                let area = Rect::new(
                    (next() % 60) as u16,
                    (next() % 40) as u16,
                    (next() % 25) as u16,
                    (next() % 18) as u16,
                );
                hits.push(id, area);
                reference.push((id, area));
            }
            5..=6 => {
                let id = next() % 32;
                let area = Rect::new(
                    (next() % 60) as u16,
                    (next() % 40) as u16,
                    (next() % 25) as u16,
                    (next() % 18) as u16,
                );
                let clip = Rect::new(
                    (next() % 30) as u16,
                    (next() % 20) as u16,
                    (next() % 40) as u16,
                    (next() % 30) as u16,
                );
                hits.push_clipped(id, area, clip);
                if let Some(clipped) = area.intersection(clip)
                    && !clipped.is_empty()
                {
                    reference.push((id, clipped));
                }
            }
            7 => {
                hits.clear();
                reference.clear();
            }
            _ => {
                let at = Position::new((next() % 90) as u16, (next() % 60) as u16);
                assert_eq!(
                    hits.hit_test(at).map(|hit| hit.id),
                    brute_force(&reference, at),
                    "step {step} at {at}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GestureRecognizer: raw mouse events classified on an injected clock.
// ---------------------------------------------------------------------------

const LEFT: MouseButton = MouseButton::Left;
const RIGHT: MouseButton = MouseButton::Right;

/// A fresh recognizer with the default 500 ms double-click interval.
fn recognizer() -> GestureRecognizer {
    GestureRecognizer::new(GestureConfig::default())
}

#[test]
fn down_then_up_is_a_single_click() {
    let mut gestures = recognizer();
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 5), ms(0)),
        Some(Gesture::Press {
            button: LEFT,
            position: Position::new(4, 5)
        })
    );
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::Up(LEFT), 4, 5), ms(60)),
        Some(Gesture::Click {
            button: LEFT,
            position: Position::new(4, 5),
            clicks: 1
        })
    );
}

#[test]
fn quick_release_in_the_same_cell_doubles_then_triples() {
    let mut gestures = recognizer();
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 5), ms(0));
    gestures.feed(mouse(MouseEventKind::Up(LEFT), 4, 5), ms(40));
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 5), ms(80));
    let second = gestures.feed(mouse(MouseEventKind::Up(LEFT), 4, 5), ms(120));
    assert_eq!(
        second,
        Some(Gesture::Click {
            button: LEFT,
            position: Position::new(4, 5),
            clicks: 2
        })
    );
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 5), ms(160));
    let third = gestures.feed(mouse(MouseEventKind::Up(LEFT), 4, 5), ms(200));
    assert_eq!(
        third,
        Some(Gesture::Click {
            button: LEFT,
            position: Position::new(4, 5),
            clicks: 3
        })
    );
}

#[test]
fn release_outside_the_interval_resets_to_single() {
    let mut gestures = recognizer();
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 5), ms(0));
    gestures.feed(mouse(MouseEventKind::Up(LEFT), 4, 5), ms(40));
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 5), ms(600));
    let late = gestures.feed(mouse(MouseEventKind::Up(LEFT), 4, 5), ms(640));
    assert_eq!(
        late,
        Some(Gesture::Click {
            button: LEFT,
            position: Position::new(4, 5),
            clicks: 1
        })
    );
}

#[test]
fn click_on_a_different_cell_resets_to_single() {
    let mut gestures = recognizer();
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 5), ms(0));
    gestures.feed(mouse(MouseEventKind::Up(LEFT), 4, 5), ms(40));
    // The user's aim drifted one cell: a fresh single click, not a double.
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 5, 5), ms(80));
    let moved = gestures.feed(mouse(MouseEventKind::Up(LEFT), 5, 5), ms(120));
    assert_eq!(
        moved,
        Some(Gesture::Click {
            button: LEFT,
            position: Position::new(5, 5),
            clicks: 1
        })
    );
}

#[test]
fn a_different_button_resets_the_multiplicity() {
    let mut gestures = recognizer();
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 5), ms(0));
    gestures.feed(mouse(MouseEventKind::Up(LEFT), 4, 5), ms(40));
    gestures.feed(mouse(MouseEventKind::Down(RIGHT), 4, 5), ms(80));
    let right = gestures.feed(mouse(MouseEventKind::Up(RIGHT), 4, 5), ms(120));
    assert_eq!(
        right,
        Some(Gesture::Click {
            button: RIGHT,
            position: Position::new(4, 5),
            clicks: 1
        })
    );
}

#[test]
fn drags_stream_start_move_end() {
    let mut gestures = recognizer();
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 10, 10), ms(0));
    // Same-cell wobble is not a drag yet.
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::Drag(LEFT), 10, 10), ms(20)),
        None
    );
    // The first different cell opens the drag at the press origin.
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::Drag(LEFT), 12, 10), ms(40)),
        Some(Gesture::DragStart {
            button: LEFT,
            origin: Position::new(10, 10)
        })
    );
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::Drag(LEFT), 15, 12), ms(60)),
        Some(Gesture::Drag {
            button: LEFT,
            origin: Position::new(10, 10),
            position: Position::new(15, 12)
        })
    );
    // Release ends the drag — it is not a click.
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::Up(LEFT), 15, 12), ms(80)),
        Some(Gesture::DragEnd {
            button: LEFT,
            origin: Position::new(10, 10),
            position: Position::new(15, 12)
        })
    );
}

#[test]
fn a_click_after_a_drag_is_a_fresh_single_click() {
    let mut gestures = recognizer();
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 10, 10), ms(0));
    gestures.feed(mouse(MouseEventKind::Drag(LEFT), 12, 10), ms(20));
    gestures.feed(mouse(MouseEventKind::Up(LEFT), 12, 10), ms(40));
    // Double-click timing and the same cell would have doubled a click;
    // after a drag the multiplicity restarts.
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 12, 10), ms(80));
    let after = gestures.feed(mouse(MouseEventKind::Up(LEFT), 12, 10), ms(120));
    assert_eq!(
        after,
        Some(Gesture::Click {
            button: LEFT,
            position: Position::new(12, 10),
            clicks: 1
        })
    );
}

#[test]
fn unmatched_release_and_drag_are_ignored() {
    let mut gestures = recognizer();
    // The terminal may start reporting mid-press: an Up with no Down.
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::Up(LEFT), 1, 1), ms(0)),
        None
    );
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::Drag(LEFT), 2, 1), ms(20)),
        None
    );
    // An Up for a button that is not the pressed one is likewise ignored.
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 1, 1), ms(40));
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::Up(RIGHT), 1, 1), ms(60)),
        None
    );
    assert!(gestures.is_pressed(), "the real press stays active");
}

#[test]
fn a_second_down_replaces_the_active_press() {
    let mut gestures = recognizer();
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 1, 1), ms(0));
    gestures.feed(mouse(MouseEventKind::Drag(LEFT), 5, 1), ms(20));
    // Out-of-order terminals: a fresh Down is the truth on the table.
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::Down(RIGHT), 7, 7), ms(40)),
        Some(Gesture::Press {
            button: RIGHT,
            position: Position::new(7, 7)
        })
    );
    assert!(!gestures.is_dragging(), "the abandoned drag state is gone");
    // The release that arrives belongs to the new press: a clean click.
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::Up(RIGHT), 7, 7), ms(80)),
        Some(Gesture::Click {
            button: RIGHT,
            position: Position::new(7, 7),
            clicks: 1
        })
    );
}

#[test]
fn scroll_and_move_pass_through_without_disturbing_the_press() {
    let mut gestures = recognizer();
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 4), ms(0));
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::ScrollDown, 4, 4), ms(20)),
        Some(Gesture::Scroll {
            kind: ScrollKind::Down,
            position: Position::new(4, 4)
        })
    );
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::ScrollUp, 4, 4), ms(40)),
        Some(Gesture::Scroll {
            kind: ScrollKind::Up,
            position: Position::new(4, 4)
        })
    );
    assert!(gestures.is_pressed(), "scrolling never releases a press");
    // The click still completes after the scroll interlude.
    assert!(matches!(
        gestures.feed(mouse(MouseEventKind::Up(LEFT), 4, 4), ms(60)),
        Some(Gesture::Click { clicks: 1, .. })
    ));
    // And a plain move (no buttons) reports as a move.
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::Moved, 9, 9), ms(80)),
        Some(Gesture::Moved {
            position: Position::new(9, 9)
        })
    );
}

#[test]
fn motion_while_pressed_follows_the_drag_rules() {
    let mut gestures = recognizer();
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 4), ms(0));
    // Some terminals report plain Moved while a button is held; the
    // classifier treats it as drag motion all the same.
    assert_eq!(
        gestures.feed(mouse(MouseEventKind::Moved, 6, 4), ms(20)),
        Some(Gesture::DragStart {
            button: LEFT,
            origin: Position::new(4, 4)
        })
    );
}

#[test]
fn reset_clears_press_and_click_state() {
    let mut gestures = recognizer();
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 4), ms(0));
    gestures.feed(mouse(MouseEventKind::Up(LEFT), 4, 4), ms(40));
    gestures.reset();
    assert!(!gestures.is_pressed());
    // Within what would have been the double-click window, the reset
    // already forgot the first click: a fresh single.
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 4), ms(80));
    assert!(matches!(
        gestures.feed(mouse(MouseEventKind::Up(LEFT), 4, 4), ms(120)),
        Some(Gesture::Click { clicks: 1, .. })
    ));
}

#[test]
fn the_interval_is_configurable() {
    let mut gestures = GestureRecognizer::new(GestureConfig::new(ms(100)));
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 4), ms(0));
    gestures.feed(mouse(MouseEventKind::Up(LEFT), 4, 4), ms(40));
    gestures.feed(mouse(MouseEventKind::Down(LEFT), 4, 4), ms(150));
    let late = gestures.feed(mouse(MouseEventKind::Up(LEFT), 4, 4), ms(150));
    // 110 ms separate the releases: outside the configured 100 ms window,
    // so this stays a single click.
    assert!(matches!(late, Some(Gesture::Click { clicks: 1, .. })));
}

/// The noise-invariance differential: inert noise — scroll events always,
/// plain motion only while no button is held — interleaved into a
/// press-release-drag stream must never change the classification of the
/// meaningful events. Motion while a button is held is *not* noise (it is
/// drag input by contract), so the test tracks the stream's press state
/// before injecting it. A ten-minute double-click window keeps the
/// timestamp shift the noise causes out of the classification.
#[test]
fn classification_is_invariant_under_inert_noise() {
    let mut state: u64 = 0x5EED_1234_ABCD_0003;
    let mut next = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) as u32
    };

    for _ in 0..300 {
        // A random meaningful stream of press/(drag*)/release triples...
        let mut meaningful: Vec<MouseEvent> = Vec::new();
        let mut pressed_cells: Vec<bool> = Vec::new(); // press state per index
        let presses = (next() % 4 + 1) as usize;
        for _ in 0..presses {
            let button = match next() % 3 {
                0 => LEFT,
                1 => RIGHT,
                _ => MouseButton::Middle,
            };
            let x = (next() % 60) as u16;
            let y = (next() % 30) as u16;
            pressed_cells.push(true);
            meaningful.push(mouse(MouseEventKind::Down(button), x, y));
            let steps = next() % 3;
            for _ in 0..steps {
                let nx = x + (next() % 4) as u16;
                let ny = y + (next() % 4) as u16;
                meaningful.push(mouse(MouseEventKind::Drag(button), nx, ny));
                pressed_cells.push(true);
            }
            meaningful.push(mouse(MouseEventKind::Up(button), x, y));
            pressed_cells.push(false);
        }

        // ...fed clean, and fed with inert noise interleaved, must
        // classify the meaningful events identically.
        let clean = classify(&meaningful);
        let mut noisy = Vec::new();
        for (index, event) in meaningful.iter().enumerate() {
            // The press state the recognizer is in when this noise fires
            // is the state after the *previous* meaningful event.
            let held = index > 0 && pressed_cells[index - 1];
            if next() % 3 == 0 {
                let kind = match (next() % 3, held) {
                    (0, false) | (1, _) => MouseEventKind::ScrollDown,
                    (2, false) => MouseEventKind::Moved,
                    (_, _) => MouseEventKind::ScrollUp,
                };
                noisy.push(mouse(kind, (next() % 60) as u16, (next() % 30) as u16));
            }
            noisy.push(*event);
        }
        let noisy_class = classify(&noisy);
        if clean != noisy_class {
            eprintln!("meaningful: {meaningful:?}");
            eprintln!("noisy:      {noisy:?}");
            eprintln!("clean:      {clean:?}");
            eprintln!("noisy out:  {noisy_class:?}");
            panic!("classification diverged under inert noise");
        }
    }
}

/// Feeds the stream through a fresh recognizer with a ten-minute click
/// window, keeping only the non-noise gestures in order.
fn classify(stream: &[MouseEvent]) -> Vec<Gesture> {
    let mut gestures = GestureRecognizer::new(GestureConfig::new(ms(600_000)));
    let mut clock = 0u64;
    let mut out = Vec::new();
    for event in stream {
        clock += 25;
        if let Some(gesture) = gestures.feed(*event, ms(clock)) {
            let is_noise = matches!(gesture, Gesture::Scroll { .. } | Gesture::Moved { .. });
            if !is_noise {
                out.push(gesture);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// MouseMap: the declarative router — a pattern matched against a resolved
// hit and a classified gesture. Stateless by construction: a gesture is
// atomic, so unlike the Keymap there is no Pending outcome.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cmd {
    Focus,
    Open,
    Dismiss,
    Move,
    Drop,
    Scroll,
}

fn sample_map() -> MouseMap<&'static str, Cmd> {
    let mut map = MouseMap::new();
    map.bind(
        MousePattern::on("panel", MouseClass::Click).button(MouseButton::Left),
        Cmd::Focus,
    );
    map.bind(
        MousePattern::on("panel", MouseClass::DoubleClick),
        Cmd::Open,
    );
    map.bind(MousePattern::anywhere(MouseClass::Scroll), Cmd::Scroll);
    map.bind(
        MousePattern::on("backdrop", MouseClass::Click),
        Cmd::Dismiss,
    );
    map
}

#[test]
fn region_bindings_resolve_only_inside_their_region() {
    let map = sample_map();
    let mut hits: HitMap<&str> = HitMap::new();
    hits.push("backdrop", Rect::new(0, 0, 40, 20));
    hits.push("panel", Rect::new(5, 5, 10, 6));

    let click = Gesture::Click {
        button: MouseButton::Left,
        position: Position::new(6, 6),
        clicks: 1,
    };
    let hit = hits.hit_test(Position::new(6, 6));
    assert_eq!(map.resolve(hit, &click), Some(Cmd::Focus));

    // Outside the panel the same click lands on the backdrop.
    let hit = hits.hit_test(Position::new(2, 2));
    assert_eq!(map.resolve(hit, &click), Some(Cmd::Dismiss));
}

#[test]
fn anywhere_bindings_resolve_on_empty_space() {
    let map = sample_map();
    let scroll = Gesture::Scroll {
        kind: ScrollKind::Down,
        position: Position::new(50, 25),
    };
    // No region here: only the `anywhere` scroll binding applies.
    assert_eq!(map.resolve(None, &scroll), Some(Cmd::Scroll));
}

#[test]
fn later_bindings_shadow_earlier_ones() {
    let mut hits: HitMap<u8> = HitMap::new();
    hits.push(1, Rect::new(0, 0, 10, 10));
    let hit = hits.hit_test(Position::new(1, 1));
    let click = Gesture::Click {
        button: MouseButton::Left,
        position: Position::new(1, 1),
        clicks: 1,
    };

    // The painter's-algorithm rule, shared with the HitMap: the binding
    // registered later is on top, whatever its scope.
    let mut map: MouseMap<u8, Cmd> = MouseMap::new();
    map.bind(MousePattern::anywhere(MouseClass::AnyClick), Cmd::Dismiss);
    map.bind(MousePattern::on(1, MouseClass::AnyClick), Cmd::Focus);
    assert_eq!(map.resolve(hit, &click), Some(Cmd::Focus));
    assert_eq!(map.resolve(None, &click), Some(Cmd::Dismiss));

    let mut map: MouseMap<u8, Cmd> = MouseMap::new();
    map.bind(MousePattern::on(1, MouseClass::AnyClick), Cmd::Focus);
    map.bind(MousePattern::anywhere(MouseClass::AnyClick), Cmd::Dismiss);
    assert_eq!(map.resolve(hit, &click), Some(Cmd::Dismiss));
}

#[test]
fn button_patterns_are_exact_or_any() {
    let mut map: MouseMap<u8, Cmd> = MouseMap::new();
    map.bind(
        MousePattern::on(1, MouseClass::AnyClick).button(MouseButton::Right),
        Cmd::Open,
    );
    map.bind(
        MousePattern::on(1, MouseClass::AnyClick).any_button(),
        Cmd::Focus,
    );
    // Later binding wins, so any button resolves as Focus; with only the
    // exact pattern bound, the left click would miss entirely.
    let left = Gesture::Click {
        button: MouseButton::Left,
        position: Position::ZERO,
        clicks: 1,
    };
    assert_eq!(map.resolve(Some(hit_at_one()), &left), Some(Cmd::Focus));

    let mut exact: MouseMap<u8, Cmd> = MouseMap::new();
    exact.bind(
        MousePattern::on(1, MouseClass::AnyClick).button(MouseButton::Right),
        Cmd::Open,
    );
    assert_eq!(exact.resolve(Some(hit_at_one()), &left), None);
    let right = Gesture::Click {
        button: MouseButton::Right,
        position: Position::ZERO,
        clicks: 1,
    };
    assert_eq!(exact.resolve(Some(hit_at_one()), &right), Some(Cmd::Open));
}

fn hit_at_one() -> tuvix_core::hit::Hit<u8> {
    tuvix_core::hit::Hit {
        id: 1,
        area: Rect::new(0, 0, 10, 10),
        local: Position::new(1, 1),
    }
}

#[test]
fn the_class_matrix_is_pinned() {
    let mut map: MouseMap<u8, Cmd> = MouseMap::new();
    // The catch-all binds first: later (more specific) bindings shadow it.
    map.bind(MousePattern::on(1, MouseClass::AnyClick), Cmd::Dismiss);
    map.bind(MousePattern::on(1, MouseClass::Click), Cmd::Focus);
    map.bind(MousePattern::on(1, MouseClass::DoubleClick), Cmd::Open);

    let click = |clicks: u16| Gesture::Click {
        button: MouseButton::Left,
        position: Position::ZERO,
        clicks,
    };
    // The specific bindings shadow the catch-all by recency: the single
    // click hits `Click`, the double `DoubleClick`, and everything —
    // including a triple — falls through to `AnyClick`.
    assert_eq!(map.resolve(Some(hit_at_one()), &click(1)), Some(Cmd::Focus));
    assert_eq!(map.resolve(Some(hit_at_one()), &click(2)), Some(Cmd::Open));
    assert_eq!(
        map.resolve(Some(hit_at_one()), &click(3)),
        Some(Cmd::Dismiss)
    );

    // Without the catch-all, a triple click matches nothing: there is no
    // implicit `TripleClick` class — apps bound on `AnyClick` get it.
    let mut strict: MouseMap<u8, Cmd> = MouseMap::new();
    strict.bind(MousePattern::on(1, MouseClass::Click), Cmd::Focus);
    assert_eq!(strict.resolve(Some(hit_at_one()), &click(3)), None);

    // Press never matches a Click binding, and vice versa.
    let mut press_map: MouseMap<u8, Cmd> = MouseMap::new();
    press_map.bind(MousePattern::on(1, MouseClass::Press), Cmd::Focus);
    assert_eq!(press_map.resolve(Some(hit_at_one()), &click(1)), None);
    let press = Gesture::Press {
        button: MouseButton::Left,
        position: Position::ZERO,
    };
    assert_eq!(
        press_map.resolve(Some(hit_at_one()), &press),
        Some(Cmd::Focus)
    );
}

#[test]
fn drag_bindings_resolve_against_the_drag_origin() {
    let mut map: MouseMap<&'static str, Cmd> = MouseMap::new();
    map.bind(MousePattern::on("panel", MouseClass::DragStart), Cmd::Move);
    map.bind(MousePattern::on("panel", MouseClass::Drag), Cmd::Move);
    map.bind(MousePattern::on("panel", MouseClass::DragEnd), Cmd::Drop);

    let mut hits: HitMap<&str> = HitMap::new();
    hits.push("panel", Rect::new(5, 5, 10, 6));

    // A drag that started on the panel belongs to the panel even when the
    // pointer has left it; the drop target resolves the `to` cell itself.
    let origin = Position::new(6, 6);
    let outside = Position::new(30, 30);
    let hit = hits.hit_test(origin);
    assert_eq!(
        map.resolve(
            hit,
            &Gesture::DragStart {
                button: MouseButton::Left,
                origin
            }
        ),
        Some(Cmd::Move)
    );
    assert_eq!(
        map.resolve(
            hit,
            &Gesture::Drag {
                button: MouseButton::Left,
                origin,
                position: outside
            }
        ),
        Some(Cmd::Move)
    );
    assert_eq!(
        map.resolve(
            hit,
            &Gesture::DragEnd {
                button: MouseButton::Left,
                origin,
                position: outside
            }
        ),
        Some(Cmd::Drop)
    );
}

#[test]
fn resolution_is_stateless_and_repeatable() {
    let mut map: MouseMap<u8, Cmd> = MouseMap::new();
    map.bind(MousePattern::on(1, MouseClass::AnyClick), Cmd::Focus);
    let click = Gesture::Click {
        button: MouseButton::Left,
        position: Position::ZERO,
        clicks: 1,
    };
    let hit = hit_at_one();
    let first = map.resolve(Some(hit), &click);
    let second = map.resolve(Some(hit), &click);
    assert_eq!(first, Some(Cmd::Focus));
    assert_eq!(second, first, "the same call answers the same command");
    // Empty space with no `Any` binding covering the class misses, and
    // a `Moved` gesture matches nothing in this map.
    let moved = Gesture::Moved {
        position: Position::ZERO,
    };
    assert_eq!(map.resolve(None, &moved), None);
    assert_eq!(map.len(), 1);
}

// ---------------------------------------------------------------------------
// B17 — the mouse-decision gate: a click resolves through a 1,000-region
// hit map plus a 32-binding mouse map inside one frame budget.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b17_mouse_decision_within_frame_budget() {
    // Build a 1,000-region map (a dense dashboard: 40x25 grid of tiles).
    let mut hits: HitMap<u32> = HitMap::new();
    for id in 0..1_000u32 {
        let x = u16::try_from((id % 40) * 5).unwrap_or(u16::MAX);
        let y = u16::try_from((id / 40) * 2).unwrap_or(u16::MAX);
        hits.push(id, Rect::new(x, y, 5, 2));
    }
    let mut map: MouseMap<u32, u32> = MouseMap::new();
    for id in 0..32u32 {
        map.bind(MousePattern::on(id, MouseClass::AnyClick), id);
    }
    let click = Gesture::Click {
        button: MouseButton::Left,
        position: Position::new(103, 51),
        clicks: 1,
    };

    // Warm-up.
    for _ in 0..2_000 {
        let hit = hits.hit_test(Position::new(103, 51));
        let _ = map.resolve(hit, &click);
    }

    let iterations = 20_000;
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        let hit = hits.hit_test(Position::new(103, 51));
        let resolved = map.resolve(hit, &click);
        std::hint::black_box(resolved);
    }
    let per_decision = start.elapsed() / iterations;
    assert!(
        per_decision <= Duration::from_micros(20),
        "B17 gate failed: {per_decision:?} per hit-test + resolve (budget 20 us, hard ceiling on shared runners)"
    );
    println!(
        "B17 gate: {per_decision:?} per mouse decision (1,000-region hit map + 32-binding mouse map)"
    );

    // The gesture half: classifying a click stream stays O(1) per event.
    let mut gestures = GestureRecognizer::new(GestureConfig::default());
    let stream: Vec<MouseEvent> = (0..1_000)
        .flat_map(|i| {
            let x = u16::try_from(i % 80).unwrap_or(u16::MAX);
            [
                mouse(MouseEventKind::Down(MouseButton::Left), x, 1),
                mouse(MouseEventKind::Up(MouseButton::Left), x, 1),
            ]
        })
        .collect();
    let start = std::time::Instant::now();
    let mut elapsed = 0u64;
    for event in &stream {
        elapsed += 700; // outside the double-click window: every release clicks
        let _ = std::hint::black_box(gestures.feed(*event, ms(elapsed)));
    }
    let per_event = start.elapsed() / u32::try_from(stream.len()).unwrap_or(u32::MAX);
    assert!(
        per_event <= Duration::from_micros(1),
        "B17 gate failed: {per_event:?} per classified mouse event (budget 1 us)"
    );
    println!("B17 gate: {per_event:?} per classified mouse event (1,000-event click stream)");
}
