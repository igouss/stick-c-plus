//! How steeply the board is tilted — the fine-grained answer the coarse [`Facing`] rounds off.
//!
//! [`Facing`](crate::Facing) names the pose in words; this names it in degrees. Two boards
//! both reading `SCREEN UP` can sit at noticeably different angles, and a readout that only
//! ever said `SCREEN UP` would hide the difference the user is holding in their hand.

use platform_core::Acceleration;

/// A quarter turn, in degrees — the steepest a pitch can be.
pub const RIGHT_ANGLE_DEG: i32 = 90;
/// A half turn, in degrees — the widest a roll can be, either way.
pub const STRAIGHT_ANGLE_DEG: i32 = 180;

/// The board's tilt, in whole degrees.
///
/// Whole degrees, not tenths: the panel shows a live readout, and a fractional digit that
/// flickers on sensor noise is worse than useless — it repaints constantly and reads as
/// instability rather than as precision. Whole degrees also mean two nearly-identical
/// readings are the *same* value, so the render loop's change-suppression can do its job.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Attitude {
    /// Rotation about the board's short axis — how far the stick's top is raised or lowered,
    /// in `-90..=90` degrees. Zero is flat.
    pub pitch_deg: i32,
    /// Rotation about the board's long axis — how far the stick is rolled onto an edge, in
    /// `-180..=180` degrees. Zero is screen-up-and-level.
    pub roll_deg: i32,
}

/// Radians to degrees.
const DEGREES_PER_RADIAN: f32 = 180.0 / core::f32::consts::PI;

/// The tilt `acceleration` implies.
///
/// At rest the acceleration vector points at the sky, so its direction *is* the board's
/// attitude — these are the standard two accelerometer angles:
///
/// - **roll** is `atan2(y, z)`: how the gravity vector is split between the screen normal and
///   the board's short axis, over the full `-180..=180` so an upside-down board is
///   distinguishable from an upright one.
/// - **pitch** is `atan2(x, hypot(y, z))`: how far the long axis has lifted out of the plane
///   the other two span. Measured against the *combined* magnitude of the other axes rather
///   than one of them, so a pitch reading does not change as the board is rolled.
///
/// Both follow the same sign rule the pose names do: a resting axis reads positive when it
/// points at the sky, so raising the stick's top raises `x` and raises the pitch. Pitch was
/// once `atan2(-x, ...)`, which reported a board standing on its USB-C port as `-90°` — a
/// board tipped nose-up reading nose-down, and the same inverted convention that misnamed
/// four of the six faces.
///
/// A zero vector — free fall, or a sensor that has stopped reporting — yields a zero attitude
/// rather than an error: `atan2(0, 0)` is defined as `0`, and the caller already has
/// [`Facing::Moving`](crate::Facing::Moving) to tell it the reading is not gravity. This
/// function's contract is a total one, so no caller has to handle an impossible failure.
pub fn attitude_of(acceleration: Acceleration) -> Attitude {
    let x: f32 = acceleration.x_mg as f32;
    let y: f32 = acceleration.y_mg as f32;
    let z: f32 = acceleration.z_mg as f32;

    let roll: f32 = libm::atan2f(y, z) * DEGREES_PER_RADIAN;
    let pitch: f32 = libm::atan2f(x, libm::sqrtf(y * y + z * z)) * DEGREES_PER_RADIAN;

    Attitude {
        pitch_deg: round_to_degrees(pitch),
        roll_deg: round_to_degrees(roll),
    }
}

