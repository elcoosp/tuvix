//! The motion layer: time-driven animation primitives.
//!
//! Everything here is a pure function of elapsed time. A [`Tween`]'s value
//! depends on the clock, never on the number of ticks that produced it, so
//! a 60 fps render loop and a 5 fps one land on bit-identical values. The
//! [`Animator`] ties many keyed tweens together and answers one question
//! in O(1) — *is anything still moving?* — which is the architecture's
//! static-CPU budget as a type: an app that stops asking for ticks when
//! [`Animator::is_idle`] returns true burns no cycles on still frames.
//!
//! # Examples
//!
//! ```
//! use std::time::Duration;
//!
//! use tuvix_core::anim::{Easing, Tween};
//!
//! let mut tween = Tween::new(0.0_f64, 100.0, Duration::from_secs(1))
//!     .easing(Easing::OutCubic);
//! tween.advance(Duration::from_millis(250));
//! // ease(0.25) for OutCubic is 0.578125: the value leads the clock.
//! assert!((tween.value() - 57.8125).abs() < 1e-9);
//! ```

use std::collections::HashMap;
use std::hash::Hash;
use std::time::Duration;

use crate::style::Color;

/// A progress value clamped to the unit interval `[0.0, 1.0]`.
///
/// Every curve in [`Easing`] and every [`Interpolate::lerp`] works in this
/// domain; the constructor clamps so extrapolation is unrepresentable.
/// `NaN` clamps to zero: a broken clock reads as "not started", never as
/// undefined behavior downstream.
#[derive(Debug, Clone, Copy, Default, PartialEq, PartialOrd)]
pub struct Progress(f64);

impl Progress {
    /// No progress: the start of the segment.
    pub const ZERO: Self = Self(0.0);
    /// Complete: the end of the segment.
    pub const ONE: Self = Self(1.0);

    /// Clamps `value` into `[0.0, 1.0]`; `NaN` becomes zero.
    ///
    /// ```
    /// use tuvix_core::anim::Progress;
    ///
    /// assert_eq!(Progress::new(1.5), Progress::ONE);
    /// assert_eq!(Progress::new(-0.5), Progress::ZERO);
    /// ```
    #[must_use]
    pub fn new(value: f64) -> Self {
        if value.is_nan() {
            return Self::ZERO;
        }
        Self(value.clamp(0.0, 1.0))
    }

    /// The fraction `elapsed / duration`, clamped. A zero duration is
    /// already complete.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use tuvix_core::anim::Progress;
    ///
    /// let quarter = Progress::of_elapsed(Duration::from_millis(250), Duration::from_secs(1));
    /// assert_eq!(quarter.value(), 0.25);
    /// ```
    #[must_use]
    pub fn of_elapsed(elapsed: Duration, duration: Duration) -> Self {
        if duration.is_zero() {
            return Self::ONE;
        }
        Self::new(elapsed.as_secs_f64() / duration.as_secs_f64())
    }

    /// The clamped value as `f64`.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }

    /// `1.0 - value`: the same segment played in reverse.
    #[must_use]
    pub const fn complement(self) -> Self {
        Self(1.0 - self.0)
    }
}

/// The curve catalog applied to [`Progress`] before interpolation.
///
/// All sixteen curves fix both endpoints exactly (`0 → 0`, `1 → 1`); the
/// formulas are the standard easings.net set, pinned by the M13 test
/// suite against an independent reference. `OutBack` and `OutElastic`
/// overshoot the interval in the middle; `OutBounce` dips; the other
/// thirteen are monotone.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Easing {
    /// Constant speed.
    #[default]
    Linear,
    /// Accelerating from zero: `p²`.
    InQuad,
    /// Decelerating to zero: `1 − (1−p)²`.
    OutQuad,
    /// Accelerate then decelerate.
    InOutQuad,
    /// Accelerating from zero: `p³`.
    InCubic,
    /// Decelerating to zero: `1 − (1−p)³`.
    OutCubic,
    /// Accelerate then decelerate, cubically.
    InOutCubic,
    /// Decelerating to zero: `1 − (1−p)⁴`.
    OutQuart,
    /// Accelerate then decelerate, quartically.
    InOutQuart,
    /// Decelerating on a sine quarter-wave.
    OutSine,
    /// Symmetric sine ease-in-out.
    InOutSine,
    /// Exponential deceleration: `1 − 2^(−10p)`.
    OutExpo,
    /// Symmetric exponential ease-in-out.
    InOutExpo,
    /// Overshoots the destination by ~10% (the classic 1.70158 back
    /// constant), then settles.
    OutBack,
    /// Elastic decay: overshoots to ~1.37 and rings down to rest.
    OutElastic,
    /// A ball landing: three decaying bounces, never leaving `[0, 1]`.
    OutBounce,
}

