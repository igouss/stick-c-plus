//! Which way the board is resting — the coarse, nameable answer to "where is up?".

use platform_core::{Acceleration, ONE_G_MG};

/// How much of a gravity the dominant axis must carry before a face is named.
///
/// At rest the gravity vector has magnitude `1 g` and points along whichever axis is up, so a
/// clearly-resting board puts most of that on one axis. Held at a corner — a true 45° between
/// two faces — each axis carries about `707 mg`, and naming either one would be a coin toss
/// dressed up as a fact. `800 mg` sits above that ambiguity: a face is named only when the
/// board really is lying on it, and anything in between reads [`Facing::Tilted`] instead.
pub const FACE_THRESHOLD_MG: i32 = 800;

/// How far the vector's magnitude may stray from `1 g` and still be read as gravity alone.
///
/// An accelerometer measures *proper* acceleration, so a board being moved reports gravity
/// plus whatever the hand is doing. When the total is far from `1 g` the vector is not
/// pointing at the earth and no face it implies is trustworthy — that reads
/// [`Facing::Moving`], which is a different fact from "resting between two faces".
pub const REST_TOLERANCE_MG: i32 = 250;

/// The board's resting pose, as the gravity vector names it.
///
/// ## The axis convention
///
/// These names assume the M5StickC Plus mounts its MPU6886 so that, with the stick held
/// screen-toward-you and the USB-C port at the bottom:
///
/// - **+Z** points out of the screen — so a board lying on its back reads `+1 g` on Z.
/// - **+X** points along the stick toward the top (away from the USB-C port).
/// - **+Y** points out of the stick's left edge.
///
/// The mapping is a fact about how the part is soldered, not about the driver, so it lives
/// here in one place: if the board disagrees, [`facing_of`] is the single function to correct
/// and every label follows. The screen shows the raw signed X/Y/Z alongside the name for
/// exactly this reason — the numbers are checkable against the name by eye.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Facing {
    /// Lying on its back, screen at the sky.
    ScreenUp,
    /// Lying face down, screen at the table.
    ScreenDown,
    /// Standing upright on the USB-C port, screen vertical.
    Upright,
    /// Standing on its top edge, upside down.
    Inverted,
    /// Resting on its left edge.
    LeftEdge,
    /// Resting on its right edge.
    RightEdge,
    /// Between faces — held at an angle, resting on a corner, with no axis clearly down.
    Tilted,
    /// Not resting at all: the vector's magnitude is too far from `1 g` for it to be
    /// gravity, so it names no pose. A board being picked up or shaken reads this.
    Moving,
}

impl Default for Facing {
    /// A board that has told us nothing yet is not resting on anything we can name.
    fn default() -> Self {
        Facing::Moving
    }
}

impl Facing {
    /// A short, fixed-width-friendly name for the glass.
    ///
    /// Ten characters at the widest, so every face fits one label field and a shorter name
    /// erases a longer one in place.
    pub const fn label(self) -> &'static str {
        match self {
            Facing::ScreenUp => "SCREEN UP",
            Facing::ScreenDown => "SCREEN DN",
            Facing::Upright => "UPRIGHT",
            Facing::Inverted => "INVERTED",
            Facing::LeftEdge => "LEFT EDGE",
            Facing::RightEdge => "RIGHT EDGE",
            Facing::Tilted => "TILTED",
            Facing::Moving => "MOVING",
        }
    }

    /// Whether this face names a settled pose, rather than reporting that it cannot.
    pub const fn is_resting(self) -> bool {
        !matches!(self, Facing::Tilted | Facing::Moving)
    }
}

/// Whether `acceleration`'s magnitude is close enough to `1 g` to be gravity alone.
///
/// Compared as squares so the check needs no square root — the same discipline
/// [`magnitude_squared_mg2`](platform_core::Acceleration::magnitude_squared_mg2) is built for.
fn is_at_rest(acceleration: Acceleration) -> bool {
    let low: i64 = i64::from(ONE_G_MG - REST_TOLERANCE_MG);
    let high: i64 = i64::from(ONE_G_MG + REST_TOLERANCE_MG);
    let magnitude_squared: i64 = acceleration.magnitude_squared_mg2();
    magnitude_squared >= low * low && magnitude_squared <= high * high
}

