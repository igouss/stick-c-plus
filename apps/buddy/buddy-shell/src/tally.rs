//! What the pet has been through: the counts the stats page reads.
//!
//! The arithmetic all lives in `buddy_core::stats`; this holds the *running totals* that
//! arithmetic is applied to, and advances them on the three events that move them — an answered
//! prompt, a nap ending, and a token credit.
//!
//! Time is injected. Nothing here reads a clock; a nap's length is `now - started`, with both
//! handed in.

use buddy_core::{
    energy_tier, fed_progress, hours_since, level, mood_tier, VelocityRing, ENERGY_AT_NAP_FULL,
};
use buddy_display::StatsView;
use platform_core::Tick;

/// Milliseconds in a minute, for the nap total.
const MINUTE_MS: u64 = 60_000;

/// The running totals behind the pet screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tally {
    /// Prompts approved.
    pub approvals: u32,
    /// Prompts denied.
    pub denials: u32,
    /// Naps taken.
    pub naps: u32,
    /// Whole milliseconds spent napping, across all of them.
    pub napped_ms: u64,
    /// Lifetime tokens credited — the level and the fed meter are functions of this.
    pub tokens: u32,
    /// Today's tokens, straight off the wire.
    pub tokens_today: u32,
    /// The energy level at the end of the last nap, which decays from there.
    energy_at_nap: u8,
    /// When the last nap ended — the decay's zero point.
    nap_end: Tick,
    /// When the current nap started, if one is running.
    nap_started: Option<Tick>,
}

impl Tally {
    /// A pet with no history: awake, unfed, never asked anything.
    ///
    /// Energy starts full rather than empty. A buddy that has just booted has not been awake
    /// long enough to be tired, and a fresh pet showing a flat energy bar reads as broken.
    pub const fn new() -> Self {
        Tally {
            approvals: 0,
            denials: 0,
            naps: 0,
            napped_ms: 0,
            tokens: 0,
            tokens_today: 0,
            energy_at_nap: ENERGY_AT_NAP_FULL,
            nap_end: 0,
            nap_started: None,
        }
    }

    /// Record an answered prompt, and the seconds the owner took over it.
    ///
    /// The answer time feeds the velocity ring, which is what the mood tier is derived from —
    /// so a buddy answered promptly is a happy one, and the ring is the memory that makes that
    /// a trend rather than a single reading.
    pub fn answered(&mut self, velocity: &mut VelocityRing, approved: bool, took_s: u32) {
        if approved {
            self.approvals = self.approvals.saturating_add(1);
        } else {
            self.denials = self.denials.saturating_add(1);
        }
        velocity.push(took_s.min(u32::from(u16::MAX)) as u16);
    }

    /// A nap began at `now`.
    pub fn nap_began(&mut self, now: Tick) {
        self.nap_started = Some(now);
    }

    /// A nap ended at `now`: count it, add its length, and refill the energy.
    ///
    /// A nap end with no recorded start adds no time but is still counted — the alternative is
    /// dropping a real nap because the firmware was restarted face-down.
    pub fn nap_ended(&mut self, now: Tick) {
        self.naps = self.naps.saturating_add(1);
        if let Some(started) = self.nap_started.take() {
            self.napped_ms = self.napped_ms.saturating_add(now.saturating_sub(started));
        }
        self.energy_at_nap = ENERGY_AT_NAP_FULL;
        self.nap_end = now;
    }

    /// Credit tokens earned since the last observation.
    pub fn credit(&mut self, tokens: u32) {
        self.tokens = self.tokens.saturating_add(tokens);
    }

    /// Whether crediting `tokens` would cross a level boundary — the level-up milestone that
    /// arms the celebrate one-shot.
    ///
    /// Asked *before* the credit, so the caller can arm the one-shot on the same frame the
    /// level changes rather than a frame later.
    pub fn would_level_up(&self, tokens: u32) -> bool {
        level(self.tokens.saturating_add(tokens)) > level(self.tokens)
    }

