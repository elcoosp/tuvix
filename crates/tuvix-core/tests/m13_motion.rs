//! M13 motion tests: the animation layer's public contract.
//!
//! The easing golden values were computed independently (Python, the
//! easings.net formulas) before the Rust implementation existed; they pin
//! the catalog at p = 0.25 / 0.5 / 0.75. Everything else pins semantics:
//! clamped progress, delayed/repeated/alternate tweens, frame-rate
//! independence, the animator's sorted finish reports and its O(1) idle
//! decision (the static-CPU principle as a type).
//!
//! The B16 timing gate at the bottom is `#[ignore]`d: run it with
//! `cargo test -p tuvix-core --release --ignored` and record the numbers
//! in docs/BENCHMARKS.md.

#![expect(
    clippy::unreadable_literal,
    reason = "machine-generated golden easing samples (M13) pinned at full precision; digit grouping would break byte-identical regeneration from the generator output"
)]
use std::time::Duration;

use tuvix_core::anim::{Animator, Easing, Interpolate, Progress, Repeat, Tween};
use tuvix_core::style::Color;

const MS: u64 = 1_000_000;
fn ms(n: u64) -> Duration {
    Duration::from_nanos(n * MS)
}

// ---------------------------------------------------------------------------
// Progress: the clamped, Duration-driven [0, 1] domain every curve works in.
// ---------------------------------------------------------------------------

#[test]
fn progress_clamps_out_of_range_and_nan() {
    assert_eq!(Progress::new(-1.0), Progress::ZERO);
    assert_eq!(Progress::new(0.5).value(), 0.5);
    assert_eq!(Progress::new(2.0), Progress::ONE);
    // NaN must never smuggle itself into the domain: it clamps to zero.
    assert_eq!(Progress::new(f64::NAN), Progress::ZERO);
}

#[test]
fn progress_of_elapsed_is_time_over_total() {
    assert_eq!(Progress::of_elapsed(ms(0), ms(1_000)).value(), 0.0);
    assert_eq!(Progress::of_elapsed(ms(400), ms(1_000)).value(), 0.4);
    assert_eq!(Progress::of_elapsed(ms(1_000), ms(1_000)).value(), 1.0);
    // Overshoot pins at the end; Duration is unsigned so underflow is
    // unrepresentable.
    assert_eq!(Progress::of_elapsed(ms(5_000), ms(1_000)), Progress::ONE);
}

#[test]
fn progress_of_zero_duration_is_complete() {
    assert_eq!(Progress::of_elapsed(ms(0), Duration::ZERO), Progress::ONE);
}

#[test]
fn progress_complement_flips_the_domain() {
    assert_eq!(Progress::new(0.3).complement().value(), 0.7);
    assert_eq!(Progress::ZERO.complement(), Progress::ONE);
}

// ---------------------------------------------------------------------------
// Easing: the 16-curve catalog, pinned against the independent reference.
// ---------------------------------------------------------------------------

/// `(curve, [ease(0.25), ease(0.5), ease(0.75)])` — computed with the
/// easings.net formulas in Python before the implementation was written.
/// The literals are machine-generated reference data in shortest
/// round-trip form: every digit shown is a digit the value needs.
const GOLDEN: &[(Easing, [f64; 3])] = &[
    (Easing::Linear, [0.25, 0.5, 0.75]),
    (Easing::InQuad, [0.0625, 0.25, 0.5625]),
    (Easing::OutQuad, [0.4375, 0.75, 0.9375]),
    (Easing::InOutQuad, [0.125, 0.5, 0.875]),
    (Easing::InCubic, [0.015625, 0.125, 0.421875]),
    (Easing::OutCubic, [0.578125, 0.875, 0.984375]),
    (Easing::InOutCubic, [0.0625, 0.5, 0.9375]),
    (Easing::OutQuart, [0.68359375, 0.9375, 0.99609375]),
    (Easing::InOutQuart, [0.03125, 0.5, 0.96875]),
    (
        Easing::OutSine,
        [0.3826834323650898, 0.7071067811865475, 0.9238795325112867],
    ),
    (
        Easing::InOutSine,
        [0.1464466094067262, 0.49999999999999994, 0.8535533905932737],
    ),
    (
        Easing::OutExpo,
        [0.8232233047033631, 0.96875, 0.99447572827198],
    ),
    (Easing::InOutExpo, [0.015625, 0.5, 0.984375]),
    (
        Easing::OutBack,
        [0.8174096875000002, 1.0876975, 1.0641365625],
    ),
    (
        Easing::OutElastic,
        [0.9116116523516816, 1.015625, 1.00552427172802],
    ),
    (Easing::OutBounce, [0.47265625, 0.765625, 0.97265625]),
];

