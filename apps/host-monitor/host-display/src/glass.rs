//! `Glass` — the host monitor's state as an [`Animated`] view the render loop can drive.

use host_core::HostState;
use platform_core::{Animated, Tick};

/// The host monitor's [`HostState`] as a view the board-generic render loop can show.
///
/// The loop (`platform_runtime::spawn_display`) is generic over any [`Animated`] state; this
/// newtype is how a `HostState` becomes one. This screen is **still**: three hosts fill the
/// glass, leaving no room for an animated creature, so there is nothing to advance frame by
/// frame — [`is_animated`](Animated::is_animated) is always `false` and
/// [`frame_index`](Animated::frame_index) is always `0`. The loop therefore repaints only
/// when the picture changes, which — because `Glass` derives `Eq` over the whole
/// `HostState` — is exactly when a new frame is fetched or the endpoint's status changes,
/// and the device rests between.
///
/// The wrapper lives here, in the display crate, rather than on `HostState` in `host-core`:
/// it keeps `host-core` free of any dependency on `platform_core`, and the orphan rule is
/// satisfied because `Glass` is local.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Glass(pub HostState);

impl Animated for Glass {
    /// The whole state is the identity — there is no separate animation clock to keep, so a
    /// coarse anchor would buy nothing. `()` means the loop only ever compares the state
    /// itself for change.
    type Anchor = ();

    fn anchor(&self) {}

    fn is_animated(&self) -> bool {
        false
    }

    fn frame_index(&self, _elapsed_ms: Tick) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_core::{Pulse, PulseBuilder, Status};

    fn frame(cpu: i32) -> Pulse {
        let mut b: PulseBuilder = PulseBuilder::new(30, 900);
        b.push("fedora", &[Some(cpu)], &[Some(50)]);
        b.build()
    }

    /// A still screen: it never animates and always sits on frame 0.
    #[test]
    fn the_screen_is_always_still() {
        let glass: Glass = Glass(HostState::new(Some(frame(50)), Status::Fresh));
        assert!(!glass.is_animated());
        assert_eq!(glass.frame_index(0), 0);
        assert_eq!(glass.frame_index(u64::MAX), 0);
    }

    /// A new frame makes the state unequal, so the render loop repaints on fresh data.
    #[test]
    fn a_new_frame_makes_the_state_unequal() {
        let a: Glass = Glass(HostState::new(Some(frame(10)), Status::Fresh));
        let b: Glass = Glass(HostState::new(Some(frame(80)), Status::Fresh));
        assert_ne!(a, b, "a new frame must trigger a repaint");
    }

    /// A status change (fresh → faulted) also makes the state unequal, so the marker appears.
    #[test]
    fn a_status_change_makes_the_state_unequal() {
        let fresh: Glass = Glass(HostState::new(Some(frame(10)), Status::Fresh));
        let faulted: Glass = Glass(HostState::new(
            Some(frame(10)),
            Status::Faulted(host_core::HostFault::Unreachable),
        ));
        assert_ne!(fresh, faulted);
    }
}