impl Easing {
    /// Maps raw progress through this curve, returning the eased
    /// coefficient. The result is in `[0.0, 1.0]` for thirteen curves and
    /// deliberately overshoots it for [`Easing::OutBack`] and
    /// [`Easing::OutElastic`] — that is their point — so consumers that
    /// cannot overshoot must clamp, and [`Interpolate`] implementations
    /// must stay well-defined when it happens.
    ///
    /// ```
    /// use tuvix_core::anim::{Easing, Progress};
    ///
    /// let half = Progress::new(0.5);
    /// assert_eq!(Easing::Linear.ease(half), 0.5);
    /// assert_eq!(Easing::InQuad.ease(half), 0.25);
    /// assert_eq!(Easing::OutQuad.ease(half), 0.75);
    /// assert!(Easing::OutBack.ease(half) > 1.0, "the overshoot survives");
    /// ```
    #[must_use]
    pub fn ease(self, p: Progress) -> f64 {
        let x = p.value();
        match self {
            Self::Linear => x,
            Self::InQuad => x * x,
            Self::OutQuad => 1.0 - (1.0 - x) * (1.0 - x),
            Self::InOutQuad => {
                if x < 0.5 {
                    2.0 * x * x
                } else {
                    1.0 - 2.0 * (1.0 - x) * (1.0 - x)
                }
            }
            Self::InCubic => x * x * x,
            Self::OutCubic => 1.0 - (1.0 - x).powi(3),
            Self::InOutCubic => {
                if x < 0.5 {
                    4.0 * x.powi(3)
                } else {
                    1.0 - (-2.0 * x + 2.0).powi(3) / 2.0
                }
            }
            Self::OutQuart => 1.0 - (1.0 - x).powi(4),
            Self::InOutQuart => {
                if x < 0.5 {
                    8.0 * x.powi(4)
                } else {
                    1.0 - (-2.0 * x + 2.0).powi(4) / 2.0
                }
            }
            Self::OutSine => f64::sin(x * std::f64::consts::FRAC_PI_2),
            Self::InOutSine => -(f64::cos(std::f64::consts::PI * x) - 1.0) / 2.0,
            Self::OutExpo => {
                if x >= 1.0 {
                    1.0
                } else {
                    1.0 - 2.0_f64.powf(-10.0 * x)
                }
            }
            Self::InOutExpo => {
                if x <= 0.0 {
                    0.0
                } else if x >= 1.0 {
                    1.0
                } else if x < 0.5 {
                    2.0_f64.powf(20.0 * x - 10.0) / 2.0
                } else {
                    (2.0 - 2.0_f64.powf(-20.0 * x + 10.0)) / 2.0
                }
            }
            Self::OutBack => {
                // easings.net constants: c1 = 1.70158, c3 = c1 + 1. The
                // endpoints are guarded exactly: the closed form at x=0
                // is 1 - c3 + c1, which float arithmetic evaluates to
                // 2.2e-16, not zero.
                const C1: f64 = 1.701_58;
                const C3: f64 = C1 + 1.0;
                if x <= 0.0 {
                    0.0
                } else if x >= 1.0 {
                    1.0
                } else {
                    1.0 + C3 * (x - 1.0).powi(3) + C1 * (x - 1.0).powi(2)
                }
            }
            Self::OutElastic => {
                if x <= 0.0 {
                    0.0
                } else if x >= 1.0 {
                    1.0
                } else {
                    2.0_f64.powf(-10.0 * x)
                        * f64::sin((x * 10.0 - 0.75) * (2.0 * std::f64::consts::PI / 3.0))
                        + 1.0
                }
            }
            Self::OutBounce => {
                // easings.net piecewise: n1 = 7.5625, d1 = 2.75, with the
                // offsets divided by d1.
                const N1: f64 = 7.5625;
                const D1: f64 = 2.75;
                if x < 1.0 / D1 {
                    N1 * x * x
                } else if x < 2.0 / D1 {
                    let x = x - 1.5 / D1;
                    N1 * x * x + 0.75
                } else if x < 2.5 / D1 {
                    let x = x - 2.25 / D1;
                    N1 * x * x + 0.9375
                } else {
                    let x = x - 2.625 / D1;
                    N1 * x * x + 0.984_375
                }
            }
        }
    }
}