/// The pose `acceleration` implies.
///
/// Reports [`Facing::Moving`] when the vector is not gravity, [`Facing::Tilted`] when it is
/// gravity but no axis clearly dominates, and otherwise the face the dominant axis names.
/// The two "cannot say" answers are deliberately distinct: a board in mid-air and a board
/// resting on a corner are different situations, and collapsing them would throw away the
/// only cue that tells a user to put it down.
pub fn facing_of(acceleration: Acceleration) -> Facing {
    if !is_at_rest(acceleration) {
        return Facing::Moving;
    }

    let Acceleration { x_mg, y_mg, z_mg } = acceleration;
    // The dominant axis must both out-carry the other two and clear the threshold, so a
    // corner-held board names no face rather than picking the marginally larger axis.
    let dominant: i32 = x_mg.abs().max(y_mg.abs()).max(z_mg.abs());
    if dominant < FACE_THRESHOLD_MG {
        return Facing::Tilted;
    }

    // One `if` chain over the three axes, ordered by which carries the dominant magnitude.
    // See the type docs for the board's axis convention.
    if z_mg.abs() == dominant {
        if z_mg > 0 {
            Facing::ScreenUp
        } else {
            Facing::ScreenDown
        }
    } else if x_mg.abs() == dominant {
        if x_mg > 0 {
            Facing::Inverted
        } else {
            Facing::Upright
        }
    } else if y_mg > 0 {
        Facing::LeftEdge
    } else {
        Facing::RightEdge
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clean one-gravity reading down the named axis.
    fn resting(x_mg: i32, y_mg: i32, z_mg: i32) -> Acceleration {
        Acceleration::new(x_mg, y_mg, z_mg)
    }

    /// Zero: a weightless reading names no pose — it is not gravity at all.
    #[test]
    fn a_weightless_reading_is_moving_not_a_face() {
        assert_eq!(facing_of(Acceleration::default()), Facing::Moving);
    }

    /// One: the pose the board actually sits in on the desk — on its back, screen up.
    #[test]
    fn a_board_on_its_back_faces_screen_up() {
        assert_eq!(facing_of(resting(0, 0, ONE_G_MG)), Facing::ScreenUp);
    }

    /// Many: every axis and sign names its own distinct face — six poses, six answers.
    #[test]
    fn every_axis_and_sign_names_its_own_face() {
        assert_eq!(facing_of(resting(0, 0, ONE_G_MG)), Facing::ScreenUp);
        assert_eq!(facing_of(resting(0, 0, -ONE_G_MG)), Facing::ScreenDown);
        assert_eq!(facing_of(resting(-ONE_G_MG, 0, 0)), Facing::Upright);
        assert_eq!(facing_of(resting(ONE_G_MG, 0, 0)), Facing::Inverted);
        assert_eq!(facing_of(resting(0, ONE_G_MG, 0)), Facing::LeftEdge);
        assert_eq!(facing_of(resting(0, -ONE_G_MG, 0)), Facing::RightEdge);
    }

    /// A board held at a true 45° corner names no face: both axes carry ~707 mg, under the
    /// threshold, so it reads Tilted rather than guessing between two equally-good answers.
    #[test]
    fn a_corner_held_board_is_tilted_not_a_guessed_face() {
        assert_eq!(facing_of(resting(0, 707, 707)), Facing::Tilted);
    }

    /// Being moved is distinct from being tilted: a 2 g reading is not gravity, so it
    /// reports Moving even though one axis dominates overwhelmingly.
    #[test]
    fn a_shaken_board_reads_moving_even_with_a_dominant_axis() {
        assert_eq!(facing_of(resting(0, 0, 2 * ONE_G_MG)), Facing::Moving);
    }

    /// A gentle tilt off a face still names that face — the threshold is a tolerance for
    /// real desks, not a demand for a perfectly level one.
    #[test]
    fn a_slightly_tilted_board_still_names_its_face() {
        // ~20° off flat: 940 mg on Z, 342 mg on X — magnitude still 1 g.
        assert_eq!(facing_of(resting(342, 0, 940)), Facing::ScreenUp);
    }

    /// Only the two "cannot say" answers report themselves as unsettled.
    #[test]
    fn the_resting_faces_are_exactly_the_nameable_poses() {
        assert!(Facing::ScreenUp.is_resting());
        assert!(Facing::RightEdge.is_resting());
        assert!(!Facing::Tilted.is_resting());
        assert!(!Facing::Moving.is_resting());
    }

    /// Every label fits the ten-character field the glass reserves for it, so a face name
    /// can never overflow its line or fail to erase a longer predecessor.
    #[test]
    fn every_label_fits_the_screens_field() {
        let faces: [Facing; 8] = [
            Facing::ScreenUp,
            Facing::ScreenDown,
            Facing::Upright,
            Facing::Inverted,
            Facing::LeftEdge,
            Facing::RightEdge,
            Facing::Tilted,
            Facing::Moving,
        ];
        faces.iter().for_each(|face: &Facing| {
            assert!(
                face.label().len() <= 10,
                "{:?}'s label does not fit the field",
                face
            );
        });
    }
}