#[test]
fn easing_catalog_matches_the_independent_reference() {
    for (curve, expected) in GOLDEN {
        for (index, raw) in [0.25, 0.5, 0.75].iter().enumerate() {
            let got = curve.ease(Progress::new(*raw));
            assert!(
                (got - expected[index]).abs() < 1e-12,
                "{curve:?} at p={raw}: got {got}, want {}",
                expected[index]
            );
        }
    }
}

#[test]
fn every_curve_fixes_both_endpoints() {
    for (curve, _) in GOLDEN {
        assert_eq!(curve.ease(Progress::ZERO), 0.0, "{curve:?} at 0");
        assert_eq!(curve.ease(Progress::ONE), 1.0, "{curve:?} at 1");
    }
}

#[test]
fn the_non_overshooting_family_is_monotone() {
    // Bounce dips by design; back/elastic overshoot by design; the other
    // thirteen curves never move backwards.
    let overshooting = [Easing::OutBack, Easing::OutElastic, Easing::OutBounce];
    for (curve, _) in GOLDEN {
        if overshooting.contains(curve) {
            continue;
        }
        let mut previous = -f64::EPSILON;
        for step in 0..=1_000 {
            let value = curve.ease(Progress::new(f64::from(step) / 1_000.0));
            assert!(
                value >= previous,
                "{curve:?} decreased at step {step}: {value} < {previous}"
            );
            previous = value;
        }
    }
}

#[test]
fn out_back_overshoots_by_about_ten_percent() {
    let mut max: f64 = 0.0;
    for step in 0..=10_000 {
        let value = Easing::OutBack.ease(Progress::new(f64::from(step) / 10_000.0));
        max = max.max(value);
    }
    assert!(
        (max - 1.1).abs() < 1e-3,
        "the classic 1.70158 constant peaks at 1.1, got {max}"
    );
}

#[test]
fn out_elastic_overshoots_then_settles() {
    let mut max: f64 = 0.0;
    for step in 0..=10_000 {
        let value = Easing::OutElastic.ease(Progress::new(f64::from(step) / 10_000.0));
        max = max.max(value);
    }
    assert!(
        max > 1.3 && max < 1.4,
        "elastic peak outside 1.3..1.4: {max}"
    );
}

#[test]
fn out_bounce_stays_within_the_unit_interval() {
    for step in 0..=1_000 {
        let value = Easing::OutBounce.ease(Progress::new(f64::from(step) / 1_000.0));
        assert!(
            (0.0..=1.0).contains(&value),
            "bounce left [0,1] at {step}: {value}"
        );
    }
}

#[test]
fn easing_default_is_linear() {
    assert_eq!(Easing::default(), Easing::Linear);
}

// ---------------------------------------------------------------------------
// Interpolate: values that can travel between endpoints.
// ---------------------------------------------------------------------------

#[test]
fn float_lerp_walks_the_segment() {
    assert_eq!(f64::lerp(0.0, 100.0, 0.4), 40.0);
    assert_eq!(f64::lerp(10.0, 20.0, 0.0), 10.0);
    assert_eq!(f64::lerp(10.0, 20.0, 1.0), 20.0);
    assert!((f32::lerp(0.0, 1.0, 0.25) - 0.25).abs() < 1e-6);
}

#[test]
fn float_lerp_descends_when_the_target_is_lower() {
    assert_eq!(f64::lerp(100.0, 0.0, 0.3), 70.0);
}

#[test]
fn integer_lerp_rounds_half_away_from_zero() {
    assert_eq!(i32::lerp(0, 100, 0.5), 50);
    assert_eq!(i32::lerp(10, 11, 0.5), 11);
    // Half-away-from-zero is direction-agnostic: both directions of the
    // midpoint land on the same integer.
    assert_eq!(i32::lerp(11, 10, 0.5), 11);
    assert_eq!(u8::lerp(10, 30, 0.5), 20);
    assert_eq!(u16::lerp(0, 255, 0.5), 128);
    assert_eq!(i64::lerp(-3, 3, 0.5), 0);
}

#[test]
fn integer_lerp_respects_the_type_range() {
    assert_eq!(u8::lerp(0, 255, 1.0), 255);
    assert_eq!(i8::lerp(-128, 127, 1.0), 127);
    assert_eq!(u32::lerp(0, 4_000_000_000, 0.5), 2_000_000_000);
}

