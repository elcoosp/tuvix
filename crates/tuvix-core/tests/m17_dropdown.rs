//! M17 integration gates: the floating-dropdown sizing decision.
//!
//! `place` answers *where* a floating surface goes (the M16
//! flip-then-clamp ladder); `dropdown_rows` answers *how many rows that
//! surface should request* — the one input placement cannot derive from
//! geometry alone. Together they close the M16 seam: an app no longer
//! re-rolls "how tall is my dropdown?" by hand.
//!
//! The rules the tests pin (docs/ARCHITECTURE.md, the M17 decisions):
//!
//! - **the option count, capped by the roomiest side** — a dropdown
//!   never claims more rows than its content needs, nor more than the
//!   larger of the two gaps (below the anchor, above it), so `place`'s
//!   ladder always receives a size one of its paths can open;
//! - **zero content, zero rows** — a dropdown with nothing to show
//!   requests no surface at all;
//! - **one row is the floor** — when both gaps are closed the answer is
//!   one row, never zero for non-empty content: the caller decides not
//!   to open, the sizing never lies about wanting space;
//! - **total at every corner** — anchors outside the host, `u16::MAX`
//!   hosts and `usize::MAX` contents produce a deterministic count, not
//!   a panic.

use tuvix_core::geometry::{Rect, Size};
use tuvix_core::layer::{Placement, dropdown_rows, place};

// ---------------------------------------------------------------------
// The sizing rules.
// ---------------------------------------------------------------------

#[test]
fn content_that_fits_below_requests_its_rows() {
    // Host 40x20, trigger row at y=5: fourteen rows free below, five
    // above. Six options fit below untouched.
    let host = Rect::new(0, 0, 40, 20);
    let trigger = Rect::new(5, 5, 10, 1);
    assert_eq!(dropdown_rows(host, trigger, 6), 6);
}

#[test]
fn content_beyond_below_borrows_the_roomier_side() {
    // Trigger at y=10: nine rows below, ten above. Eighteen options
    // request the ten above — the side `place` will flip to.
    let host = Rect::new(0, 0, 40, 20);
    let trigger = Rect::new(5, 10, 10, 1);
    assert_eq!(dropdown_rows(host, trigger, 18), 10);
}

#[test]
fn content_beyond_both_sides_caps_at_the_roomiest() {
    let host = Rect::new(0, 0, 40, 20);
    let trigger = Rect::new(5, 10, 10, 1);
    assert_eq!(dropdown_rows(host, trigger, 30), 10);
}

#[test]
fn a_bottom_row_trigger_opens_with_the_room_above() {
    let host = Rect::new(0, 0, 40, 20);
    let trigger = Rect::new(5, 19, 10, 1);
    assert_eq!(dropdown_rows(host, trigger, 8), 8);
}

#[test]
fn no_room_on_either_side_floors_at_one_row() {
    // A trigger filling a one-row host: no gap anywhere, but non-empty
    // content still asks for its one row and lets the buffer clip it.
    let host = Rect::new(0, 0, 40, 1);
    let trigger = Rect::new(0, 0, 40, 1);
    assert_eq!(dropdown_rows(host, trigger, 9), 1);
}

#[test]
fn zero_content_requests_zero_rows() {
    let host = Rect::new(0, 0, 40, 20);
    let trigger = Rect::new(5, 5, 10, 1);
    assert_eq!(dropdown_rows(host, trigger, 0), 0);
}

#[test]
fn an_anchor_outside_the_host_stays_total() {
    // The trigger hangs 90 rows past the host's bottom: no room below
    // (saturating), one hundred above.
    let host = Rect::new(0, 0, 10, 10);
    let anchor = Rect::new(0, 100, 5, 1);
    assert_eq!(dropdown_rows(host, anchor, 200), 100);
}

#[test]
fn the_maximum_corners_are_total() {
    let host = Rect::new(0, 0, u16::MAX, u16::MAX);
    let anchor = Rect::new(0, 0, 1, 1);
    assert_eq!(dropdown_rows(host, anchor, usize::MAX), 65_534);
}

#[test]
fn the_answer_never_exceeds_the_content_or_the_host() {
    // A deterministic sweep across host heights, option counts and
    // trigger rows: the count stays within both bounds and never dips
    // below one for non-empty content.
    let options = [0_usize, 1, 5, 17, 100];
    for host_height in 1..=24_u16 {
        let host = Rect::new(0, 0, 80, host_height);
        for count in options {
            for trigger_y in 0..host_height {
                let trigger = Rect::new(0, trigger_y, 8, 1);
                let rows = dropdown_rows(host, trigger, count);
                assert!(
                    rows <= host_height,
                    "rows {rows} exceed the {host_height}-row host"
                );
                if count == 0 {
                    assert_eq!(rows, 0, "no content, no surface");
                } else {
                    assert!(rows >= 1, "non-empty content floors at one row");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// The composition: size, then place.
// ---------------------------------------------------------------------

#[test]
fn the_sized_surface_places_below_when_it_fits() {
    let host = Rect::new(0, 0, 40, 20);
    let trigger = Rect::new(5, 5, 10, 1);
    let rows = dropdown_rows(host, trigger, 6);
    let surface = place(host, trigger, Size::new(12, rows), Placement::Below);
    assert_eq!(surface, Rect::new(5, 6, 12, 6), "opens under the trigger");
}

#[test]
fn the_sized_surface_flips_above_when_below_is_short() {
    // Nine rows below the trigger, ten above, eighteen options: the
    // sized surface (ten rows) does not fit below, so `place` flips it
    // above — flush against the host's top edge, ending at the trigger.
    let host = Rect::new(0, 0, 40, 20);
    let trigger = Rect::new(5, 10, 10, 1);
    let rows = dropdown_rows(host, trigger, 18);
    let surface = place(host, trigger, Size::new(12, rows), Placement::Below);
    assert_eq!(rows, 10);
    assert_eq!(surface, Rect::new(5, 0, 12, 10));
    assert_eq!(
        surface.bottom(),
        trigger.y,
        "the surface ends at the trigger"
    );
}
