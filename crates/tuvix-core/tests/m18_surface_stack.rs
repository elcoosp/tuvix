//! M18 contract tests: the floating-surface stack.
//!
//! M16 shipped the geometry (`place`) and the paint (`Overlay`); M17
//! closed the single-dropdown seam. What an app with more than one
//! floating surface still hand-rolls is the *routing*: open/close flags
//! per surface, z-order by manual paint order, dismissal precedence and
//! hit-map registration order. [`SurfaceStack`] is that routing as a
//! type — the `FocusRing`/`Keymap` treatment for floating surfaces.
//!
//! The contract pinned here:
//!
//! - **push opens or raises** — a new key opens on top (`true`), an
//!   open key is raised to the top without duplicating (`false`);
//! - **`order()` is the painter's order** — bottom-to-top, the exact
//!   sequence both the paint and the `HitMap` registration consume;
//! - **`close` is stable** — removing a middle surface keeps the
//!   relative order of the rest;
//! - **`close_top` is the Esc precedence** — the top surface dismisses
//!   first, exactly the seam the hand-rolled example gets wrong (Esc
//!   closing the dropdown underneath an open modal).

use tuvix_core::geometry::{Position, Rect};
use tuvix_core::hit::HitMap;
use tuvix_core::layer::SurfaceStack;

/// The app's own pane enum — the generic `K` in practice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Dropdown,
    Tooltip,
    Modal,
    Toast,
}

use Panel::{Dropdown, Modal, Toast, Tooltip};

#[test]
fn new_surfaces_stack_on_top() {
    let mut stack = SurfaceStack::new();
    assert!(stack.is_empty());

    assert!(stack.push(Dropdown), "a new surface opens");
    assert!(stack.push(Tooltip), "a second surface opens");
    assert!(stack.push(Modal), "a third surface opens");

    assert_eq!(stack.order(), &[Dropdown, Tooltip, Modal]);
    assert_eq!(stack.top(), Some(&Modal), "the last push is on top");
    assert_eq!(stack.len(), 3);
    assert!(!stack.is_empty());
}

#[test]
fn pushing_an_open_surface_raises_it_without_duplicating() {
    let mut stack = SurfaceStack::new();
    stack.push(Dropdown);
    stack.push(Tooltip);
    stack.push(Modal);

    assert!(
        !stack.push(Dropdown),
        "an open surface is raised, not opened"
    );
    assert_eq!(stack.order(), &[Tooltip, Modal, Dropdown]);
    assert_eq!(stack.top(), Some(&Dropdown));
    assert_eq!(stack.len(), 3, "no duplicate key entered the stack");
}

#[test]
fn closing_removes_a_surface_stably() {
    let mut stack = SurfaceStack::new();
    stack.push(Dropdown);
    stack.push(Tooltip);
    stack.push(Modal);

    assert!(stack.close(&Tooltip));
    assert_eq!(
        stack.order(),
        &[Dropdown, Modal],
        "the remaining surfaces keep their relative order"
    );

    assert!(
        !stack.close(&Tooltip),
        "closing a closed surface is a no-op"
    );
    assert_eq!(stack.order(), &[Dropdown, Modal]);
    assert!(!stack.is_open(&Tooltip));
    assert!(stack.is_open(&Dropdown));
}

#[test]
fn close_top_dismisses_the_top_surface_first() {
    let mut stack = SurfaceStack::new();
    stack.push(Dropdown);
    stack.push(Modal);

    assert_eq!(stack.close_top(), Some(Modal), "Esc dismisses the modal");
    assert_eq!(stack.order(), &[Dropdown], "the dropdown stays open");
    assert_eq!(stack.top(), Some(&Dropdown));

    assert_eq!(stack.close_top(), Some(Dropdown));
    assert!(stack.is_empty());

    assert_eq!(
        stack.close_top(),
        None,
        "an empty stack has nothing to dismiss"
    );
}

#[test]
fn an_empty_stack_is_total() {
    let mut stack: SurfaceStack<Panel> = SurfaceStack::new();
    assert!(stack.is_empty());
    assert_eq!(stack.len(), 0);
    assert_eq!(stack.top(), None);
    assert_eq!(stack.close_top(), None);
    assert!(!stack.close(&Dropdown));
    assert!(!stack.is_open(&Dropdown));
    assert_eq!(stack.order(), [], "the order of an empty stack is empty");
}

#[test]
fn order_is_the_paint_and_hit_map_registration_order() {
    let mut stack = SurfaceStack::new();
    stack.push(Dropdown);
    stack.push(Modal);

    // The base, then the surfaces bottom-to-top — the painter's
    // algorithm on both sides of the frame.
    let base = Rect::new(0, 0, 40, 10);
    let dropdown_rect = Rect::new(2, 1, 10, 3);
    let modal_rect = Rect::new(4, 2, 12, 5);

    let mut hits: HitMap<&'static str> = HitMap::new();
    hits.push("base", base);
    for (key, rect) in [(Dropdown, dropdown_rect), (Modal, modal_rect)] {
        assert!(
            stack.is_open(&key),
            "order() drove the registration, so the surface is open"
        );
        hits.push(key_name(key), rect);
    }

    // Inside both surfaces the topmost registration wins...
    let hit = hits.hit_test(Position::new(6, 3)).unwrap();
    assert_eq!(hit.id, "modal");

    // ...inside the dropdown only, the dropdown wins...
    let hit = hits.hit_test(Position::new(2, 1)).unwrap();
    assert_eq!(hit.id, "dropdown");

    // ...and the uncovered base stays reachable.
    let hit = hits.hit_test(Position::new(0, 9)).unwrap();
    assert_eq!(hit.id, "base");
}

/// Maps the test enum onto the `HitMap` id type it feeds.
fn key_name(key: Panel) -> &'static str {
    match key {
        Dropdown => "dropdown",
        Tooltip => "tooltip",
        Modal => "modal",
        Toast => "toast",
    }
}

#[test]
fn clear_dismisses_every_surface() {
    let mut stack = SurfaceStack::new();
    stack.push(Dropdown);
    stack.push(Modal);

    stack.clear();
    assert!(stack.is_empty());
    assert_eq!(stack.top(), None);
    assert_eq!(stack.close_top(), None);
}

#[test]
fn toasts_stack_above_everything_until_closed() {
    // A toast pushed over a busy stack sits on top and closes by value
    // without disturbing the routing of the surfaces under it.
    let mut stack = SurfaceStack::new();
    stack.push(Dropdown);
    stack.push(Modal);

    assert!(stack.push(Toast));
    assert_eq!(stack.top(), Some(&Toast));

    assert!(stack.close(&Toast));
    assert_eq!(
        stack.top(),
        Some(&Modal),
        "the stack resumes below the toast"
    );
    assert_eq!(stack.order(), &[Dropdown, Modal]);
}