#[test]
fn rgb_colors_blend_per_channel() {
    let from = Color::Rgb {
        r: 100,
        g: 0,
        b: 40,
    };
    let to = Color::Rgb {
        r: 200,
        g: 100,
        b: 240,
    };
    assert_eq!(
        Color::lerp(from, to, 0.5),
        Color::Rgb {
            r: 150,
            g: 50,
            b: 140
        }
    );
    assert_eq!(Color::lerp(from, to, 0.0), from);
    assert_eq!(Color::lerp(from, to, 1.0), to);
}

#[test]
fn discrete_colors_snap_at_the_midpoint() {
    // Named colors have no honest in-between: they hold, then flip.
    assert_eq!(Color::lerp(Color::Red, Color::Blue, 0.4), Color::Red);
    assert_eq!(Color::lerp(Color::Red, Color::Blue, 0.5), Color::Blue);
    assert_eq!(Color::lerp(Color::Red, Color::Blue, 1.0), Color::Blue);
    // Mixed families snap the same way.
    let rgb = Color::Rgb {
        r: 10,
        g: 20,
        b: 30,
    };
    assert_eq!(Color::lerp(Color::Red, rgb, 0.2), Color::Red);
    assert_eq!(Color::lerp(Color::Red, rgb, 0.8), rgb);
    // Equal colors never snap away from themselves.
    assert_eq!(Color::lerp(Color::Cyan, Color::Cyan, 0.5), Color::Cyan);
}

// ---------------------------------------------------------------------------
// Tween: one value travelling from `from` to `to` on the clock.
// ---------------------------------------------------------------------------

#[test]
fn tween_reports_the_linear_value_at_elapsed_time() {
    let mut tween = Tween::new(0.0_f64, 100.0, ms(1_000));
    tween.advance(ms(400));
    assert!((tween.value() - 40.0).abs() < 1e-12);
    assert!(!tween.is_finished());
}

#[test]
fn tween_applies_its_easing_curve() {
    // ease(0.25) for OutCubic is 0.578125: the value leads the clock.
    let mut tween = Tween::new(0.0_f64, 100.0, ms(1_000)).easing(Easing::OutCubic);
    tween.advance(ms(250));
    assert!((tween.value() - 57.8125).abs() < 1e-12);
}

#[test]
fn tween_pins_at_the_destination_after_the_end() {
    let mut tween = Tween::new(0_i32, 100, ms(1_000));
    tween.advance(ms(5_000));
    assert_eq!(tween.value(), 100);
    assert!(tween.is_finished());
}

#[test]
fn tween_delay_holds_the_start_value() {
    let mut tween = Tween::new(0.0_f64, 100.0, ms(1_000)).delay(ms(200));
    tween.advance(ms(100));
    assert_eq!(
        tween.value(),
        0.0,
        "inside the delay the value rests at from"
    );
    tween.advance(ms(100));
    assert_eq!(
        tween.value(),
        0.0,
        "the delay boundary is still zero progress"
    );
    tween.advance(ms(300));
    assert!(
        (tween.value() - 30.0).abs() < 1e-12,
        "300 ms into a 1 s play"
    );
    assert!(!tween.is_finished());
    tween.advance(ms(700));
    assert!(tween.is_finished());
}

#[test]
fn tween_with_zero_duration_completes_immediately() {
    let mut tween = Tween::new(0.0_f64, 100.0, Duration::ZERO);
    tween.advance(Duration::ZERO);
    assert_eq!(tween.value(), 100.0);
    assert!(tween.is_finished());
}

#[test]
fn tween_value_is_independent_of_the_step_count() {
    // The same total elapsed time must produce the same value whether it
    // arrived in one jump or 400 frame-sized slices — the render loop's
    // tick cadence is none of the tween's business.
    let coarse = Tween::new(0.0_f64, 1.0, ms(400));
    let mut fine = Tween::new(0.0_f64, 1.0, ms(400));
    let mut coarse = coarse;
    coarse.advance(ms(400));
    for _ in 0..400 {
        fine.advance(ms(1));
    }
    assert_eq!(
        coarse.value(),
        fine.value(),
        "easing math is a pure function of elapsed"
    );

    let mut coarse_mid = Tween::new(0.0_f64, 1.0, ms(400)).easing(Easing::OutElastic);
    coarse_mid.advance(ms(137));
    let mut fine_mid = Tween::new(0.0_f64, 1.0, ms(400)).easing(Easing::OutElastic);
    for _ in 0..137 {
        fine_mid.advance(ms(1));
    }
    assert_eq!(coarse_mid.value(), fine_mid.value());
}

