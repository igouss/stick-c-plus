//! Percent — the domain's `0..=100` value object for a CPU or memory reading.
//!
//! Both facts the host monitor shows — how busy the CPU is, how full memory is —
//! are percentages, so they share one newtype whose invariant (`0..=100`) is
//! enforced at construction. `0` is idle/empty, `100` is pegged/full. The graph
//! plots the raw byte, so a `Percent` is exactly one column's height.

/// A CPU or memory reading as a percentage, `0..=100`.
///
/// A newtype over `u8` whose invariant — the value is always `0..=100` — cannot be
/// bypassed: there is no way to build a `Percent` outside that range. This is what
/// lets the sparkline treat a value as a column height without re-clamping.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Percent(u8);

impl Percent {
    /// The idle / empty reading — also the safe fallback for arithmetic that cannot
    /// be computed (a degenerate memory total, a stalled CPU counter).
    pub const ZERO: Percent = Percent(0);

    /// A fully pegged / full reading.
    pub const FULL: Percent = Percent(100);

    /// Build a `Percent` from a value, rejecting anything above 100.
    ///
    /// Returns `None` rather than clamping: a value that fell outside the range is a
    /// caller bug worth surfacing, not something to paper over. Use [`clamped`] where
    /// a computed ratio must be *coerced* into range.
    ///
    /// [`clamped`]: Percent::clamped
    pub const fn new(value: u8) -> Option<Percent> {
        if value <= 100 {
            Some(Percent(value))
        } else {
            None
        }
    }

    /// Coerce a computed value into a `Percent`, clamping to `0..=100`.
    ///
    /// The constructor the rate arithmetic uses: a busy fraction or a memory ratio is
    /// computed as a wider number and then pinned into range here, so a rounding
    /// overshoot to `101` or a negative counter glitch can never escape the invariant.
    /// Takes an `i32` so a caller can hand it a value it has already rounded.
    pub const fn clamped(value: i32) -> Percent {
        // Bounded into `0..=100` with plain comparisons (`i32::clamp` is not yet
        // const), so the cast is always in `u8` range.
        let bounded: i32 = if value < 0 {
            0
        } else if value > 100 {
            100
        } else {
            value
        };
        Percent(bounded as u8)
    }

    /// The percentage, `0..=100`.
    pub const fn value(self) -> u8 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn new_rejects_above_one_hundred() {
        assert_eq!(Percent::new(101), None);
        assert_eq!(Percent::new(255), None);
    }

    #[test]
    fn new_accepts_the_endpoints() {
        assert_eq!(Percent::new(0), Some(Percent::ZERO));
        assert_eq!(Percent::new(100), Some(Percent::FULL));
    }

    #[test]
    fn clamped_pins_out_of_range_values() {
        assert_eq!(Percent::clamped(-7), Percent::ZERO);
        assert_eq!(Percent::clamped(142), Percent::FULL);
        assert_eq!(Percent::clamped(37).value(), 37);
    }

    proptest! {
        /// Whatever the input, `clamped` yields a valid percentage — the invariant
        /// the sparkline relies on.
        #[test]
        fn clamped_is_always_a_valid_percent(value in any::<i32>()) {
            prop_assert!(Percent::clamped(value).value() <= 100);
        }

        /// An in-range value survives `new` unchanged.
        #[test]
        fn new_round_trips_in_range(value in 0u8..=100) {
            prop_assert_eq!(Percent::new(value).map(Percent::value), Some(value));
        }
    }
}
