//! HostState — what a reader observes about the pulse endpoint at one instant.
//!
//! The value the imperative shell hands the display each tick: the latest good
//! [`Pulse`] frame (the rows the display draws), plus the [`Status`] the endpoint
//! marker reflects. It is the seam between the shell (which owns the cache) and the
//! display (which draws it) — the analog of the plant monitor's `Observation` — and it
//! lives here in the domain so the shell can produce it and the display can consume it
//! without either depending on the other.
//!
//! ## The frame outlives the reading
//!
//! The frame and the status are *independent*. An endpoint that has gone stale or is
//! failing to answer still carries the last frame it returned — the data is retained —
//! while its status reports the trouble. A frozen *window of the recent past* is useful,
//! not a lie, so it stays on the glass with a status marker over it; only when nothing has
//! ever been fetched is there no frame to show ([`frame`](HostState::frame) is [`None`]).

use crate::pulse::Pulse;
use crate::status::Status;

/// The host monitor's rendered state: the last good frame (if any), and the current
/// endpoint status.
///
/// `Copy + Eq`, so the display can wrap it in an [`Animated`](platform_core::Animated)
/// view and the render loop can compare it tick-to-tick for change suppression.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HostState {
    /// The last frame the endpoint returned, retained across faults; [`None`] until the
    /// first successful fetch.
    pub frame: Option<Pulse>,
    /// What the cache can honestly report right now — chooses the endpoint marker.
    pub status: Status,
}

impl HostState {
    /// A state from its frame and status.
    pub const fn new(frame: Option<Pulse>, status: Status) -> Self {
        Self { frame, status }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pulse::PulseBuilder;

    #[test]
    fn a_state_carries_its_frame_and_status() {
        let mut b: PulseBuilder = PulseBuilder::new(30, 900);
        b.push("fedora", &[Some(11)], &[Some(41)]);
        let frame: Pulse = b.build();

        let state: HostState = HostState::new(Some(frame), Status::Fresh);
        assert_eq!(state.frame.map(|f: Pulse| f.len()), Some(1));
        assert_eq!(state.status, Status::Fresh);
    }

    #[test]
    fn a_never_sampled_state_has_no_frame() {
        let state: HostState = HostState::new(None, Status::NeverSampled);
        assert!(state.frame.is_none());
    }
}