#[test]
fn tween_count_repeat_plays_n_times_then_exhausts() {
    let mut tween = Tween::new(0_i32, 100, ms(100)).repeat(Repeat::Count(3));
    tween.advance(ms(250));
    assert_eq!(
        tween.value(),
        50,
        "250 ms = two full cycles + half of the third"
    );
    assert!(!tween.is_finished());
    tween.advance(ms(50));
    assert_eq!(tween.value(), 100);
    assert!(tween.is_finished(), "3 x 100 ms of play have elapsed");
    // Finished tweens keep answering with the rest position.
    tween.advance(ms(1_000));
    assert_eq!(tween.value(), 100);
    assert!(tween.is_finished());
}

#[test]
fn tween_infinite_repeat_never_exhausts() {
    let mut tween = Tween::new(0_i32, 100, ms(100)).repeat(Repeat::Infinite);
    tween.advance(ms(1_500));
    // One boundary rule for every schedule: exactly on a cycle boundary
    // the tween reads as the END of the previous cycle (this is what
    // keeps alternate tweens continuous at every bounce apex).
    assert_eq!(tween.value(), 100);
    assert!(!tween.is_finished());
    tween.advance(ms(25));
    assert_eq!(tween.value(), 25);
}

#[test]
fn tween_alternate_bounces_back_to_the_start() {
    let mut tween = Tween::new(0_i32, 100, ms(100))
        .repeat(Repeat::Count(2))
        .alternate();
    tween.advance(ms(50));
    assert_eq!(tween.value(), 50, "forward cycle climbs");
    tween.advance(ms(50));
    assert_eq!(tween.value(), 100, "cycle boundary stays continuous");
    tween.advance(ms(50));
    assert_eq!(
        tween.value(),
        50,
        "the reverse cycle plays the same curve backwards"
    );
    tween.advance(ms(50));
    assert_eq!(
        tween.value(),
        0,
        "two cycles rest where the second one ends: from"
    );
    assert!(tween.is_finished());
}

#[test]
fn tween_alternate_odd_count_rests_at_the_destination() {
    let mut tween = Tween::new(0_i32, 100, ms(100))
        .repeat(Repeat::Count(1))
        .alternate();
    tween.advance(ms(100));
    assert_eq!(tween.value(), 100);
    assert!(tween.is_finished());
}

#[test]
fn tween_reset_returns_to_the_beginning() {
    let mut tween = Tween::new(0.0_f64, 100.0, ms(1_000));
    tween.advance(ms(400));
    tween.reset();
    assert_eq!(tween.value(), 0.0);
    assert!(!tween.is_finished());
    tween.advance(ms(1_000));
    assert!(tween.is_finished());
}

#[test]
fn tween_exposes_its_elapsed_budget() {
    let mut tween = Tween::new(0.0_f64, 1.0, ms(1_000));
    tween.advance(ms(123));
    assert_eq!(tween.elapsed(), ms(123));
}

// ---------------------------------------------------------------------------
// Animator: many keyed tweens behind one O(1) idle decision.
// ---------------------------------------------------------------------------

#[test]
fn animator_reports_values_for_running_tweens_only() {
    let mut animator = Animator::new();
    animator.start("gauge", Tween::new(0.0_f64, 1.0, ms(1_000)));
    assert!((animator.value(&"gauge").unwrap() - 0.0).abs() < 1e-12);
    let cancelled = animator.cancel(&"gauge");
    assert!(cancelled.is_some());
    assert_eq!(
        animator.value(&"gauge"),
        None,
        "a cancelled tween stops answering"
    );
    assert_eq!(
        animator.cancel(&"gauge"),
        None,
        "cancelling twice is a no-op"
    );
}

#[test]
fn animator_starting_the_same_key_replaces_the_tween() {
    let mut animator = Animator::new();
    animator.start("x", Tween::new(0.0_f64, 100.0, ms(1_000)));
    animator.advance(ms(500));
    animator.start("x", Tween::new(50.0_f64, 60.0, ms(1_000)));
    assert!(
        (animator.value(&"x").unwrap() - 50.0).abs() < 1e-12,
        "the new tween owns the key"
    );
    assert_eq!(animator.len(), 1);
}

