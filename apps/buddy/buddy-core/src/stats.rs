//! The stats math: the velocity ring and mood tier, the energy decay, the level/fed
//! progress, and the three-case token-delta latch.
//!
//! All pure arithmetic. Time enters only as an already-computed `hours_since_nap_end`, never
//! as a clock read. Nothing here is persisted except at the level-up milestone (an adapter
//! concern); the token latch is explicitly RAM-only, which is what stops a reboot re-crediting
//! a whole desktop session.

use platform_core::Tick;

/// Tokens earned per level.
pub const TOKENS_PER_LEVEL: u32 = 50_000;

/// The velocity history: a ring of eight `u16` samples, reporting the **upper** median.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VelocityRing {
    samples: [u16; 8],
    count: usize,
    next: usize,
}

impl VelocityRing {
    /// An empty ring — [`median`](VelocityRing::median) reads `0` until the first push.
    pub const fn new() -> Self {
        VelocityRing {
            samples: [0; 8],
            count: 0,
            next: 0,
        }
    }

    /// Record one velocity sample, overwriting the oldest once eight are held.
    pub fn push(&mut self, velocity: u16) {
        self.samples[self.next] = velocity;
        self.next = (self.next + 1) % self.samples.len();
        if self.count < self.samples.len() {
            self.count += 1;
        }
    }

    /// The **upper** median of the held samples: sort the first `count`, return `tmp[count/2]`;
    /// `0` when empty.
    pub fn median(&self) -> u16 {
        if self.count == 0 {
            return 0;
        }
        let mut tmp: [u16; 8] = self.samples;
        tmp[..self.count].sort_unstable();
        tmp[self.count / 2]
    }
}

impl Default for VelocityRing {
    fn default() -> Self {
        Self::new()
    }
}

/// The mood tier `0..=4` from the median velocity, with a deny-rate penalty.
///
/// Base from velocity, first match wins: `vel == 0 → 2` (neutral, no data); `< 15 → 4`;
/// `< 30 → 3`; `< 60 → 2`; `< 120 → 1`; else `0`. Then, once `approvals + denials >= 3`: if
/// `denials > approvals` subtract 2; else if `denials * 2 > approvals` (deny rate over a
/// third) subtract 1. Finally clamp low at `0`.
pub fn mood_tier(median_velocity: u16, approvals: u32, denials: u32) -> u8 {
    let mut tier: i32 = if median_velocity == 0 {
        2
    } else if median_velocity < 15 {
        4
    } else if median_velocity < 30 {
        3
    } else if median_velocity < 60 {
        2
    } else if median_velocity < 120 {
        1
    } else {
        0
    };
    // Widen to avoid any overflow when the counts are large.
    let approvals_wide: u64 = u64::from(approvals);
    let denials_wide: u64 = u64::from(denials);
    if approvals_wide + denials_wide >= 3 {
        if denials_wide > approvals_wide {
            tier -= 2;
        } else if denials_wide * 2 > approvals_wide {
            tier -= 1;
        }
    }
    tier.max(0) as u8
}

/// The energy that a nap-exit restores, and the ceiling of the energy tier.
pub const ENERGY_AT_NAP_FULL: u8 = 5;

/// The energy tier `0..=5`: `energy_at_nap - (hours_since_nap_end / 2)`, clamped to `0..=5`.
///
/// Integer division, so the step lands at exactly 2 h, 4 h and 6 h. `energy_at_nap` is set to
/// [`ENERGY_AT_NAP_FULL`] only on a nap-exit; nothing else refills it.
pub fn energy_tier(energy_at_nap: u8, hours_since_nap_end: u64) -> u8 {
    let decay: i64 = (hours_since_nap_end / 2) as i64;
    let energy: i64 = i64::from(energy_at_nap) - decay;
    energy.clamp(0, i64::from(ENERGY_AT_NAP_FULL)) as u8
}

/// Hours since a nap ended, from a millisecond span: integer division by 3_600_000.
pub fn hours_since(nap_end: Tick, now: Tick) -> u64 {
    now.saturating_sub(nap_end) / 3_600_000
}

