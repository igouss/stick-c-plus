//! Which way is up *on the glass* — so the readout can be read whichever way the board is held.
//!
//! Of the four quarter turns the picture could be drawn at, which one puts its top toward the
//! sky. A narrow question, and a different one from which face the board is resting on: a
//! board lying flat rests on a perfectly nameable face and has no readable rotation at all.
//! Naming the face is an app's business (`orientation-core` does it); the quarter turn is the
//! *board's*, because every app's glass is the same glass and turns the same way.
//!
//! Lives here rather than in an app so that all four apps can be drawn the right way up
//! without depending on one another — which the context boundary would refuse anyway.

use crate::imu::is_at_rest;
use crate::{Acceleration, Tick};

/// How much gravity must lie *in the screen's plane* before a quadrant can be read from it.
///
/// The screen's plane is spanned by the board's X and Y axes, so a board lying flat puts all
/// of its gravity on Z and leaves nothing here to point with. `400 mg` is about 24° off flat:
/// past the angle where the in-plane direction is real rather than noise, and well short of
/// the tilt at which someone would say they are holding the thing up to read it.
pub const IN_PLANE_MINIMUM_MG: i32 = 400;

/// How decisively one in-plane axis must beat the other before the picture turns.
///
/// The quadrant boundaries sit on the diagonals, where the two axes are equal. Without a dead
/// band a board resting near 45° would flip between two rotations on sensor noise alone — a
/// full repaint each time, and unreadable exactly when someone is looking at it. `200 mg` of
/// margin means the board has to be committed to a quadrant, and inside the band the picture
/// keeps whatever rotation it already had.
pub const QUADRANT_MARGIN_MG: i32 = 200;

/// A quarter turn of the picture on the glass.
///
/// Four rotations, because a screen has four sides. The names are the rotation applied to the
/// image, clockwise, from the panel's own native landscape.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ScreenRotation {
    /// The panel's native landscape — the picture as it has always been drawn.
    #[default]
    Deg0,
    /// A quarter turn clockwise. Portrait.
    Deg90,
    /// Upside down. Landscape.
    Deg180,
    /// Three quarter turns clockwise. Portrait.
    Deg270,
}

impl ScreenRotation {
    /// Whether this rotation puts the canvas in portrait — taller than it is wide.
    ///
    /// The one thing the rest of the app needs to branch on: a portrait canvas is 135 px wide
    /// where a landscape one is 240, which is thirteen characters instead of twenty-four, and
    /// no single layout serves both.
    pub const fn is_portrait(self) -> bool {
        matches!(self, ScreenRotation::Deg90 | ScreenRotation::Deg270)
    }

    /// The rotation half a turn from this one.
    pub const fn opposite(self) -> ScreenRotation {
        match self {
            ScreenRotation::Deg0 => ScreenRotation::Deg180,
            ScreenRotation::Deg90 => ScreenRotation::Deg270,
            ScreenRotation::Deg180 => ScreenRotation::Deg0,
            ScreenRotation::Deg270 => ScreenRotation::Deg90,
        }
    }
}

/// Which board axis is pointing at the sky, when one of the in-plane pair clearly is.
///
/// The intermediate answer between a vector and a rotation, named so the hardware-dependent
/// half of the decision is one small table rather than four arms of arithmetic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum UpAxis {
    /// `+X` — the top of the stick, away from the USB-C port.
    StickTop,
    /// `−X` — the USB-C end.
    UsbEnd,
    /// `+Y` — the left edge.
    LeftEdge,
    /// `−Y` — the right edge.
    RightEdge,
}

impl UpAxis {
    /// The rotation that puts this axis at the top of the picture.
    ///
    /// **This table is a fact about the hardware**, not about geometry: the panel is a 135×240
    /// portrait part mounted rotated, so which board edge its native landscape image calls
    /// "up" cannot be derived — only measured, the same way the six faces were.
    ///
    /// The measurement: laid flat with the screen up and turned until the text read the right
    /// way round, the board has its **USB-C port to the right**. Screen up puts `+Z` toward the
    /// reader and USB-C on the right puts `+X` to their left, so — the board frame being
    /// right-handed, `Y = Z × X` — `+Y` points *at* the reader and the top of the image is
    /// `−Y`, the right edge. Every other row follows by turning the image a quarter at a time,
    /// clockwise, which walks the top through `−Y → −X → +Y → +X`.
    ///
    /// It checks against the poses independently: `−Y` up is the board resting on its **left**
    /// edge, which is the "screen toward you, USB-C on the right" pose measured at `y = −986`
    /// — the stick held horizontally, exactly where a landscape picture is legible.
    const fn rotation(self) -> ScreenRotation {
        match self {
            UpAxis::RightEdge => ScreenRotation::Deg0,
            UpAxis::UsbEnd => ScreenRotation::Deg90,
            UpAxis::LeftEdge => ScreenRotation::Deg180,
            UpAxis::StickTop => ScreenRotation::Deg270,
        }
    }
}