/// Round an angle to the nearest whole degree.
///
/// `as i32` truncates toward zero, which would round `-0.6°` to `0°` and `0.6°` to `0°`
/// alike — a bias that shows up as a readout stuck one degree shy of the truth. Adding a
/// signed half before truncating rounds half away from zero, symmetrically for both signs.
fn round_to_degrees(angle: f32) -> i32 {
    let half: f32 = if angle >= 0.0 { 0.5 } else { -0.5 };
    (angle + half) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::ONE_G_MG;
    use proptest::prelude::*;

    /// Zero: a board lying flat on its back is level — no pitch, no roll.
    #[test]
    fn a_flat_board_is_level() {
        assert_eq!(
            attitude_of(Acceleration::new(0, 0, ONE_G_MG)),
            Attitude {
                pitch_deg: 0,
                roll_deg: 0,
            }
        );
    }

    /// Zero: a free-falling board reports a level attitude rather than failing — the total
    /// contract the docs promise.
    #[test]
    fn a_zero_vector_yields_a_level_attitude() {
        assert_eq!(attitude_of(Acceleration::default()), Attitude::default());
    }

    /// One: standing the stick on its USB-C port is a full quarter turn of pitch, *upward* —
    /// the top of the stick is the end in the air, so `+X` carries the gravity and the pitch
    /// is positive.
    #[test]
    fn an_upright_board_pitches_a_quarter_turn() {
        let upright: Attitude = attitude_of(Acceleration::new(ONE_G_MG, 0, 0));
        assert_eq!(upright.pitch_deg, RIGHT_ANGLE_DEG);
    }

    /// ...and standing it on its top edge pitches the other way by the same quarter turn. The
    /// pair is what pins the sign: a formula with the sign inverted passes either one alone.
    #[test]
    fn an_inverted_board_pitches_the_other_way() {
        let inverted: Attitude = attitude_of(Acceleration::new(-ONE_G_MG, 0, 0));
        assert_eq!(inverted.pitch_deg, -RIGHT_ANGLE_DEG);
    }

    /// One: rolling the stick onto an edge is a quarter turn of roll, and the sign follows
    /// the direction it was rolled.
    #[test]
    fn rolling_onto_an_edge_is_a_signed_quarter_turn() {
        assert_eq!(
            attitude_of(Acceleration::new(0, ONE_G_MG, 0)).roll_deg,
            RIGHT_ANGLE_DEG
        );
        assert_eq!(
            attitude_of(Acceleration::new(0, -ONE_G_MG, 0)).roll_deg,
            -RIGHT_ANGLE_DEG
        );
    }

    /// Many: a board turned fully over reads a half turn of roll — the reason roll spans
    /// `-180..=180` rather than the quarter-turn range a pitch needs. A `+-90` roll could not
    /// tell face-up from face-down at all.
    #[test]
    fn an_upside_down_board_reads_a_half_turn_of_roll() {
        let inverted: Attitude = attitude_of(Acceleration::new(0, 0, -ONE_G_MG));
        assert_eq!(inverted.roll_deg.abs(), STRAIGHT_ANGLE_DEG);
    }

    /// Many: the familiar 45° diagonal comes out at 45°, both in pitch and in roll — the one
    /// angle whose answer can be checked without trusting the arctangent.
    #[test]
    fn a_diagonal_reads_forty_five_degrees() {
        // 707 mg on two axes is a unit vector at 45°.
        assert_eq!(attitude_of(Acceleration::new(0, 707, 707)).roll_deg, 45);
        assert_eq!(attitude_of(Acceleration::new(707, 0, 707)).pitch_deg, 45);
    }

    /// Pitch is measured against the other two axes combined, so rolling the board does not
    /// disturb its pitch. Measuring pitch against Z alone would make these disagree — the
    /// bug this test exists to catch.
    #[test]
    fn rolling_the_board_does_not_change_its_pitch() {
        // The same 30° pitch (500 mg on X), with the remaining 866 mg split three ways.
        let unrolled: Attitude = attitude_of(Acceleration::new(500, 0, 866));
        let rolled: Attitude = attitude_of(Acceleration::new(500, 866, 0));
        let half_rolled: Attitude = attitude_of(Acceleration::new(500, 612, 612));
        assert_eq!(unrolled.pitch_deg, 30);
        assert_eq!(rolled.pitch_deg, 30);
        assert_eq!(half_rolled.pitch_deg, 30);
    }

    /// Rounding is symmetric about zero: a small negative tilt must not round to the same
    /// degree as the equal positive one, which truncation toward zero would do.
    #[test]
    fn rounding_is_symmetric_for_both_signs() {
        // ~+-0.63 degrees of roll: 11 mg against 1000 mg.
        assert_eq!(attitude_of(Acceleration::new(0, 11, ONE_G_MG)).roll_deg, 1);
        assert_eq!(
            attitude_of(Acceleration::new(0, -11, ONE_G_MG)).roll_deg,
            -1
        );
    }

    proptest! {
        /// Whatever the reading, both angles stay inside the ranges the type documents — no
        /// caller has to defend against a 400-degree pitch, and the display's field widths
        /// are sound for every input the sensor can produce.
        #[test]
        fn every_reading_yields_angles_inside_their_documented_ranges(
            x_mg in -32_000i32..=32_000,
            y_mg in -32_000i32..=32_000,
            z_mg in -32_000i32..=32_000,
        ) {
            let attitude: Attitude = attitude_of(Acceleration::new(x_mg, y_mg, z_mg));
            prop_assert!(attitude.pitch_deg.abs() <= RIGHT_ANGLE_DEG);
            prop_assert!(attitude.roll_deg.abs() <= STRAIGHT_ANGLE_DEG);
        }

        /// Scaling the whole vector is a change of units, not of direction, so the attitude
        /// it implies is unchanged. This is what makes the readout independent of the
        /// accelerometer's configured full-scale range.
        #[test]
        fn scaling_the_vector_does_not_change_the_attitude(
            x_mg in -1_000i32..=1_000,
            y_mg in -1_000i32..=1_000,
            z_mg in -1_000i32..=1_000,
            scale in 2i32..=8,
        ) {
            // A near-zero vector's direction is numerically meaningless, so only ask this of
            // vectors with a real direction to preserve.
            prop_assume!(Acceleration::new(x_mg, y_mg, z_mg).magnitude_squared_mg2() > 10_000);
            let plain: Attitude = attitude_of(Acceleration::new(x_mg, y_mg, z_mg));
            let scaled: Attitude = attitude_of(Acceleration::new(
                x_mg * scale,
                y_mg * scale,
                z_mg * scale,
            ));
            // One degree of slack: the two vectors round independently, so a reading sitting
            // exactly on a half-degree boundary may land either side of it.
            prop_assert!((plain.pitch_deg - scaled.pitch_deg).abs() <= 1);
            prop_assert!((plain.roll_deg - scaled.roll_deg).abs() <= 1);
        }
    }
}