/// A value that can travel between two endpoints.
///
/// The `p` argument is the *eased coefficient* — [`Easing::ease`]'s
/// output — which is normally in `[0.0, 1.0]` but deliberately
/// overshoots for the back and elastic curves, so implementations must
/// stay well-defined outside the unit interval (the integer and channel
/// casts saturate). Implemented for the float and integer primitives and
/// for [`Color`]: RGB colors blend per channel, while discrete colors
/// (the ANSI palette, indexed entries) have no honest in-between and
/// snap at the midpoint.
pub trait Interpolate {
    /// The point `p` of the way from `from` to `to`.
    fn lerp(from: Self, to: Self, p: f64) -> Self;
}

impl Interpolate for f64 {
    fn lerp(from: Self, to: Self, p: f64) -> Self {
        from + (to - from) * p
    }
}

impl Interpolate for f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "f32 easing is the lossy variant by definition: narrowing \
                  the f64 coefficient once per lerp call is the documented \
                  M13 contract, and std has no fallible f64-to-f32 step"
    )]
    fn lerp(from: Self, to: Self, p: f64) -> Self {
        from + (to - from) * p as f32
    }
}

macro_rules! impl_interpolate_signed {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Interpolate for $ty {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "the saturating float-to-int cast is the M13 lerp \
                              contract: NaN becomes 0 and overshooting \
                              coefficients clamp at the type bounds; std has \
                              no lossless f64-to-int conversion"
                )]
                fn lerp(from: Self, to: Self, p: f64) -> Self {
                    let start = f64::from(from);
                    let end = f64::from(to);
                    (start + (end - start) * p).round() as Self
                }
            }
        )*
    };
}

macro_rules! impl_interpolate_unsigned {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Interpolate for $ty {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "the saturating float-to-int cast is the M13 lerp \
                              contract: NaN becomes 0 and overshooting \
                              coefficients clamp at the type bounds; std has \
                              no lossless f64-to-int conversion"
                )]
                fn lerp(from: Self, to: Self, p: f64) -> Self {
                    let start = f64::from(from);
                    let end = f64::from(to);
                    (start + (end - start) * p).round() as Self
                }
            }
        )*
    };
}

macro_rules! impl_interpolate_wide_signed {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Interpolate for $ty {
                #[expect(
                    clippy::cast_precision_loss,
                    clippy::cast_possible_truncation,
                    reason = "64-bit integers have no lossless f64 path (no \
                              `From<i64> for f64`); easing in the f64 domain \
                              is the documented M13 design, and the \
                              saturating float-to-int cast is the lerp \
                              contract"
                )]
                fn lerp(from: Self, to: Self, p: f64) -> Self {
                    let start = from as f64;
                    let end = to as f64;
                    (start + (end - start) * p).round() as Self
                }
            }
        )*
    };
}

macro_rules! impl_interpolate_wide_unsigned {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Interpolate for $ty {
                #[expect(
                    clippy::cast_precision_loss,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "64-bit integers have no lossless f64 path (no \
                              `From<i64> for f64`); easing in the f64 domain \
                              is the documented M13 design, and the \
                              saturating float-to-int cast is the lerp \
                              contract"
                )]
                fn lerp(from: Self, to: Self, p: f64) -> Self {
                    let start = from as f64;
                    let end = to as f64;
                    (start + (end - start) * p).round() as Self
                }
            }
        )*
    };
}