/// The in-plane axis clearly pointing up, or `None` when none is.
///
/// `None` covers the two cases that must not turn the picture, and they are genuinely
/// different: a board lying flat has no in-plane direction to read, and a board resting near a
/// diagonal has one that is a coin toss. Both mean "keep what you have", which is why they
/// collapse to one answer here.
fn up_axis(acceleration: Acceleration) -> Option<UpAxis> {
    let Acceleration { x_mg, y_mg, .. } = acceleration;
    let (across, along): (i32, i32) = (x_mg.abs(), y_mg.abs());

    if across.max(along) < IN_PLANE_MINIMUM_MG {
        return None;
    }

    // The dominant axis must clear the other by the margin, so the diagonals are a dead band
    // rather than a hair trigger.
    if across >= along + QUADRANT_MARGIN_MG {
        Some(if x_mg > 0 {
            UpAxis::StickTop
        } else {
            UpAxis::UsbEnd
        })
    } else if along >= across + QUADRANT_MARGIN_MG {
        Some(if y_mg > 0 {
            UpAxis::LeftEdge
        } else {
            UpAxis::RightEdge
        })
    } else {
        None
    }
}

/// The rotation the picture should be drawn at, given `acceleration` and the rotation
/// `current`ly showing.
///
/// Total, and deliberately conservative: it returns `current` unchanged for every reading it
/// cannot make a confident case against. Three separate situations do that, and each is a real
/// one rather than a defensive default —
///
/// - **The board is moving.** An accelerometer in a moving hand reports gravity plus the hand,
///   so the "up" it implies is not up. Turning the picture mid-swing would spin the glass
///   exactly while someone is trying to read it, which is why this settles rather than tracks.
/// - **The board is flat.** All of the gravity is on Z, and neither in-plane axis has anything
///   to say. A flat board keeps the rotation it was last read at, so setting the stick down on
///   the desk does not scramble the picture.
/// - **The board is near a diagonal.** Inside [`QUADRANT_MARGIN_MG`] the two candidate
///   rotations are equally defensible, and flipping between them on noise is worse than
///   either.
pub fn rotation_for(acceleration: Acceleration, current: ScreenRotation) -> ScreenRotation {
    if !is_at_rest(acceleration) {
        return current;
    }
    match up_axis(acceleration) {
        Some(up) => up.rotation(),
        None => current,
    }
}

/// How long a new rotation must hold before the picture actually turns, in milliseconds.
///
/// A settled reading is not yet an intention. Setting the stick down passes through poses on
/// the way, and each one is briefly "at rest" as the hand lets go — turning on the first of
/// them would make the glass flicker through two rotations before landing. A quarter second is
/// longer than those transients and shorter than anyone's patience.
pub const ROTATION_SETTLE_MS: Tick = 250;

/// The rotation currently showing, and the candidate waiting to replace it.
///
/// Holds the one piece of state the rule needs that a single reading cannot carry: *how long*
/// the board has been arguing for a different rotation. Kept here as a value rather than in
/// the shell, so the whole settle-then-turn behaviour is provable by handing it readings and
/// timestamps.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RotationSettler {
    showing: ScreenRotation,
    candidate: Option<(ScreenRotation, Tick)>,
}

impl RotationSettler {
    /// A settler showing the panel's native landscape, with nothing pending.
    pub const fn new() -> Self {
        RotationSettler {
            showing: ScreenRotation::Deg0,
            candidate: None,
        }
    }

    /// The rotation the glass should be drawn at right now.
    pub const fn showing(&self) -> ScreenRotation {
        self.showing
    }

