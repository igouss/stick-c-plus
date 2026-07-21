//! `OrientationView` — a snapshot of the board's pose as an [`Animated`] view the loop shows.

use orientation_core::{Facing, Orientation};
use platform_core::{Acceleration, Animated, Tick};

/// What the orientation glass shows at one instant.
///
/// A plain restatement of an [`Orientation`], carried by value so the render loop can compare
/// two of them for equality and skip a repaint when the picture has not changed. That
/// comparison is the whole reason the angles are whole degrees and the axes whole milli-g: a
/// board sitting still produces an identical view frame after frame, so a still readout costs
/// no SPI traffic at all, while a board being turned repaints at the full loop cadence.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct OrientationView {
    /// The smoothed acceleration, in milli-g — what the three axis bars draw.
    pub acceleration: Acceleration,
    /// How far the board's long axis is tilted out of level, in degrees.
    pub pitch_deg: i32,
    /// How far the board is rolled onto an edge, in degrees.
    pub roll_deg: i32,
    /// The face it is resting on, if any.
    pub facing: Facing,
}

impl OrientationView {
    /// The view of `orientation`.
    pub const fn of(orientation: &Orientation) -> Self {
        OrientationView {
            acceleration: orientation.acceleration,
            pitch_deg: orientation.attitude.pitch_deg,
            roll_deg: orientation.attitude.roll_deg,
            facing: orientation.facing,
        }
    }

    /// The three axes in the order the screen stacks them, each with its one-letter name.
    ///
    /// One source of truth for "which row is which axis", so the bars, the values and the
    /// labels cannot get out of step with one another.
    pub const fn axes(&self) -> [(&'static str, i32); 3] {
        [
            ("X", self.acceleration.x_mg),
            ("Y", self.acceleration.y_mg),
            ("Z", self.acceleration.z_mg),
        ]
    }
}

impl Animated for OrientationView {
    /// The face. Nothing on this screen animates, but the anchor is still the *coarse* fact
    /// rather than the whole view: it is what the render loop measures a state's age from, and
    /// a live milli-g readout would otherwise reset that age on essentially every frame.
    type Anchor = Facing;

    fn anchor(&self) -> Facing {
        self.facing
    }

    /// Never. This screen is an instrument, not a scene — it carries no creature, and every
    /// pixel it paints is a consequence of a sensor reading rather than of the clock. Its
    /// liveliness comes from the data changing, which the render loop already repaints for.
    fn is_animated(&self) -> bool {
        false
    }

    /// Always the first frame: there is no sprite here to advance.
    fn frame_index(&self, _elapsed_ms: Tick) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::ONE_G_MG;

    fn view_of(x_mg: i32, y_mg: i32, z_mg: i32) -> OrientationView {
        OrientationView::of(&Orientation::of(Acceleration::new(x_mg, y_mg, z_mg)))
    }

    /// Zero: a default view names no pose and shows a level, weightless board.
    #[test]
    fn a_default_view_shows_nothing_and_names_no_pose() {
        let blank: OrientationView = OrientationView::default();
        assert_eq!(blank.acceleration, Acceleration::default());
        assert_eq!(blank.facing, Facing::Moving);
    }

    /// One: the view carries the orientation's own conclusions, unchanged.
    #[test]
    fn a_view_restates_its_orientation() {
        let flat: OrientationView = view_of(0, 0, ONE_G_MG);
        assert_eq!(flat.facing, Facing::ScreenUp);
        assert_eq!(flat.pitch_deg, 0);
        assert_eq!(flat.roll_deg, 0);
        assert_eq!(flat.acceleration.z_mg, ONE_G_MG);
    }

    /// Many: the three axes come out in the screen's stacking order, each with its value.
    #[test]
    fn the_axes_are_listed_in_the_screens_order() {
        assert_eq!(
            view_of(11, 22, 33).axes(),
            [("X", 11), ("Y", 22), ("Z", 33)]
        );
    }

    /// The change-suppression contract the render loop depends on: an unchanged board is an
    /// equal view, so a still readout repaints nothing.
    #[test]
    fn an_unchanged_reading_is_an_equal_view() {
        assert_eq!(view_of(0, 0, ONE_G_MG), view_of(0, 0, ONE_G_MG));
    }

    /// ...and a board that moved is not, so the readout is never stale. A single milli-g on
    /// one axis is enough to repaint — this is what makes the readout responsive.
    #[test]
    fn a_single_milli_g_of_movement_is_a_different_view() {
        assert_ne!(view_of(0, 0, ONE_G_MG), view_of(1, 0, ONE_G_MG));
    }

    /// The anchor is the coarse face, not the live reading. Anchoring on the whole view would
    /// reset the loop's state-age clock on almost every frame, which is exactly what the
    /// `Animated` contract's anchor exists to avoid.
    #[test]
    fn the_anchor_ignores_a_reading_that_did_not_change_the_face() {
        let level: OrientationView = view_of(0, 0, ONE_G_MG);
        let nudged: OrientationView = view_of(60, 0, 990);
        assert_ne!(level, nudged, "the two readings are different views");
        assert_eq!(
            level.anchor(),
            nudged.anchor(),
            "a nudge that did not change the face must not reset the state clock"
        );
    }

    /// Nothing on this screen animates, at any elapsed time.
    #[test]
    fn the_readout_never_animates() {
        let view: OrientationView = view_of(0, 0, ONE_G_MG);
        assert!(!view.is_animated());
        assert_eq!(view.frame_index(0), 0);
        assert_eq!(view.frame_index(Tick::MAX), 0);
    }
}
