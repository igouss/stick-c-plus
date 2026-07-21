//! `Orientation` — everything the board's pose is, read off one acceleration vector.

use platform_core::Acceleration;

use crate::attitude::{attitude_of, Attitude};
use crate::facing::{facing_of, Facing};

/// The board's orientation at one instant: the vector it was read from, the angles it
/// implies, and the pose it is nameable as.
///
/// Carries the raw [`Acceleration`] alongside the conclusions rather than discarding it, for
/// two reasons. The screen draws the per-axis bars straight from it — that *is* the x/y/z
/// graph — and keeping the evidence beside the verdict means a face label that looks wrong
/// can be checked against the numbers that produced it, on the glass, without a debugger.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Orientation {
    /// The acceleration this orientation was read from, in milli-g.
    pub acceleration: Acceleration,
    /// How steeply the board is tilted.
    pub attitude: Attitude,
    /// Which face it is resting on, if any.
    pub facing: Facing,
}

impl Orientation {
    /// The orientation `acceleration` implies.
    ///
    /// A total function of one vector: the same reading always yields the same orientation,
    /// with no clock, no history and no hidden state. Whatever averaging the sampler chose to
    /// do happened before this — which is what lets the whole readout be tested by handing it
    /// numbers.
    pub fn of(acceleration: Acceleration) -> Self {
        Orientation {
            acceleration,
            attitude: attitude_of(acceleration),
            facing: facing_of(acceleration),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::ONE_G_MG;

    /// Zero: the default orientation names no pose, rather than claiming the board is flat.
    #[test]
    fn a_default_orientation_names_no_pose() {
        assert_eq!(Orientation::default().facing, Facing::Moving);
    }

    /// One: the pose the board sits in on the desk, read end to end — the vector, the angles
    /// and the name all agree.
    #[test]
    fn a_board_on_its_back_reads_level_and_screen_up() {
        let flat: Acceleration = Acceleration::new(0, 0, ONE_G_MG);
        let orientation: Orientation = Orientation::of(flat);

        assert_eq!(orientation.acceleration, flat);
        assert_eq!(orientation.attitude, Attitude::default());
        assert_eq!(orientation.facing, Facing::ScreenUp);
    }

    /// Many: turning the board through three poses gives three different orientations —
    /// nothing is stuck, and the transform really is reading the vector.
    #[test]
    fn three_poses_read_as_three_distinct_orientations() {
        let back: Orientation = Orientation::of(Acceleration::new(0, 0, ONE_G_MG));
        let upright: Orientation = Orientation::of(Acceleration::new(ONE_G_MG, 0, 0));
        let edge: Orientation = Orientation::of(Acceleration::new(0, -ONE_G_MG, 0));

        assert_ne!(back, upright);
        assert_ne!(upright, edge);
        assert_ne!(back, edge);
        assert_eq!(back.facing, Facing::ScreenUp);
        assert_eq!(upright.facing, Facing::Upright);
        assert_eq!(edge.facing, Facing::LeftEdge);
    }

    /// The evidence is kept beside the verdict: the raw vector survives the transform
    /// untouched, so the bars the screen draws are the numbers the sensor gave.
    #[test]
    fn the_raw_vector_survives_untouched() {
        let odd: Acceleration = Acceleration::new(-321, 654, -987);
        assert_eq!(Orientation::of(odd).acceleration, odd);
    }
}