// One macro per integer family: which cast lints fire depends on the
// width and signedness of `$ty`, so each family carries exactly the
// `#[expect]` set its body provably triggers — an unfulfilled expectation
// is a hard error, keeping the exceptions honest.
impl_interpolate_signed!(i8, i16, i32);
impl_interpolate_unsigned!(u8, u16, u32);
impl_interpolate_wide_signed!(i64, isize);
impl_interpolate_wide_unsigned!(u64, usize);

impl Interpolate for Color {
    fn lerp(from: Self, to: Self, p: f64) -> Self {
        match (from, to) {
            (
                Self::Rgb {
                    r: r1,
                    g: g1,
                    b: b1,
                },
                Self::Rgb {
                    r: r2,
                    g: g2,
                    b: b2,
                },
            ) => {
                let blend = |a: u8, b: u8| -> u8 {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "the blend is clamped to 0.0..=255.0 first, so \
                                  the cast cannot truncate or lose its sign; \
                                  std has no lossless f64-to-u8 conversion"
                    )]
                    let blended = (f64::from(a) + (f64::from(b) - f64::from(a)) * p)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                    blended
                };
                Self::Rgb {
                    r: blend(r1, r2),
                    g: blend(g1, g2),
                    b: blend(b1, b2),
                }
            }
            // Discrete colors hold, then flip at the midpoint. Equal
            // colors never snap away from themselves.
            (a, b) => {
                if a == b || p < 0.5 {
                    a
                } else {
                    b
                }
            }
        }
    }
}

/// How many times a [`Tween`] plays its segment.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Repeat {
    /// Play once.
    #[default]
    None,
    /// Play exactly `count` times; `Count(0)` never plays and rests at
    /// the start value.
    Count(u32),
    /// Play forever; the tween never reports finished.
    Infinite,
}

/// One value travelling from `from` to `to` on the clock.
///
/// The value is a pure function of the accumulated [`Tween::elapsed`]:
/// advancing in one jump or a thousand slices produces identical values
/// (the M13 suite pins this). `advance` saturates, so a tween parked at
/// its destination costs a comparison per tick and nothing more.
///
/// ```
/// use std::time::Duration;
///
/// use tuvix_core::anim::{Repeat, Tween};
///
/// let mut tween = Tween::new(0_i32, 100, Duration::from_millis(1_000))
///     .repeat(Repeat::Count(3));
/// tween.advance(Duration::from_millis(1_500));
/// assert_eq!(tween.value(), 50, "two cycles plus half of the third");
/// assert!(!tween.is_finished());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Tween<T> {
    from: T,
    to: T,
    duration: Duration,
    delay: Duration,
    easing: Easing,
    repeat: Repeat,
    alternate: bool,
    elapsed: Duration,
}

impl<T> Tween<T> {
    /// A linear, undelayed, play-once tween from `from` to `to` over
    /// `duration`.
    #[must_use]
    pub fn new(from: T, to: T, duration: Duration) -> Self {
        Self {
            from,
            to,
            duration,
            delay: Duration::ZERO,
            easing: Easing::Linear,
            repeat: Repeat::None,
            alternate: false,
            elapsed: Duration::ZERO,
        }
    }

    /// Sets the curve applied to the raw progress.
    #[must_use]
    pub fn easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    /// Holds at the start value for `delay` before the play begins.
    #[must_use]
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Sets how many times the segment plays.
    #[must_use]
    pub fn repeat(mut self, repeat: Repeat) -> Self {
        self.repeat = repeat;
        self
    }

    /// Plays every second cycle in reverse (there and back). With
    /// `alternate`, an exhausted even cycle count rests at the start
    /// value, an odd one at the destination.
    #[must_use]
    pub fn alternate(mut self) -> Self {
        self.alternate = true;
        self
    }

    /// Adds `dt` to the tween's clock, saturating at the end.
    pub fn advance(&mut self, dt: Duration) {
        self.elapsed = self.elapsed.saturating_add(dt);
    }

    /// Rewinds the clock to zero (the configuration is kept).
    pub fn reset(&mut self) {
        self.elapsed = Duration::ZERO;
    }

