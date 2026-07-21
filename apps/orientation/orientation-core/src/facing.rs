//! Which way the board is resting — the coarse, nameable answer to "where is up?".

use platform_core::{is_at_rest, Acceleration};

/// How much of a gravity the dominant axis must carry before a face is named.
///
/// At rest the gravity vector has magnitude `1 g` and points along whichever axis is up, so a
/// clearly-resting board puts most of that on one axis. Held at a corner — a true 45° between
/// two faces — each axis carries about `707 mg`, and naming either one would be a coin toss
/// dressed up as a fact. `800 mg` sits above that ambiguity: a face is named only when the
/// board really is lying on it, and anything in between reads [`Facing::Tilted`] instead.
pub const FACE_THRESHOLD_MG: i32 = 800;

// Whether a vector is gravity alone is [`is_at_rest`](platform_core::is_at_rest), in the
// shared kernel: it is a fact about an accelerometer reading rather than about this app's
// vocabulary of poses. What *this* crate adds is the consequence — a vector that is not
// gravity reads [`Facing::Moving`], which is a different fact from "resting between two
// faces".

/// The board's resting pose, as the gravity vector names it.
///
/// ## The axis convention
///
/// These names read the *board* frame the [`Imu`](platform_core::Imu) port promises — with the
/// stick held screen-toward-you and the USB-C port at the bottom:
///
/// - **+Z** points out of the screen.
/// - **+X** points along the stick toward the top (away from the USB-C port).
/// - **+Y** points out of the stick's left edge.
///
/// How the MPU6886 is actually soldered is *not* a fact about this crate: the part sits a
/// quarter turn about Z from these axes, and the adapter rotates every reading before it
/// crosses the port. That is why this file names no chip and needs no correction if the part
/// is ever remounted.
///
/// ## Which way a resting axis reads
///
/// An accelerometer measures proper acceleration, so at rest it reads `+1 g` along whichever
/// axis points **up**, away from the earth — not along the one pointing down. A board lying on
/// its back therefore reads `z = +1 g`, and a board standing on its USB-C port has its top in
/// the air and reads `x = +1 g`.
///
/// This is the half of the convention that is easy to get backwards, and getting it backwards
/// is invisible in isolation: every pose still produces a confident name, just the *opposite*
/// one, and only two of the six faces look wrong at a glance. The screen shows the raw signed
/// X/Y/Z alongside the name for exactly this reason — the numbers are checkable against the
/// name by eye, which is how the six faces were verified on the metal.
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
    // Each arm names the face whose *up* direction that axis points along, because a resting
    // axis reads positive when it points at the sky. See the type docs.
    if z_mg.abs() == dominant {
        if z_mg > 0 {
            Facing::ScreenUp
        } else {
            Facing::ScreenDown
        }
    } else if x_mg.abs() == dominant {
        if x_mg > 0 {
            Facing::Upright
        } else {
            Facing::Inverted
        }
    } else if y_mg > 0 {
        Facing::RightEdge
    } else {
        Facing::LeftEdge
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::ONE_G_MG;

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
    ///
    /// Each line reads "the axis pointing up names the face resting down". All six were
    /// checked against the board itself; the adapter's rotation is what makes these the same
    /// numbers the metal produces.
    #[test]
    fn every_axis_and_sign_names_its_own_face() {
        assert_eq!(facing_of(resting(0, 0, ONE_G_MG)), Facing::ScreenUp);
        assert_eq!(facing_of(resting(0, 0, -ONE_G_MG)), Facing::ScreenDown);
        assert_eq!(facing_of(resting(ONE_G_MG, 0, 0)), Facing::Upright);
        assert_eq!(facing_of(resting(-ONE_G_MG, 0, 0)), Facing::Inverted);
        assert_eq!(facing_of(resting(0, -ONE_G_MG, 0)), Facing::LeftEdge);
        assert_eq!(facing_of(resting(0, ONE_G_MG, 0)), Facing::RightEdge);
    }

    /// The sign convention itself, stated once as a test: a resting axis reads *positive*
    /// when it points at the sky. Getting this backwards names every face its opposite, which
    /// is exactly what shipped before the six faces were checked on the board.
    #[test]
    fn a_resting_axis_reads_positive_when_it_points_up() {
        // The board on its back has its screen — and so +Z — pointing at the sky.
        assert_eq!(facing_of(resting(0, 0, ONE_G_MG)), Facing::ScreenUp);
        // Standing on the USB-C port puts the stick's top — and so +X — in the air.
        assert_eq!(facing_of(resting(ONE_G_MG, 0, 0)), Facing::Upright);
        // Resting on the left edge puts the *right* edge up, and +Y points out of the left.
        assert_eq!(facing_of(resting(0, -ONE_G_MG, 0)), Facing::LeftEdge);
    }

    /// Opposite readings name opposite faces, on every axis. A convention that drifted on one
    /// axis only would still pass a per-face check that happened to test the other sign.
    #[test]
    fn opposite_readings_name_opposing_faces() {
        let opposites: [(Facing, Facing); 3] = [
            (
                facing_of(resting(0, 0, ONE_G_MG)),
                facing_of(resting(0, 0, -ONE_G_MG)),
            ),
            (
                facing_of(resting(ONE_G_MG, 0, 0)),
                facing_of(resting(-ONE_G_MG, 0, 0)),
            ),
            (
                facing_of(resting(0, ONE_G_MG, 0)),
                facing_of(resting(0, -ONE_G_MG, 0)),
            ),
        ];
        assert_eq!(
            opposites,
            [
                (Facing::ScreenUp, Facing::ScreenDown),
                (Facing::Upright, Facing::Inverted),
                (Facing::RightEdge, Facing::LeftEdge),
            ]
        );
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
