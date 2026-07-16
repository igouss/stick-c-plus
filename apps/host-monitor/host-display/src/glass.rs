//! `Glass` — the host monitor's state as an [`Animated`] view the render loop can drive.

use host_core::HostState;
use platform_core::{Animated, Tick};

use crate::scene::{self, LoadBand};

/// The host monitor's [`HostState`] as a view the board-generic render loop can show.
///
/// The loop (`platform_runtime::spawn_display`) is generic over any [`Animated`] state;
/// this newtype is how a `HostState` becomes one. Its [`anchor`](Animated::anchor) is
/// the coarse [`LoadBand`] — deliberately **not** the whole state — so the creature's
/// animation clock resets only when the host crosses a load threshold or changes status,
/// while the graph still repaints on every new sample (the loop compares the whole
/// `Glass`, history included, so any sample change is a repaint). This is the pomodoro
/// pattern — anchor on `(phase, status)`, not on the second-by-second value — applied to
/// a scrolling graph.
///
/// The wrapper lives here, in the display crate, rather than on `HostState` in
/// `host-core`: it keeps `host-core` free of any dependency on `platform_core`, and the
/// orphan rule is satisfied because `Glass` is local.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Glass(pub HostState);

impl Animated for Glass {
    type Anchor = LoadBand;

    fn anchor(&self) -> LoadBand {
        scene::band(self.0.status)
    }

    fn is_animated(&self) -> bool {
        scene::is_animated(self.anchor())
    }

    fn frame_index(&self, elapsed_ms: Tick) -> usize {
        scene::frame_index(self.anchor(), elapsed_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_core::{History, HostFault, Percent, Sample, Status};

    fn history_of(loads: &[u8]) -> History {
        let mut history: History = History::new();
        for &load in loads {
            history.push(Sample::new(
                Percent::new(load).expect("0..=100"),
                Percent::ZERO,
            ));
        }
        history
    }

    fn glass(status: Status, loads: &[u8]) -> Glass {
        Glass(HostState::new(history_of(loads), status))
    }

    /// The anchor tracks the load band, not the individual samples: two states in the
    /// same band share an anchor even with different histories, so the creature keeps
    /// its clock while the graph scrolls.
    #[test]
    fn the_anchor_is_the_load_band_not_the_history() {
        let calm_a: Glass = glass(
            Status::Fresh(Sample::new(Percent::new(10).unwrap(), Percent::ZERO)),
            &[10, 20],
        );
        let calm_b: Glass = glass(
            Status::Fresh(Sample::new(Percent::new(10).unwrap(), Percent::ZERO)),
            &[30, 40, 10],
        );
        assert_eq!(calm_a.anchor(), calm_b.anchor(), "same band, same anchor");
        assert_eq!(calm_a.anchor(), LoadBand::Calm);
    }

    /// Crossing a load threshold changes the anchor, which restarts the creature.
    #[test]
    fn crossing_a_threshold_changes_the_anchor() {
        let calm: Glass = glass(
            Status::Fresh(Sample::new(Percent::new(10).unwrap(), Percent::ZERO)),
            &[],
        );
        let pegged: Glass = glass(
            Status::Fresh(Sample::new(Percent::new(95).unwrap(), Percent::ZERO)),
            &[],
        );
        assert_ne!(calm.anchor(), pegged.anchor());
    }

    /// Two states that differ only in their history are *not equal* — so the render loop
    /// repaints on a new sample even within one band.
    #[test]
    fn a_new_sample_makes_the_state_unequal_even_within_a_band() {
        let a: Glass = glass(
            Status::Fresh(Sample::new(Percent::new(10).unwrap(), Percent::ZERO)),
            &[10, 20],
        );
        let b: Glass = glass(
            Status::Fresh(Sample::new(Percent::new(10).unwrap(), Percent::ZERO)),
            &[10, 20, 15],
        );
        assert_eq!(a.anchor(), b.anchor(), "still the same band");
        assert_ne!(a, b, "but a new sample must trigger a repaint");
    }

    /// A calm host is still; a fault animates.
    #[test]
    fn calm_is_still_and_a_fault_animates() {
        assert!(!glass(
            Status::Fresh(Sample::new(Percent::ZERO, Percent::ZERO)),
            &[]
        )
        .is_animated());
        assert!(glass(Status::Faulted(HostFault::Unreachable), &[]).is_animated());
    }
}
