//! M16 integration gates: the floating-surface placement module.
//!
//! `place` is the geometry the popup pattern needs: a dropdown opens
//! below its trigger and flips above when the edge is near, a menu
//! opens above and flips below, a modal centers, a tooltip pins at a
//! position — and every result stays inside the host when the size
//! allows it, pins to the nearest honest spot when it does not, and
//! never panics at any `u16` corner.
//!
//! The rules the tests pin (docs/ARCHITECTURE.md, the M16 decisions):
//!
//! - **flip, then clamp** — the preferred side first, the mirror side
//!   when the edge is near, the nearest host edge when neither fits;
//! - **size never shrinks** (except `Centered`, which keeps the M6
//!   `Rect::centered` rule of becoming the host) — the buffer's clip
//!   discipline owns truncation, the caller sees honest geometry;
//! - **the rect never starts outside the host** — position clamps to
//!   the host origin no matter where the anchor hangs;
//! - **saturating corners are total** — `u16::MAX` anchors, sizes and
//!   hosts produce a deterministic rect, not a panic.

use tuvix_core::geometry::{Position, Rect, Size};
use tuvix_core::layer::{Placement, place};

fn place_below(host: Rect, anchor: Rect, width: u16, height: u16) -> Rect {
    place(host, anchor, Size::new(width, height), Placement::Below)
}

fn place_above(host: Rect, anchor: Rect, width: u16, height: u16) -> Rect {
    place(host, anchor, Size::new(width, height), Placement::Above)
}

fn place_at(host: Rect, x: u16, y: u16, width: u16, height: u16) -> Rect {
    place(
        host,
        anchor_anywhere(),
        Size::new(width, height),
        Placement::At(Position::new(x, y)),
    )
}

fn anchor_anywhere() -> Rect {
    Rect::new(0, 0, 1, 1)
}

// ---------------------------------------------------------------------
// Below: the dropdown case.
// ---------------------------------------------------------------------

#[test]
fn below_opens_directly_under_the_trigger_left_aligned() {
    let host = Rect::new(0, 0, 40, 20);
    let trigger = Rect::new(5, 5, 10, 1);
    assert_eq!(place_below(host, trigger, 8, 3), Rect::new(5, 6, 8, 3));
}

#[test]
fn below_flips_above_when_the_edge_is_near() {
    let host = Rect::new(0, 0, 40, 10);
    let trigger = Rect::new(5, 7, 10, 1);
    // Two rows free below, three needed: the dropdown opens above.
    assert_eq!(place_below(host, trigger, 8, 3), Rect::new(5, 4, 8, 3));
}

#[test]
fn below_clamps_bottom_flush_when_neither_side_fits() {
    let host = Rect::new(0, 0, 40, 10);
    let trigger = Rect::new(5, 4, 10, 2);
    // Four rows on either side, eight needed: bottom-flush inside the
    // host, covering the trigger — the least-surprise fallback.
    assert_eq!(place_below(host, trigger, 8, 8), Rect::new(5, 2, 8, 8));
}

#[test]
fn below_right_aligns_to_the_trigger_when_left_does_not_fit() {
    let host = Rect::new(0, 0, 20, 10);
    let trigger = Rect::new(14, 0, 5, 1);
    // Left edge needs 22 columns, the host has 20: right-align to the
    // trigger's right edge.
    assert_eq!(place_below(host, trigger, 8, 3), Rect::new(11, 1, 8, 3));
}

#[test]
fn below_pins_to_the_host_top_when_taller_than_the_host() {
    let host = Rect::new(0, 0, 10, 5);
    let trigger = Rect::new(2, 2, 4, 1);
    let placed = place_below(host, trigger, 3, 9);
    assert_eq!(placed, Rect::new(2, 0, 3, 9));
    // The size never shrinks: the caller sees honest geometry and the
    // buffer clips the rows past the host.
    assert_eq!(placed.height, 9);
}

#[test]
fn below_pins_to_the_host_left_when_wider_than_the_host() {
    let host = Rect::new(0, 0, 10, 5);
    let trigger = Rect::new(6, 1, 3, 1);
    assert_eq!(place_below(host, trigger, 20, 3), Rect::new(0, 2, 20, 3));
}

// ---------------------------------------------------------------------
// Above: the upward menu case — the exact mirror of Below.
// ---------------------------------------------------------------------

#[test]
fn above_opens_directly_over_the_trigger() {
    let host = Rect::new(0, 0, 40, 20);
    let trigger = Rect::new(5, 10, 10, 1);
    assert_eq!(place_above(host, trigger, 8, 3), Rect::new(5, 7, 8, 3));
}

