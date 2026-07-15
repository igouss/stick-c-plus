//! `Glass` — the plant [`Observation`] as an [`Animated`] view the render loop can drive.

use plant_core::Observation;
use platform_core::{Animated, Tick};

use crate::scene;

/// The plant monitor's [`Observation`] as a view the board-generic render loop can show.
///
/// The loop (`platform_runtime::spawn_display`) is generic over any [`Animated`] state; this
/// newtype is how a plant `Observation` becomes one, delegating the animation policy to
/// [`crate::scene`]. Its [`anchor`](Animated::anchor) is the whole observation, so the
/// creature restarts only on a real state change — which reproduces the plant monitor's
/// original behaviour exactly, since an `Observation` carries no per-tick value that would
/// otherwise reset the clock.
///
/// The wrapper lives here, in the plant display crate, rather than on `Observation` in
/// `plant-core`: it keeps `plant-core` zero-dependency (it never learns what `Animated` is),
/// and the orphan rule is satisfied because `Glass` is local.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Glass(pub Observation);

impl Animated for Glass {
    type Anchor = Observation;

    fn anchor(&self) -> Observation {
        self.0
    }

    fn is_animated(&self) -> bool {
        scene::is_animated(self.0)
    }

    fn frame_index(&self, elapsed_ms: Tick) -> usize {
        scene::frame_index(self.0, elapsed_ms)
    }
}