    /// The accumulated play time so far, delay included.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// The number of play cycles the configuration asks for, or `None`
    /// for [`Repeat::Infinite`].
    fn cycles_total(&self) -> Option<u32> {
        match self.repeat {
            Repeat::None => Some(1),
            Repeat::Count(count) => Some(count),
            Repeat::Infinite => None,
        }
    }

    /// Whether the tween has played its full budget and now rests at its
    /// final position. A zero-duration tween completes immediately
    /// (including an infinite one: there is nothing to loop over);
    /// `Count(0)` completes without ever playing.
    pub fn is_finished(&self) -> bool {
        if self.duration.is_zero() {
            return true;
        }
        let Some(cycles) = self.cycles_total() else {
            return false;
        };
        if cycles == 0 {
            return self.elapsed >= self.delay;
        }
        let total = self.duration.checked_mul(cycles).unwrap_or(Duration::MAX);
        self.elapsed >= self.delay.saturating_add(total)
    }
}

impl<T: Interpolate + Copy> Tween<T> {
    /// The value at the current clock position.
    ///
    /// At a cycle boundary the tween reads as the *end* of the previous
    /// cycle, so play-once tweens rest at `to`, alternate tweens are
    /// continuous across every bounce, and exhausted tweens rest where
    /// their last cycle ended.
    #[must_use]
    pub fn value(&self) -> T {
        let t = self.elapsed.saturating_sub(self.delay);
        if self.duration.is_zero() {
            return T::lerp(self.from, self.to, 1.0);
        }
        let Some(cycles) = self.cycles_total() else {
            return self.value_of_cycle_position(t, false);
        };
        if cycles == 0 {
            // Never played: rest at the start value.
            return T::lerp(self.from, self.to, 0.0);
        }
        let total_nanos = u128::from(cycles).saturating_mul(self.duration.as_nanos());
        let t_nanos = t.as_nanos();
        if t_nanos >= total_nanos {
            // Exhausted: rest where the last cycle ended. Every curve
            // fixes its endpoints, so the rest position needs no easing.
            let last_reversed = self.alternate && (cycles - 1) % 2 == 1;
            let rest = if last_reversed { 0.0 } else { 1.0 };
            return T::lerp(self.from, self.to, rest);
        }
        self.value_of_cycle_position(t, self.alternate)
    }

    /// The value at play time `t` inside an unbounded (or still-running)
    /// schedule; `alternating` flips every second cycle.
    fn value_of_cycle_position(&self, t: Duration, alternating: bool) -> T {
        let d_nanos = self.duration.as_nanos();
        let mut cycle = t.as_nanos() / d_nanos;
        let mut local = t.as_nanos() % d_nanos;
        // Exactly on a boundary, read as the end of the previous cycle —
        // never as the start of the next.
        if local == 0 && cycle > 0 {
            cycle -= 1;
            local = d_nanos;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "the cycle position is a duration ratio in f64 by \
                      design (docs/BENCHMARKS.md B-scores pin this easing \
                      domain); u128 nanoseconds exceed the f64 mantissa \
                      deliberately"
        )]
        let mut raw = local as f64 / d_nanos as f64;
        if alternating && cycle % 2 == 1 {
            raw = 1.0 - raw;
        }
        T::lerp(self.from, self.to, self.easing.ease(Progress::new(raw)))
    }
}

/// Many keyed [`Tween`]s behind one O(1) idle decision.
///
/// `advance` walks every tween, reports the keys that finished this tick
/// in sorted order (deterministic, whatever the hash order did), and
/// *pins* finished tweens at their rest position: they keep answering
/// [`Animator::value`] until replaced, cancelled or cleared, but they no
/// longer count as running.
///
/// ```
/// use std::time::Duration;
///
/// use tuvix_core::anim::{Animator, Tween};
///
/// let mut animator = Animator::new();
/// animator.start('x', Tween::new(0_i32, 10, Duration::from_millis(10)));
/// assert!(!animator.is_idle());
/// assert_eq!(animator.advance(Duration::from_millis(10)), ['x']);
/// assert!(animator.is_idle(), "nothing moves: no tick is owed");
/// assert_eq!(animator.value(&'x'), Some(10), "the rest position stays readable");
/// ```
#[derive(Debug)]
pub struct Animator<K, T> {
    tweens: HashMap<K, Tween<T>>,
    running: usize,
}

