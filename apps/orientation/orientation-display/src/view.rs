//! `OrientationView` — a snapshot of the board's pose as an [`Animated`] view the loop shows.

use orientation_core::{Facing, Reading, Signal};
use platform_core::{Acceleration, Animated, ScreenRotation, Tick};

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
    /// Whether these numbers are still being confirmed by a sensor that answers.
    pub signal: Signal,
    /// Which way up the picture is drawn, and therefore which layout it is drawn through.
    ///
    /// Part of the view rather than a separate argument to the renderer, because the render
    /// loop repaints on a *changed view*: a rotation that reached the panel any other way
    /// would turn the glass and leave the previous quadrant's pixels on it until the sensor
    /// happened to move.
    pub rotation: ScreenRotation,
}

impl OrientationView {
    /// The view of `reading`, drawn at the panel's native landscape.
    pub const fn of(reading: &Reading) -> Self {
        OrientationView {
            acceleration: reading.orientation.acceleration,
            pitch_deg: reading.orientation.attitude.pitch_deg,
            roll_deg: reading.orientation.attitude.roll_deg,
            facing: reading.orientation.facing,
            signal: reading.signal,
            rotation: ScreenRotation::Deg0,
        }
    }

    /// The same view, drawn at `rotation`.
    pub const fn rotated(self, rotation: ScreenRotation) -> Self {
        OrientationView { rotation, ..self }
    }

    /// What the label field says: the face name, or that the readout has stopped being fed.
    ///
    /// A lost signal takes the label rather than sitting beside it, because the face name is
    /// exactly the thing that has stopped being true — leaving `SCREEN UP` on the glass with a
    /// warning next to it asks the reader to notice a modifier before believing a word they
    /// have already read. Nine characters, inside the ten the field reserves.
    pub const fn label(&self) -> &'static str {
        match self.signal {
            Signal::Live => self.facing.label(),
            Signal::Lost => "NO SIGNAL",
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
    /// The face, the signal and the rotation. Nothing on this screen animates, but the anchor
    /// is still the *coarse* fact rather than the whole view: it is what the render loop
    /// measures a state's age from, and a live milli-g readout would otherwise reset that age
    /// on essentially every frame. Losing or regaining the signal is a genuine change of state
    /// — the label changes word and colour — and so is turning the picture, which redraws
    /// every pixel through a different layout. Both belong in the anchor beside the face.
    type Anchor = (Facing, Signal, ScreenRotation);

    fn anchor(&self) -> (Facing, Signal, ScreenRotation) {
        (self.facing, self.signal, self.rotation)
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
    use orientation_core::Orientation;
    use platform_core::ONE_G_MG;

    /// A live view of the board reading these axes.
    fn view_of(x_mg: i32, y_mg: i32, z_mg: i32) -> OrientationView {
        OrientationView::of(&Reading::aged(
            Orientation::of(Acceleration::new(x_mg, y_mg, z_mg)),
            0,
        ))
    }

    /// The same board, but nothing has refreshed the reading for long enough to say so.
    fn stale_view_of(x_mg: i32, y_mg: i32, z_mg: i32) -> OrientationView {
        OrientationView::of(&Reading::aged(
            Orientation::of(Acceleration::new(x_mg, y_mg, z_mg)),
            orientation_core::SIGNAL_TIMEOUT_MS,
        ))
    }

    /// Zero: a default view names no pose, shows a level weightless board, and vouches for
    /// nothing — a view nothing was read into must not claim a live sensor.
    #[test]
    fn a_default_view_shows_nothing_and_names_no_pose() {
        let blank: OrientationView = OrientationView::default();
        assert_eq!(blank.acceleration, Acceleration::default());
        assert_eq!(blank.facing, Facing::Moving);
        assert_eq!(blank.signal, Signal::Lost);
        assert_eq!(blank.rotation, ScreenRotation::Deg0);
    }

    /// A view reads at the panel's native landscape until something says otherwise, and says
    /// otherwise without disturbing a single number.
    #[test]
    fn turning_the_picture_changes_nothing_but_the_rotation() {
        let flat: OrientationView = view_of(0, 0, ONE_G_MG);
        let turned: OrientationView = flat.rotated(ScreenRotation::Deg90);

        assert_eq!(flat.rotation, ScreenRotation::Deg0);
        assert_eq!(turned.rotation, ScreenRotation::Deg90);
        assert_eq!(turned.acceleration, flat.acceleration);
        assert_eq!(turned.facing, flat.facing);
        assert_eq!(turned.label(), flat.label());
    }

    /// Turning the picture is a different view, so the render loop's change suppression
    /// repaints for it. Without this the glass would turn and keep the old quadrant's pixels.
    #[test]
    fn turning_the_picture_is_a_different_view() {
        let flat: OrientationView = view_of(0, 0, ONE_G_MG);
        assert_ne!(flat, flat.rotated(ScreenRotation::Deg90));
        assert_ne!(flat.anchor(), flat.rotated(ScreenRotation::Deg90).anchor());
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

    /// A live view labels itself with the face it read.
    #[test]
    fn a_live_view_is_labelled_with_its_face() {
        assert_eq!(view_of(0, 0, ONE_G_MG).label(), Facing::ScreenUp.label());
    }

    /// A lost signal takes over the label, so the face name cannot be read as current.
    #[test]
    fn a_lost_signal_replaces_the_face_name() {
        let stale: OrientationView = stale_view_of(0, 0, ONE_G_MG);
        assert_eq!(stale.label(), "NO SIGNAL");
        assert_eq!(
            stale.facing,
            Facing::ScreenUp,
            "the last known face is still carried, it is just not what the label says"
        );
    }

    /// The `NO SIGNAL` label fits the same ten-character field every face name does, so it
    /// cannot overflow its line or fail to erase a longer predecessor.
    #[test]
    fn the_no_signal_label_fits_the_screens_field() {
        assert!(stale_view_of(0, ONE_G_MG, 0).label().len() <= 10);
    }

    /// Losing the signal is a different view even when the numbers are identical — otherwise
    /// the render loop's change suppression would keep a stale readout on the glass forever.
    #[test]
    fn losing_the_signal_is_a_different_view() {
        assert_ne!(view_of(0, 0, ONE_G_MG), stale_view_of(0, 0, ONE_G_MG));
    }

    /// ...and it resets the state clock, because the label really did change.
    #[test]
    fn losing_the_signal_moves_the_anchor() {
        assert_ne!(
            view_of(0, 0, ONE_G_MG).anchor(),
            stale_view_of(0, 0, ONE_G_MG).anchor()
        );
    }
}
