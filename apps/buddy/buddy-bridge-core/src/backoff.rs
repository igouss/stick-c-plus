//! The reconnect backoff schedule — a pure function of the consecutive-failure count.
//!
//! BlueZ does not auto-reconnect a central (Handoff 1), so the bridge owns the retry cadence.
//! The delay doubles each failure from [`BASE`] and clamps at [`MAX`], so a flapping link
//! settles into a gentle poll instead of hammering the adapter, while a one-off drop
//! reconnects almost immediately. `attempt` is injected by the caller (the FSM), so this stays
//! clock-free and host-tested.

use std::time::Duration;

/// The first (and smallest) backoff: a dropped link retries almost at once.
pub const BASE: Duration = Duration::from_millis(250);

/// The ceiling: however long the link stays down, retries never space out beyond this.
pub const MAX: Duration = Duration::from_secs(30);

/// Capped exponential backoff: `BASE · 2^attempt`, clamped to [`MAX`].
///
/// `attempt` is the count of consecutive failures so far (0 for the first retry). The shift is
/// saturated well before it could overflow, and the result is clamped, so every input — including
/// a pathologically large `attempt` — returns a valid `Duration` at most [`MAX`].
pub fn backoff(attempt: u32) -> Duration {
    // 250 ms << 7 = 32 s already exceeds the 30 s ceiling, so there is no reason to shift
    // further; capping the shift also keeps the multiply far from overflow.
    let shift: u32 = attempt.min(7);
    let millis: u64 = (BASE.as_millis() as u64).saturating_mul(1u64 << shift);
    Duration::from_millis(millis).min(MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn the_first_attempt_is_the_base_delay() {
        assert_eq!(backoff(0), BASE);
    }

    #[test]
    fn one_failure_doubles_the_delay() {
        assert_eq!(backoff(1), Duration::from_millis(500));
    }

    #[test]
    fn many_failures_clamp_at_the_ceiling() {
        assert_eq!(backoff(1_000_000), MAX);
    }

    proptest! {
        /// Non-decreasing in the failure count — a longer outage never retries sooner.
        #[test]
        fn backoff_is_monotonic(attempt in 0u32..40) {
            prop_assert!(backoff(attempt) <= backoff(attempt + 1));
        }

        /// Always within the documented band, for every possible input.
        #[test]
        fn backoff_stays_within_the_band(attempt in any::<u32>()) {
            let delay: Duration = backoff(attempt);
            prop_assert!(delay >= BASE);
            prop_assert!(delay <= MAX);
        }
    }
}