impl<K, T> Default for Animator<K, T> {
    fn default() -> Self {
        Self {
            tweens: HashMap::new(),
            running: 0,
        }
    }
}

impl<K: Copy + Eq + Hash + Ord, T: Interpolate + Copy> Animator<K, T> {
    /// An empty animator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts (or replaces) the tween bound to `key`.
    pub fn start(&mut self, key: K, tween: Tween<T>) {
        let new_running = usize::from(!tween.is_finished());
        if let Some(old) = self.tweens.insert(key, tween) {
            self.running += new_running - usize::from(!old.is_finished());
        } else {
            self.running += new_running;
        }
    }

    /// Removes and returns the tween bound to `key`, if any.
    pub fn cancel(&mut self, key: &K) -> Option<Tween<T>> {
        let old = self.tweens.remove(key)?;
        self.running -= usize::from(!old.is_finished());
        Some(old)
    }

    /// The pinned or current value of the tween bound to `key`.
    #[must_use]
    pub fn value(&self, key: &K) -> Option<T> {
        self.tweens.get(key).map(Tween::value)
    }

    /// Whether `key` currently owns an unfinished tween.
    #[must_use]
    pub fn is_running(&self, key: &K) -> bool {
        self.tweens
            .get(key)
            .is_some_and(|tween| !tween.is_finished())
    }

    /// Advances every tween by `dt` and returns the keys that finished
    /// *this* tick, sorted. Finished tweens stay pinned at their rest
    /// value and are never reported twice.
    pub fn advance(&mut self, dt: Duration) -> Vec<K> {
        let mut finished = Vec::new();
        for (key, tween) in &mut self.tweens {
            let was_running = !tween.is_finished();
            tween.advance(dt);
            if was_running && tween.is_finished() {
                finished.push(*key);
            }
        }
        self.running = self.running.saturating_sub(finished.len());
        finished.sort_unstable();
        finished
    }

    /// Whether no tween is running — the render loop may stop ticking.
    /// O(1): a counter, not a scan.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.running == 0
    }

    /// How many tweens are tracked, finished ones included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tweens.len()
    }

    /// Whether no tween is tracked at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tweens.is_empty()
    }

    /// Forgets every tween.
    pub fn clear(&mut self) {
        self.tweens.clear();
        self.running = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_display_and_equality_agree() {
        assert_eq!(Progress::default(), Progress::ZERO);
        assert!(Progress::new(0.2) < Progress::new(0.8));
    }

    #[test]
    fn linear_easing_is_the_identity_across_the_domain() {
        for step in 0..=100 {
            let raw = f64::from(step) / 100.0;
            assert_eq!(Easing::Linear.ease(Progress::new(raw)), raw);
        }
    }

    #[test]
    fn tween_advance_saturates_past_the_end() {
        let mut tween = Tween::new(0_i32, 1, Duration::from_secs(1));
        tween.advance(Duration::MAX);
        tween.advance(Duration::MAX);
        assert!(tween.is_finished());
        assert_eq!(tween.value(), 1);
    }

    #[test]
    fn tween_count_zero_never_plays() {
        let mut tween = Tween::new(0_i32, 100, Duration::from_millis(100)).repeat(Repeat::Count(0));
        assert!(tween.is_finished(), "a zero-count tween has no budget");
        tween.advance(Duration::from_millis(50));
        assert_eq!(tween.value(), 0, "it rests at the start value");
    }

    #[test]
    fn animator_replace_recount_running_bookkeeping() {
        let mut animator = Animator::new();
        animator.start('a', Tween::new(0_i32, 1, Duration::from_secs(1)));
        animator.advance(Duration::from_secs(1));
        assert!(animator.is_idle(), "the old tween finished");
        // Replacing a finished tween with a running one wakes the engine.
        animator.start('a', Tween::new(0_i32, 1, Duration::from_secs(1)));
        assert!(!animator.is_idle());
        // Replacing a running tween keeps exactly one running.
        animator.start('a', Tween::new(2_i32, 3, Duration::from_secs(1)));
        assert!(!animator.is_idle());
        animator.cancel(&'a');
        assert!(animator.is_idle());
        assert!(animator.is_empty());
    }
}
