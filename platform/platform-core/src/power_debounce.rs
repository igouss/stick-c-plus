//! The pure debounce that turns a chattering VBUS level into a settled one.

use crate::tick::Tick;

/// How long a raw VBUS level must hold steady before [`PowerDebounce`] accepts it, in
/// milliseconds.
///
/// A plug/unplug can chatter the VBUS status bit for tens of milliseconds; this window sits
/// comfortably past that chatter so a bounce train collapses to a single settle, yet stays
/// short enough that a real plug/unplug is heard promptly.
pub const POWER_DEBOUNCE_MS: Tick = 50;

/// Turns a raw, potentially chattering VBUS level into a settled one, as a pure function of
/// time.
///
/// Mirrors the button debounce's stability window (`platform_input::Debounce`), simplified to
/// a bare level — a
/// power source has no tap/hold gesture policy to layer on top. A level is accepted only
/// once it has held steady for [`POWER_DEBOUNCE_MS`], so a bounce train settles once.
///
/// Constructed already-settled at the caller's own first real sample ([`PowerDebounce::new`]),
/// so debounce state never invents a spurious transition at boot — a watch loop treats that
/// first sample as the baseline, not as something [`update`](PowerDebounce::update) need
/// settle towards.
#[derive(Clone, Copy, Debug)]
pub struct PowerDebounce {
    /// The accepted, stable level.
    settled: bool,
    /// The most recent raw level seen — the candidate for the next accepted level.
    candidate: bool,
    /// When `candidate` was first seen: the start of its stability window.
    since: Tick,
}

impl PowerDebounce {
    /// A debounce already settled at `initial` — the caller's own first raw sample — so it
    /// never treats wherever the board started as a transition.
    pub fn new(initial: bool) -> Self {
        PowerDebounce {
            settled: initial,
            candidate: initial,
            since: 0,
        }
    }

    /// Fold one poll `(now, raw_on_usb)` into the debounce, returning the newly settled
    /// level only on the poll where it changes.
    pub fn update(&mut self, now: Tick, raw_on_usb: bool) -> Option<bool> {
        // Track the candidate level and (re)start its stability window on any change.
        if raw_on_usb != self.candidate {
            self.candidate = raw_on_usb;
            self.since = now;
        }

        // Accept the candidate as the new settled level once it has held steady past the
        // window — so a bounce train, which keeps resetting `since`, only ever settles once.
        if self.candidate != self.settled && now.saturating_sub(self.since) >= POWER_DEBOUNCE_MS {
            self.settled = self.candidate;
            return Some(self.settled);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed a poll sequence and collect the settle events it produced, in order.
    fn feed(db: &mut PowerDebounce, steps: &[(Tick, bool)]) -> Vec<bool> {
        steps
            .iter()
            .filter_map(|&(now, raw): &(Tick, bool)| db.update(now, raw))
            .collect()
    }

    /// Zero — a steady reading, however long it is polled, settles nothing more: it is
    /// already the baseline.
    #[test]
    fn a_steady_reading_emits_nothing_after_the_baseline() {
        let mut db: PowerDebounce = PowerDebounce::new(false);
        let events: Vec<bool> = feed(&mut db, &[(0, false), (1_000, false), (5_000, false)]);
        assert_eq!(events, vec![]);
    }

    /// One — a clean level change, held past the window, settles exactly once.
    #[test]
    fn a_clean_change_settles_once() {
        let mut db: PowerDebounce = PowerDebounce::new(false);
        let events: Vec<bool> = feed(
            &mut db,
            &[(0, false), (10, true), (10 + POWER_DEBOUNCE_MS, true)],
        );
        assert_eq!(events, vec![true]);
    }

    /// Many (R4/R14) — a plug/unplug bounce inside the window collapses to one settle: the
    /// chatter never counts twice.
    #[test]
    fn a_bounce_inside_the_window_collapses_to_one_settle() {
        let mut db: PowerDebounce = PowerDebounce::new(false);
        let events: Vec<bool> = feed(
            &mut db,
            &[
                (0, false),
                (2, true),
                (4, false),
                (6, true),                     // chatter, all inside the window
                (6 + POWER_DEBOUNCE_MS, true), // held steady from here -> settles once
            ],
        );
        assert_eq!(events, vec![true]);
    }

    /// Many — two separate, well-spaced level changes settle twice, once each: the window
    /// only suppresses chatter, it never eats a real second transition.
    #[test]
    fn two_separate_changes_settle_twice() {
        let mut db: PowerDebounce = PowerDebounce::new(false);
        let events: Vec<bool> = feed(
            &mut db,
            &[
                (0, false),
                (10, true),
                (10 + POWER_DEBOUNCE_MS, true), // settles true
                (500, false),
                (500 + POWER_DEBOUNCE_MS, false), // settles false
            ],
        );
        assert_eq!(events, vec![true, false]);
    }
}
