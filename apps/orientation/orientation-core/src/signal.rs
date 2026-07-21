//! Whether the readout is still being fed — the fact that stands behind every other one.
//!
//! [`Facing`](crate::Facing) answers "which way is the board resting?" from the gravity
//! vector. That answer is only worth as much as the vector's freshness, and freshness is not
//! something the vector can report about itself: a sensor that stopped answering leaves the
//! last good reading sitting there, perfectly well-formed and quietly false. So liveness is a
//! *separate* axis from pose, decided by a clock rather than by arithmetic on the reading.

use platform_core::Tick;

use crate::orientation::Orientation;

/// How long the readout may go unfed before it stops claiming to be current.
///
/// The sampler polls every 10 ms, so this is fifty consecutive missed reads. That is far more
/// than the flaky single I2C transaction the skip-and-carry-on policy exists for — a glitch
/// costs one or two cycles — and far less than a person staring at a frozen screen will take
/// to be misled by it. Half a second is also below the threshold where a human reads a delay
/// as a fault in the readout rather than in the sensor.
pub const SIGNAL_TIMEOUT_MS: Tick = 500;

/// Whether the pose on the glass is still being refreshed by a sensor that answers.
///
/// Deliberately not a [`Facing`](crate::Facing) variant. `Facing` is a total function of one
/// acceleration vector, and "the sensor went away" is not a thing any vector says — a
/// `Facing::NoSignal` would be a variant `facing_of` could never return, which is a seam in
/// the wrong place. Keeping the two orthogonal also lets the glass say what it actually
/// knows: the *last* pose was `SCREEN UP`, and it is no longer being confirmed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signal {
    /// A reading arrived recently enough that the pose is still being confirmed.
    Live,
    /// Nothing has arrived for [`SIGNAL_TIMEOUT_MS`]. Whatever is on the glass is a memory.
    Lost,
}

impl Default for Signal {
    /// Lost. A readout nothing has been published into has never been confirmed by anything,
    /// and claiming otherwise is precisely the lie this type exists to prevent.
    fn default() -> Self {
        Signal::Lost
    }
}

impl Signal {
    /// The verdict on a reading published `age_ms` ago.
    ///
    /// A board that has never published is not a special case: its first publication is
    /// pending, its age grows from boot like any other, and it goes [`Signal::Lost`] on the
    /// same clock as a sensor that died mid-run. One rule, no exceptions to keep in step.
    pub const fn after(age_ms: Tick) -> Signal {
        if age_ms < SIGNAL_TIMEOUT_MS {
            Signal::Live
        } else {
            Signal::Lost
        }
    }

    /// Whether the reading behind this signal is still being confirmed.
    pub const fn is_live(self) -> bool {
        matches!(self, Signal::Live)
    }
}

/// A pose, and whether it is still being confirmed.
///
/// What the glass is handed: never an [`Orientation`] on its own, because an orientation on
/// its own cannot be drawn honestly — the picture would be identical whether the sensor was
/// answering or had been unplugged an hour ago. Pairing the two at the type level means the
/// question "is this still true?" has to be answered before anything can be painted.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Reading {
    /// The most recently published pose.
    pub orientation: Orientation,
    /// Whether that pose is still being refreshed.
    pub signal: Signal,
}

impl Reading {
    /// The reading `orientation` makes, published `age_ms` ago.
    pub const fn aged(orientation: Orientation, age_ms: Tick) -> Self {
        Reading {
            orientation,
            signal: Signal::after(age_ms),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::{Acceleration, ONE_G_MG};

    /// Zero: a reading published this instant is live.
    #[test]
    fn a_fresh_reading_is_live() {
        assert_eq!(Signal::after(0), Signal::Live);
    }

    /// One: one millisecond short of the timeout is still live — the boundary belongs to
    /// the live side, so a sampler landing exactly on budget is never called dead.
    #[test]
    fn the_last_millisecond_before_the_timeout_is_still_live() {
        assert_eq!(Signal::after(SIGNAL_TIMEOUT_MS - 1), Signal::Live);
    }

    /// Many: at the timeout and every age beyond it, the signal is lost and stays lost.
    #[test]
    fn the_timeout_and_everything_past_it_is_lost() {
        assert_eq!(Signal::after(SIGNAL_TIMEOUT_MS), Signal::Lost);
        assert_eq!(Signal::after(SIGNAL_TIMEOUT_MS + 1), Signal::Lost);
        assert_eq!(Signal::after(60_000), Signal::Lost);
        assert_eq!(Signal::after(Tick::MAX), Signal::Lost);
    }

    /// A signal nothing has been published into reports itself lost, rather than defaulting
    /// to the flattering answer.
    #[test]
    fn a_default_signal_is_lost() {
        assert_eq!(Signal::default(), Signal::Lost);
        assert!(!Signal::default().is_live());
    }

    /// `is_live` agrees with the variant it reports on — the predicate the glass branches on
    /// cannot drift from the enum it reads.
    #[test]
    fn is_live_agrees_with_the_variant() {
        assert!(Signal::Live.is_live());
        assert!(!Signal::Lost.is_live());
    }

    /// A reading keeps its pose whichever way the signal went — losing the sensor does not
    /// erase what it last said, it only stops vouching for it.
    #[test]
    fn a_reading_keeps_its_pose_on_both_sides_of_the_timeout() {
        let flat: Orientation = Orientation::of(Acceleration::new(0, 0, ONE_G_MG));

        let live: Reading = Reading::aged(flat, 0);
        assert_eq!(live.signal, Signal::Live);
        assert_eq!(live.orientation, flat);

        let stale: Reading = Reading::aged(flat, SIGNAL_TIMEOUT_MS);
        assert_eq!(stale.signal, Signal::Lost);
        assert_eq!(
            stale.orientation, flat,
            "a lost signal must not discard the last known pose"
        );
    }

    /// A default reading names no pose and vouches for nothing.
    #[test]
    fn a_default_reading_is_lost() {
        assert_eq!(Reading::default().signal, Signal::Lost);
        assert_eq!(Reading::default().orientation, Orientation::default());
    }
}