/// The level for a token total: `tokens / `[`TOKENS_PER_LEVEL`].
pub fn level(tokens: u32) -> u32 {
    tokens / TOKENS_PER_LEVEL
}

/// The fed progress within the current level: `(tokens % `[`TOKENS_PER_LEVEL`]`) / 5000`,
/// in `0..=9` — it never reaches 10.
pub fn fed_progress(tokens: u32) -> u8 {
    ((tokens % TOKENS_PER_LEVEL) / 5_000) as u8
}

/// The three-case token-delta latch: turns a running desktop total into a per-observation
/// credit, RAM-only so a device reboot cannot re-credit a whole session.
///
/// The three cases, in order:
/// 1. **boot** — not yet synced: adopt the total as the baseline, credit `0`;
/// 2. **bridge restart** — the total went backwards (`total < last`): re-baseline, credit `0`;
/// 3. **normal** — credit `total - last`, then advance the baseline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TokenLatch {
    synced: bool,
    last: u32,
}

impl TokenLatch {
    /// A fresh, unsynced latch — the first [`observe`](TokenLatch::observe) adopts the total.
    pub const fn new() -> Self {
        TokenLatch {
            synced: false,
            last: 0,
        }
    }

    /// Observe a new desktop token total; return the tokens to credit this observation
    /// (`0` for the boot and bridge-restart cases).
    pub fn observe(&mut self, total: u32) -> u32 {
        if !self.synced {
            // Case 1 — boot: adopt the total, credit nothing.
            self.last = total;
            self.synced = true;
            return 0;
        }
        if total < self.last {
            // Case 2 — bridge restart: the total went backwards, re-baseline, credit nothing.
            self.last = total;
            return 0;
        }
        // Case 3 — normal: credit the delta and advance the baseline.
        let delta: u32 = total - self.last;
        self.last = total;
        delta
    }
}

impl Default for TokenLatch {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// An empty ring reads a zero median.
    #[test]
    fn an_empty_ring_has_a_zero_median() {
        let ring: VelocityRing = VelocityRing::new();
        assert_eq!(ring.median(), 0);
    }

    /// One sample is its own median.
    #[test]
    fn a_single_sample_is_its_own_median() {
        let mut ring: VelocityRing = VelocityRing::new();
        ring.push(42);
        assert_eq!(ring.median(), 42);
    }

    /// The upper median: for an even count the higher middle element wins.
    #[test]
    fn an_even_count_takes_the_upper_median() {
        let mut ring: VelocityRing = VelocityRing::new();
        ring.push(10);
        ring.push(20);
        // count == 2, sorted [10, 20], index 2/2 == 1 → 20 (upper).
        assert_eq!(ring.median(), 20);
    }

    /// The ring overwrites the oldest once full (holds only the last eight).
    #[test]
    fn the_ring_holds_only_the_last_eight() {
        let mut ring: VelocityRing = VelocityRing::new();
        push_all(&mut ring, &[1, 1, 1, 1, 1, 1, 1, 1, 1000]);
        // Nine pushes: the first 1 is overwritten by 1000. Sorted holds seven 1s and one 1000;
        // index 8/2 == 4 → 1.
        assert_eq!(ring.median(), 1);
    }

    /// Zero velocity is the neutral tier 2, not the fast tier 4.
    #[test]
    fn zero_velocity_is_the_neutral_tier() {
        assert_eq!(mood_tier(0, 0, 0), 2);
    }

    /// A fast median with no penalty is the top tier.
    #[test]
    fn a_fast_median_is_the_top_tier() {
        assert_eq!(mood_tier(10, 0, 0), 4);
    }

    /// A deny-heavy record penalises the tier, clamped at zero.
    #[test]
    fn a_deny_heavy_record_penalises_and_clamps() {
        // vel 10 → 4; denials > approvals → −2 → 2.
        assert_eq!(mood_tier(10, 1, 2), 2);
        // A slow median already at 0 cannot go negative.
        assert_eq!(mood_tier(200, 0, 3), 0);
    }