    /// The stats page's view of all this, at `now`.
    pub fn view(&self, velocity: &VelocityRing, now: Tick) -> StatsView {
        StatsView {
            mood: mood_tier(velocity.median(), self.approvals, self.denials),
            fed: fed_progress(self.tokens),
            energy: energy_tier(self.energy_at_nap, hours_since(self.nap_end, now)),
            level: level(self.tokens),
            approvals: self.approvals,
            denials: self.denials,
            naps: self.naps,
            nap_minutes: (self.napped_ms / MINUTE_MS).min(u64::from(u32::MAX)) as u32,
            tokens_today: self.tokens_today,
        }
    }
}

impl Default for Tally {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buddy_core::TOKENS_PER_LEVEL;

    /// Zero: a fresh pet has no history — and full energy, because it has not been awake long
    /// enough to be tired.
    #[test]
    fn a_fresh_pet_has_no_history_and_full_energy() {
        let tally: Tally = Tally::new();
        let velocity: VelocityRing = VelocityRing::new();
        let view: StatsView = tally.view(&velocity, 0);
        assert_eq!(view.approvals, 0);
        assert_eq!(view.naps, 0);
        assert_eq!(view.level, 0);
        assert_eq!(view.energy, ENERGY_AT_NAP_FULL);
    }

    /// One: an approval is counted, and a denial is counted separately.
    #[test]
    fn approvals_and_denials_are_counted_apart() {
        let mut tally: Tally = Tally::new();
        let mut velocity: VelocityRing = VelocityRing::new();
        tally.answered(&mut velocity, true, 2);
        tally.answered(&mut velocity, false, 3);
        assert_eq!(tally.approvals, 1);
        assert_eq!(tally.denials, 1);
    }

    /// A nap's length is the span between its ends, in whole minutes.
    #[test]
    fn a_nap_is_counted_and_its_length_added() {
        let mut tally: Tally = Tally::new();
        tally.nap_began(1_000);
        tally.nap_ended(1_000 + 90 * MINUTE_MS);
        let view: StatsView = tally.view(&VelocityRing::new(), 1_000 + 90 * MINUTE_MS);
        assert_eq!(view.naps, 1);
        assert_eq!(view.nap_minutes, 90);
    }

    /// A nap end with no start is still a nap — a firmware restarted face-down must not lose it.
    #[test]
    fn a_nap_end_with_no_start_still_counts() {
        let mut tally: Tally = Tally::new();
        tally.nap_ended(5_000);
        assert_eq!(tally.naps, 1);
        assert_eq!(tally.view(&VelocityRing::new(), 5_000).nap_minutes, 0);
    }

    /// Energy refills on a nap end and decays from there, on the domain's own schedule.
    #[test]
    fn energy_refills_on_a_nap_and_decays_after() {
        let mut tally: Tally = Tally::new();
        tally.nap_ended(0);
        let velocity: VelocityRing = VelocityRing::new();
        assert_eq!(tally.view(&velocity, 0).energy, ENERGY_AT_NAP_FULL);
        let six_hours: Tick = 6 * 60 * 60 * 1_000;
        assert!(tally.view(&velocity, six_hours).energy < ENERGY_AT_NAP_FULL);
    }

    /// Many: tokens accumulate into a level, and the milestone is seen BEFORE the credit so the
    /// celebrate one-shot can be armed on the same frame.
    #[test]
    fn a_credit_that_crosses_a_level_is_seen_before_it_is_taken() {
        let mut tally: Tally = Tally::new();
        tally.credit(TOKENS_PER_LEVEL - 1);
        assert!(!tally.would_level_up(0));
        assert!(tally.would_level_up(1));
        tally.credit(1);
        assert_eq!(tally.view(&VelocityRing::new(), 0).level, 1);
    }

    /// A prompt answered fast is a happier pet than one answered slowly — the velocity ring
    /// really does reach the mood tier.
    #[test]
    fn a_fast_answer_reads_happier_than_a_slow_one() {
        let mut fast_ring: VelocityRing = VelocityRing::new();
        let mut fast: Tally = Tally::new();
        fast.answered(&mut fast_ring, true, 2);

        let mut slow_ring: VelocityRing = VelocityRing::new();
        let mut slow: Tally = Tally::new();
        slow.answered(&mut slow_ring, true, 200);

        assert!(fast.view(&fast_ring, 0).mood > slow.view(&slow_ring, 0).mood);
    }
}