    /// Fold in a reading taken at `now`, and report the rotation to draw at.
    ///
    /// A reading agreeing with what is showing clears any pending candidate — a board that
    /// wobbled toward another quadrant and came back must not carry that wobble forward.
    pub fn update(&mut self, acceleration: Acceleration, now: Tick) -> ScreenRotation {
        let wanted: ScreenRotation = rotation_for(acceleration, self.showing);

        if wanted == self.showing {
            self.candidate = None;
            return self.showing;
        }

        match self.candidate {
            // A different candidate than last time: start its clock over.
            Some((pending, _)) if pending != wanted => {
                self.candidate = Some((wanted, now));
            }
            // The same candidate, held long enough: turn.
            Some((pending, since)) if now.saturating_sub(since) >= ROTATION_SETTLE_MS => {
                self.showing = pending;
                self.candidate = None;
            }
            // The same candidate, still too new: keep waiting.
            Some(_) => {}
            // First sighting of this candidate.
            None => self.candidate = Some((wanted, now)),
        }

        self.showing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ONE_G_MG;

    /// A board held with its stick-top at the sky, standing on the USB-C port.
    const UPRIGHT: Acceleration = Acceleration::new(ONE_G_MG, 0, 0);
    /// A board lying flat on its back.
    const FLAT: Acceleration = Acceleration::new(0, 0, ONE_G_MG);

    /// Zero: a weightless reading turns nothing — it is not gravity, so it points nowhere.
    #[test]
    fn a_weightless_reading_keeps_the_current_rotation() {
        assert_eq!(
            rotation_for(Acceleration::default(), ScreenRotation::Deg90),
            ScreenRotation::Deg90
        );
    }

    /// One: a board stood on its USB-C port puts the stick's top up, and the picture follows.
    #[test]
    fn standing_the_stick_up_turns_the_picture_to_match() {
        assert_eq!(
            rotation_for(UPRIGHT, ScreenRotation::Deg0),
            UpAxis::StickTop.rotation()
        );
    }

    /// Many: each of the four in-plane directions names its own rotation, and no two agree.
    #[test]
    fn the_four_in_plane_directions_name_four_distinct_rotations() {
        let found: [ScreenRotation; 4] = [
            rotation_for(Acceleration::new(ONE_G_MG, 0, 0), ScreenRotation::Deg0),
            rotation_for(Acceleration::new(-ONE_G_MG, 0, 0), ScreenRotation::Deg0),
            rotation_for(Acceleration::new(0, ONE_G_MG, 0), ScreenRotation::Deg0),
            rotation_for(Acceleration::new(0, -ONE_G_MG, 0), ScreenRotation::Deg0),
        ];
        assert_eq!(
            found,
            [
                UpAxis::StickTop.rotation(),
                UpAxis::UsbEnd.rotation(),
                UpAxis::LeftEdge.rotation(),
                UpAxis::RightEdge.rotation(),
            ]
        );
        // Four inputs, four different answers — no direction shares a rotation with another.
        assert!(found.iter().all(|one: &ScreenRotation| found
            .iter()
            .filter(|other: &&ScreenRotation| *other == one)
            .count()
            == 1));
    }

    /// The measured table itself, stated literally.
    ///
    /// Every other test here asks `UpAxis::_.rotation()` what the answer is, which keeps them
    /// about structure — but means they would all pass just as happily against a table turned
    /// a quarter the wrong way. This one is the measurement, written down: the board laid flat
    /// with its screen up reads the right way round with the **USB-C port to the right**, which
    /// puts `−Y` at the top of the native image. Change these four lines only with the board in
    /// your hand.
    #[test]
    fn the_measured_rotation_table_is_what_the_board_said() {
        assert_eq!(UpAxis::RightEdge.rotation(), ScreenRotation::Deg0);
        assert_eq!(UpAxis::UsbEnd.rotation(), ScreenRotation::Deg90);
        assert_eq!(UpAxis::LeftEdge.rotation(), ScreenRotation::Deg180);
        assert_eq!(UpAxis::StickTop.rotation(), ScreenRotation::Deg270);
    }

    /// The pose a landscape picture is legible in is the one that needs no rotation at all.
    /// If this ever fails, the readout has started asking to be turned in its resting pose.
    #[test]
    fn the_natural_reading_pose_needs_no_rotation() {
        // Screen toward the reader with USB-C on the right: the board rests on its left edge,
        // so the right edge is up and `-Y` carries the gravity. Measured at y = -986.
        let on_the_left_edge: Acceleration = Acceleration::new(-3, -986, 76);
        assert_eq!(
            rotation_for(on_the_left_edge, ScreenRotation::Deg180),
            ScreenRotation::Deg0
        );
    }

    /// Turning the image clockwise walks its top around the board the other way, one quarter
    /// at a time. This is the relationship the four rows encode, checked as a cycle rather
    /// than as four independent facts.
    #[test]
    fn a_quarter_turn_of_the_image_is_a_quarter_turn_of_its_top() {
        let clockwise: [UpAxis; 4] = [
            UpAxis::RightEdge,
            UpAxis::UsbEnd,
            UpAxis::LeftEdge,
            UpAxis::StickTop,
        ];
        let turns: [ScreenRotation; 4] = [
            ScreenRotation::Deg0,
            ScreenRotation::Deg90,
            ScreenRotation::Deg180,
            ScreenRotation::Deg270,
        ];
        (0..4).for_each(|step: usize| {
            assert_eq!(clockwise[step].rotation(), turns[step]);
            // Two steps around the cycle is a half turn, on every axis.
            assert_eq!(
                clockwise[(step + 2) % 4].rotation(),
                turns[step].opposite(),
                "opposite edges must be opposite rotations"
            );
        });
    }

    /// Opposite in-plane directions are opposite rotations — the property that makes the
    /// picture turn *with* the board rather than merely differently.
    #[test]
    fn opposing_directions_are_opposing_rotations() {
        let top: ScreenRotation = rotation_for(UPRIGHT, ScreenRotation::Deg0);
        let usb: ScreenRotation =
            rotation_for(Acceleration::new(-ONE_G_MG, 0, 0), ScreenRotation::Deg0);
        assert_eq!(top.opposite(), usb);

        let left: ScreenRotation =
            rotation_for(Acceleration::new(0, ONE_G_MG, 0), ScreenRotation::Deg0);
        let right: ScreenRotation =
            rotation_for(Acceleration::new(0, -ONE_G_MG, 0), ScreenRotation::Deg0);
        assert_eq!(left.opposite(), right);
    }

    /// A flat board keeps whatever rotation it had — there is no in-plane direction to read,
    /// and snapping to a default would scramble the picture every time it is set down.
    #[test]
    fn a_flat_board_keeps_the_rotation_it_had() {
        assert_eq!(
            rotation_for(FLAT, ScreenRotation::Deg270),
            ScreenRotation::Deg270
        );
        assert_eq!(
            rotation_for(Acceleration::new(0, 0, -ONE_G_MG), ScreenRotation::Deg90),
            ScreenRotation::Deg90
        );
    }

    /// A board being moved keeps its rotation even when its vector points somewhere definite:
    /// the vector is gravity plus a hand, so the direction it names is not up.
    #[test]
    fn a_moving_board_keeps_its_rotation() {
        let swung: Acceleration = Acceleration::new(2 * ONE_G_MG, 0, 0);
        assert_eq!(
            rotation_for(swung, ScreenRotation::Deg180),
            ScreenRotation::Deg180
        );
    }

    /// On the diagonal the two candidates are equally good, so the picture holds still rather
    /// than flipping between them.
    #[test]
    fn a_board_on_the_diagonal_keeps_its_rotation() {
        let corner: Acceleration = Acceleration::new(707, 707, 0);
        assert_eq!(
            rotation_for(corner, ScreenRotation::Deg0),
            ScreenRotation::Deg0
        );
        assert_eq!(
            rotation_for(corner, ScreenRotation::Deg180),
            ScreenRotation::Deg180
        );
    }

    /// The dead band is symmetric: just inside the margin holds, just outside it turns. This
    /// is the boundary the whole no-flicker claim rests on.
    #[test]
    fn the_dead_band_holds_until_one_axis_clears_the_margin() {
        // Y leads X by one short of the margin — not yet decisive.
        let undecided: Acceleration = Acceleration::new(500, 500 + QUADRANT_MARGIN_MG - 1, 0);
        assert_eq!(
            rotation_for(undecided, ScreenRotation::Deg0),
            ScreenRotation::Deg0
        );

        // One more milli-g and it is.
        let decided: Acceleration = Acceleration::new(500, 500 + QUADRANT_MARGIN_MG, 0);
        assert_eq!(
            rotation_for(decided, ScreenRotation::Deg0),
            UpAxis::LeftEdge.rotation()
        );
    }

    /// A board tilted too little to point anywhere keeps its rotation, even resting and even
    /// with one axis nominally larger than the other.
    #[test]
    fn a_barely_tilted_board_keeps_its_rotation() {
        // 300 mg of in-plane lean, under the minimum, the rest on Z.
        let barely: Acceleration = Acceleration::new(300, 0, 954);
        assert!(is_at_rest(barely), "the fixture must be a resting reading");
        assert_eq!(
            rotation_for(barely, ScreenRotation::Deg90),
            ScreenRotation::Deg90
        );
    }

    /// Portrait is exactly the two quarter turns, and `opposite` stays inside the pair.
    #[test]
    fn the_quarter_turns_are_the_portrait_ones() {
        assert!(!ScreenRotation::Deg0.is_portrait());
        assert!(!ScreenRotation::Deg180.is_portrait());
        assert!(ScreenRotation::Deg90.is_portrait());
        assert!(ScreenRotation::Deg270.is_portrait());
        assert_eq!(
            ScreenRotation::Deg90.opposite(),
            ScreenRotation::Deg270,
            "a half turn from portrait must stay portrait"
        );
    }

    /// Turning twice by a half turn is where it started, on every rotation.
    #[test]
    fn a_half_turn_twice_is_the_identity() {
        [
            ScreenRotation::Deg0,
            ScreenRotation::Deg90,
            ScreenRotation::Deg180,
            ScreenRotation::Deg270,
        ]
        .iter()
        .for_each(|rotation: &ScreenRotation| {
            assert_eq!(rotation.opposite().opposite(), *rotation);
        });
    }

    /// Zero: a settler nothing has been fed shows the native landscape.
    #[test]
    fn a_fresh_settler_shows_the_native_landscape() {
        assert_eq!(RotationSettler::new().showing(), ScreenRotation::Deg0);
    }

    /// One: a new rotation does not take effect on its first sighting — it has to hold.
    #[test]
    fn a_new_rotation_does_not_turn_the_picture_at_once() {
        let mut settler: RotationSettler = RotationSettler::new();
        assert_eq!(settler.update(UPRIGHT, 0), ScreenRotation::Deg0);
        assert_eq!(
            settler.update(UPRIGHT, ROTATION_SETTLE_MS - 1),
            ScreenRotation::Deg0
        );
    }

    /// Many: held for the settle time, it turns — and stays turned.
    #[test]
    fn a_rotation_held_long_enough_turns_the_picture_and_stays() {
        let mut settler: RotationSettler = RotationSettler::new();
        settler.update(UPRIGHT, 0);
        assert_eq!(
            settler.update(UPRIGHT, ROTATION_SETTLE_MS),
            UpAxis::StickTop.rotation()
        );
        assert_eq!(
            settler.update(UPRIGHT, 10_000),
            UpAxis::StickTop.rotation(),
            "a settled rotation must not drift once it has turned"
        );
    }

    /// A wobble toward another quadrant that comes back leaves nothing behind: the picture
    /// must not turn later because of a transient it has already recovered from.
    #[test]
    fn a_wobble_that_returns_does_not_turn_the_picture() {
        // Showing Deg0, which is the rotation `-Y` up asks for.
        let settled: Acceleration = Acceleration::new(0, -ONE_G_MG, 0);
        let mut settler: RotationSettler = RotationSettler::new();
        assert_eq!(settler.update(settled, 0), ScreenRotation::Deg0);

        // A wobble toward another quadrant, then back to agreeing before it could settle.
        settler.update(UPRIGHT, 100);
        assert_eq!(settler.update(settled, 200), ScreenRotation::Deg0);

        // The wobble reappears long afterward. Its clock must start again from here, not
        // inherit the age of the earlier one — so this reading does not turn the picture.
        assert_eq!(
            settler.update(UPRIGHT, 5_000),
            ScreenRotation::Deg0,
            "the earlier wobble carried its age forward"
        );
        // ...and it turns only once the new candidate has held for the settle time itself.
        assert_eq!(
            settler.update(UPRIGHT, 5_000 + ROTATION_SETTLE_MS),
            UpAxis::StickTop.rotation()
        );
    }

    /// A candidate replaced by a different candidate restarts the clock rather than
    /// inheriting the first one's age.
    #[test]
    fn a_changed_candidate_restarts_the_settle_clock() {
        let mut settler: RotationSettler = RotationSettler::new();
        settler.update(UPRIGHT, 0);
        // A different quadrant arrives just before the first would have settled.
        let usb_up: Acceleration = Acceleration::new(-ONE_G_MG, 0, 0);
        settler.update(usb_up, ROTATION_SETTLE_MS - 1);
        // The first candidate's deadline passes, but it is no longer the candidate.
        assert_eq!(
            settler.update(usb_up, ROTATION_SETTLE_MS),
            ScreenRotation::Deg0
        );
        // The second one settles on its own clock.
        assert_eq!(
            settler.update(usb_up, ROTATION_SETTLE_MS * 2),
            UpAxis::UsbEnd.rotation()
        );
    }

    /// Being moved never turns the picture, however long the movement lasts.
    #[test]
    fn a_long_movement_never_turns_the_picture() {
        let mut settler: RotationSettler = RotationSettler::new();
        let swung: Acceleration = Acceleration::new(2 * ONE_G_MG, 0, 0);
        assert_eq!(settler.update(swung, 0), ScreenRotation::Deg0);
        assert_eq!(settler.update(swung, 10_000), ScreenRotation::Deg0);
    }
}