    /// The deny penalty needs at least three decisions to engage.
    #[test]
    fn the_deny_penalty_needs_three_decisions() {
        // Only two decisions: no penalty even though denials > approvals.
        assert_eq!(mood_tier(10, 0, 2), 4);
    }

    /// Energy decays one step per two hours, integer-divided.
    #[test]
    fn energy_decays_one_step_per_two_hours() {
        assert_eq!(energy_tier(5, 0), 5);
        assert_eq!(energy_tier(5, 1), 5); // 1/2 == 0
        assert_eq!(energy_tier(5, 2), 4);
        assert_eq!(energy_tier(5, 4), 3);
        assert_eq!(energy_tier(5, 100), 0); // clamped low
    }

    /// Hours-since is a saturating millisecond division; a future nap-end reads zero.
    #[test]
    fn hours_since_is_a_saturating_division() {
        assert_eq!(hours_since(0, 3_600_000), 1);
        assert_eq!(hours_since(3_600_000, 0), 0); // now before nap_end saturates
    }

    /// Level and fed progress split a token total; fed never reaches ten.
    #[test]
    fn level_and_fed_split_the_token_total() {
        assert_eq!(level(0), 0);
        assert_eq!(level(50_000), 1);
        assert_eq!(fed_progress(0), 0);
        assert_eq!(fed_progress(49_999), 9); // never 10
    }

    /// Token latch case 1 — boot: the first observation credits nothing and adopts the total.
    #[test]
    fn the_first_observation_credits_nothing() {
        let mut latch: TokenLatch = TokenLatch::new();
        assert_eq!(latch.observe(9_000), 0);
    }

    /// Token latch case 3 — normal: subsequent observations credit the delta.
    #[test]
    fn a_normal_observation_credits_the_delta() {
        let mut latch: TokenLatch = TokenLatch::new();
        latch.observe(1_000);
        assert_eq!(latch.observe(1_500), 500);
    }

    /// Token latch case 2 — bridge restart: a backwards total credits nothing and re-baselines.
    #[test]
    fn a_backwards_total_credits_nothing_and_rebaselines() {
        let mut latch: TokenLatch = TokenLatch::new();
        latch.observe(10_000);
        assert_eq!(latch.observe(3_000), 0); // restart: no credit
        assert_eq!(latch.observe(3_200), 200); // resumes from the new baseline
    }

    // Loop-free fixture: a copied fold over the slice, no `for` keyword, so callers stay
    // cyclomatic-complexity 1.
    fn push_all(ring: &mut VelocityRing, values: &[u16]) {
        values.iter().copied().for_each(|value: u16| {
            ring.push(value);
        });
    }

    proptest! {
        /// The median is always one of the pushed samples and lies within their range.
        #[test]
        fn the_median_is_within_the_pushed_range(values in proptest::collection::vec(0u16..1000, 1..8)) {
            let mut ring: VelocityRing = VelocityRing::new();
            push_all(&mut ring, &values);
            let lo: u16 = *values.iter().min().unwrap();
            let hi: u16 = *values.iter().max().unwrap();
            let median: u16 = ring.median();
            prop_assert!(median >= lo && median <= hi);
        }

        /// The mood tier never exceeds 4 and never underflows below 0.
        #[test]
        fn the_mood_tier_is_bounded(vel in 0u16..500, approvals in 0u32..50, denials in 0u32..50) {
            let tier: u8 = mood_tier(vel, approvals, denials);
            prop_assert!(tier <= 4);
        }

        /// Across a fresh latch, the total credited equals the running total (no double-credit).
        #[test]
        fn the_latch_credits_the_running_total_once(a in 0u32..100_000, b in 0u32..100_000) {
            let (first, second): (u32, u32) = if a <= b { (a, b) } else { (b, a) };
            let mut latch: TokenLatch = TokenLatch::new();
            latch.observe(first); // boot: credit 0, baseline first
            let credited: u32 = latch.observe(second);
            prop_assert_eq!(credited, second - first);
        }
    }
}