#[test]
fn animator_advance_returns_the_sorted_finished_keys_once() {
    let mut animator = Animator::new();
    animator.start('c', Tween::new(0_i32, 10, ms(30)));
    animator.start('a', Tween::new(0_i32, 10, ms(10)));
    animator.start('b', Tween::new(0_i32, 10, ms(20)));
    assert!(!animator.is_idle());

    assert_eq!(
        animator.advance(ms(20)),
        ['a', 'b'],
        "sorted, not hash order"
    );
    assert_eq!(
        animator.advance(ms(0)),
        Vec::<char>::new(),
        "no double reporting"
    );
    assert_eq!(
        animator.value(&'a'),
        Some(10),
        "finished tweens pin their rest value"
    );
    assert_eq!(animator.advance(ms(10)), ['c']);
    assert!(animator.is_idle(), "everything finished: no tick is owed");
    assert_eq!(
        animator.len(),
        3,
        "finished tweens stay pinned until replaced"
    );
    assert_eq!(animator.advance(ms(100)), Vec::<char>::new());
}

#[test]
fn animator_is_running_distinguishes_pinned_from_playing() {
    let mut animator = Animator::new();
    animator.start('x', Tween::new(0_i32, 10, ms(10)));
    assert!(animator.is_running(&'x'));
    animator.advance(ms(10));
    assert!(
        !animator.is_running(&'x'),
        "pinned: finished but still queryable"
    );
    assert_eq!(animator.value(&'x'), Some(10));
    assert!(!animator.is_running(&'y'));
}

#[test]
fn animator_infinite_tween_keeps_the_engine_awake() {
    let mut animator = Animator::new();
    animator.start(
        "pulse",
        Tween::new(0_i32, 100, ms(100)).repeat(Repeat::Infinite),
    );
    for _ in 0..50 {
        animator.advance(ms(100));
    }
    assert!(!animator.is_idle(), "a looping tween is never done");
    animator.cancel(&"pulse");
    assert!(
        animator.is_idle(),
        "cancelling the loop puts the engine back to sleep"
    );
}

#[test]
fn animator_advance_on_empty_is_free() {
    let mut animator: Animator<&'static str, f64> = Animator::new();
    assert!(animator.is_empty());
    assert!(animator.is_idle());
    assert_eq!(animator.advance(ms(16)), Vec::<&str>::new());
    animator.clear();
    assert!(animator.is_empty());
}

#[test]
fn animator_mixed_finishes_and_survivors() {
    let mut animator = Animator::new();
    animator.start("done", Tween::new(0.0_f64, 1.0, ms(10)));
    animator.start("slow", Tween::new(0.0_f64, 1.0, ms(1_000)));
    animator.start(
        "loop",
        Tween::new(0.0_f64, 1.0, ms(10)).repeat(Repeat::Infinite),
    );

    assert_eq!(animator.advance(ms(10)), ["done"]);
    assert!((animator.value(&"loop").unwrap() - 1.0).abs() < 1e-12);
    assert!(!animator.is_idle());
    assert_eq!(animator.len(), 3);
}

// ---------------------------------------------------------------------------
// B16 — the motion gate: 1,000 concurrent tweens must advance in well
// under a frame. Timing gate: `cargo test -p tuvix-core --release --ignored`.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b16_one_thousand_concurrent_tweens_advance_under_one_millisecond() {
    let mut animator = Animator::new();
    for index in 0..1_000_u32 {
        let from = f64::from(index % 7);
        animator.start(
            index,
            Tween::new(from, from + 10.0, ms(10_000))
                .easing(Easing::OutCubic)
                .repeat(Repeat::Infinite)
                .alternate(),
        );
    }
    assert!(!animator.is_idle());

    let mut worst = Duration::ZERO;
    let mut total = Duration::ZERO;
    for _ in 0..600 {
        let started = std::time::Instant::now();
        let finished = animator.advance(ms(16));
        let spent = started.elapsed();
        assert!(
            finished.is_empty(),
            "a 10 s loop never finishes inside one second"
        );
        worst = worst.max(spent);
        total += spent;
    }
    assert!(
        worst <= Duration::from_millis(1),
        "the worst 1,000-tween advance took {worst:?} (budget: 1 ms per frame)"
    );
    let mean = total / 600;
    println!("B16: worst advance {worst:?}, mean {mean:?} for 1,000 concurrent tweens");

    // Deterministic post-conditions: values stayed inside their lanes.
    for index in 0..1_000_u32 {
        let value = animator.value(&index).expect("infinite tween stays");
        assert!(value.is_finite() && (0.0..=17.0).contains(&value));
    }
}