#[test]
fn above_flips_below_when_the_edge_is_near() {
    let host = Rect::new(0, 0, 40, 10);
    let trigger = Rect::new(5, 1, 10, 1);
    assert_eq!(place_above(host, trigger, 8, 3), Rect::new(5, 2, 8, 3));
}

#[test]
fn above_clamps_top_flush_when_neither_side_fits() {
    let host = Rect::new(0, 0, 40, 10);
    let trigger = Rect::new(5, 4, 10, 2);
    assert_eq!(place_above(host, trigger, 8, 8), Rect::new(5, 0, 8, 8));
}

// ---------------------------------------------------------------------
// Centered: exact parity with the M6 rule.
// ---------------------------------------------------------------------

#[test]
fn centered_matches_rect_centered_exactly() {
    let host = Rect::new(0, 0, 80, 24);
    let trigger = Rect::new(3, 3, 9, 1);
    assert_eq!(
        place(host, trigger, Size::new(30, 10), Placement::Centered),
        host.centered(30, 10),
    );
    assert_eq!(
        place(host, trigger, Size::new(30, 10), Placement::Centered),
        Rect::new(25, 7, 30, 10),
    );
}

#[test]
fn centered_oversized_becomes_the_host_like_the_m6_rule() {
    let host = Rect::new(4, 2, 80, 24);
    let trigger = anchor_anywhere();
    assert_eq!(
        place(host, trigger, Size::new(100, 30), Placement::Centered),
        host,
        "a modal bigger than the host is the host (Rect::centered parity)"
    );
}

// ---------------------------------------------------------------------
// At: the tooltip case — clamp, never flip.
// ---------------------------------------------------------------------

#[test]
fn at_places_the_corner_exactly_when_inside() {
    let host = Rect::new(0, 0, 40, 10);
    assert_eq!(place_at(host, 3, 4, 5, 3), Rect::new(3, 4, 5, 3));
}

#[test]
fn at_clamps_at_the_host_edges() {
    let host = Rect::new(0, 0, 40, 10);
    // Three columns past the right edge: pulled back to right-flush.
    assert_eq!(place_at(host, 38, 0, 5, 3), Rect::new(35, 0, 5, 3));
    // Way past both edges: pinned to the nearest honest corner.
    assert_eq!(place_at(host, 100, 100, 5, 3), Rect::new(35, 7, 5, 3));
}

#[test]
fn at_never_starts_before_the_host_origin() {
    let host = Rect::new(10, 10, 40, 10);
    assert_eq!(place_at(host, 0, 0, 5, 3), Rect::new(10, 10, 5, 3));
}

// ---------------------------------------------------------------------
// Host honesty and saturating corners.
// ---------------------------------------------------------------------

#[test]
fn an_anchor_outside_the_host_never_pulls_the_rect_out() {
    let host = Rect::new(10, 10, 20, 10);
    let trigger = Rect::new(0, 0, 5, 1);
    // The trigger hangs off the host's top-left; "below" it is still
    // outside. The rect starts at the host origin instead.
    assert_eq!(place_below(host, trigger, 4, 2), Rect::new(10, 10, 4, 2));
}

#[test]
fn u16_max_corners_are_total_and_deterministic() {
    let host = Rect::new(u16::MAX - 10, u16::MAX - 10, 10, 10);
    let trigger = Rect::new(u16::MAX - 5, u16::MAX - 5, 5, 5);

    // A size that fits resolves exactly.
    assert_eq!(
        place_below(host, trigger, 3, 3),
        Rect::new(u16::MAX - 5, u16::MAX - 8, 3, 3),
    );

    // A `u16::MAX` surface pins to the host origin, size unchanged.
    let huge = Size::new(u16::MAX, u16::MAX);
    let placed = place(host, trigger, huge, Placement::Below);
    assert_eq!(placed.x, host.x);
    assert_eq!(placed.y, host.y);
    assert_eq!(placed.width, u16::MAX);
    assert_eq!(placed.height, u16::MAX);
}

#[test]
fn an_empty_surface_resolves_to_a_zero_rect_inside_the_host() {
    let host = Rect::new(2, 3, 20, 10);
    let trigger = Rect::new(5, 5, 4, 1);
    assert_eq!(place_below(host, trigger, 0, 0), Rect::new(5, 6, 0, 0));
}

#[test]
fn an_empty_host_still_resolves_without_panicking() {
    let host = Rect::new(0, 0, 0, 0);
    let trigger = Rect::new(0, 0, 1, 1);
    assert_eq!(place_below(host, trigger, 3, 2), Rect::new(0, 0, 3, 2));
    assert_eq!(
        place(host, trigger, Size::new(3, 2), Placement::Centered),
        host
    );
}
