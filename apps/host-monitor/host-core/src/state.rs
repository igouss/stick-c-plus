//! HostState — what a reader observes about the host at one instant.
//!
//! The value the imperative shell hands the display each tick: the rolling
//! [`History`] the two graphs plot, plus the [`Status`] the labels and creature reflect.
//! It is the seam between the shell (which owns the cache) and the display (which draws
//! it) — the analog of the plant monitor's `Observation`, and it lives here in the
//! domain for the same reason: so the shell can produce it and the display can consume
//! it without either depending on the other.
//!
//! ## The graph outlives the reading
//!
//! The history and the status are *independent*. A host that has gone stale or is
//! failing to answer still carries the trailing window of what it was doing — the
//! history is retained — while its status reports the trouble. That is a deliberate
//! divergence from the plant monitor, whose lone scalar must blank when stale because a
//! frozen number is a lie; a frozen *graph of the recent past* is not.

use crate::history::History;
use crate::status::Status;

/// The host monitor's rendered state: the sample history, and the current status.
///
/// `Copy + Eq`, so the display can wrap it in an
/// [`Animated`](platform_core::Animated)-implementing view and the render loop can
/// compare it tick-to-tick for change suppression.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HostState {
    /// The rolling window of recent samples — one column per sample in each graph.
    pub history: History,
    /// What the cache can honestly report right now — chooses the labels and creature.
    pub status: Status,
}

impl HostState {
    /// A state from its history and status.
    pub const fn new(history: History, status: Status) -> Self {
        Self { history, status }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::percent::Percent;
    use crate::sample::Sample;

    #[test]
    fn a_state_carries_its_history_and_status() {
        let mut history: History = History::new();
        history.push(Sample::new(Percent::FULL, Percent::ZERO));
        let status: Status = Status::Fresh(Sample::new(Percent::FULL, Percent::ZERO));

        let state: HostState = HostState::new(history, status);
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.status, status);
    }
}
