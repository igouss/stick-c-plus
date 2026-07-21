//! The MPU6886 IMU as the platform [`Imu`] port: which way is the board being pulled?

use board_support::{Mpu6886, Mpu6886Error};
use embedded_hal::i2c::I2c;
use platform_core::{Acceleration, Imu};

/// The M5StickC Plus MPU6886 IMU as the [`Imu`](platform_core::Imu) port.
///
/// Wraps a brought-up [`Mpu6886`] and answers [`acceleration`](Imu::acceleration) with one
/// burst read, converted to milli-g at the driver's configured full-scale range and rotated
/// into the board frame. The tilt arithmetic and the naming of poses live inward in
/// `orientation-core`; this adapter reports the vector, owns the bus error type, and owns the
/// one thing neither of those can know — how the part is soldered down.
///
/// Generic over the I2C bus, so the composition root decides ownership: on the M5StickC Plus
/// the IMU and the AXP192 PMIC share the one internal bus, so the root hands each an
/// `embedded-hal-bus` device over a single controller. Either way the sampler sees only
/// `impl Imu + Send`.
pub struct Mpu6886Imu<I2C> {
    imu: Mpu6886<I2C>,
}

/// The board-frame acceleration a chip-frame reading means.
///
/// The MPU6886 is soldered a quarter turn about Z from the board's own frame, so its axes are
/// not the ones a pose should ever be named from:
///
/// | board axis                              | chip axis |
/// |-----------------------------------------|-----------|
/// | **+X** — toward the top (away from USB-C) | `+y`      |
/// | **+Y** — out of the left edge             | `−x`      |
/// | **+Z** — out of the screen                | `+z`      |
///
/// A proper rotation, not a relabelling: it preserves handedness, so the tilt angles derived
/// downstream keep their signs. Measured on the metal by resting the board on all six faces
/// and reading the vector against the pose it was actually in — the only way this can be
/// established, since every rotation of a soldered part produces equally plausible numbers and
/// no host test can tell them apart:
///
/// | pose            | chip reading   | board reading  |
/// |-----------------|----------------|----------------|
/// | screen up       | `z = +1050`    | `z = +1050`    |
/// | screen down     | `z = −959`     | `z = −959`     |
/// | on the USB port | `y = +1010`    | `x = +1010`    |
/// | on the top edge | `y = −985`     | `x = −985`     |
/// | on the left edge| `x = +988`     | `y = −988`     |
/// | on the right edge| `x = −1011`   | `y = +1011`    |
const fn into_board_frame(chip: [i32; 3]) -> Acceleration {
    let [x_mg, y_mg, z_mg]: [i32; 3] = chip;
    Acceleration::new(y_mg, -x_mg, z_mg)
}

impl<I2C: I2c> Mpu6886Imu<I2C> {
    /// Wrap a brought-up [`Mpu6886`] as an [`Imu`]. No I/O until it is polled.
    pub const fn new(imu: Mpu6886<I2C>) -> Self {
        Mpu6886Imu { imu }
    }
}

impl<I2C: I2c> Imu for Mpu6886Imu<I2C> {
    type Error = Mpu6886Error<I2C::Error>;

    fn acceleration(&mut self) -> Result<Acceleration, Self::Error> {
        Ok(into_board_frame(self.imu.acceleration_milli_g()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::ONE_G_MG;

    /// Zero: a weightless reading rotates to a weightless reading — the rotation adds nothing
    /// of its own.
    #[test]
    fn a_zero_reading_stays_zero() {
        assert_eq!(into_board_frame([0, 0, 0]), Acceleration::default());
    }

    /// One: the pose the board sits in on the desk. Z is the axis the mounting leaves alone,
    /// and it is the one face that read correctly before this rotation existed.
    #[test]
    fn a_board_on_its_back_reads_one_gravity_out_of_the_screen() {
        assert_eq!(
            into_board_frame([0, 0, ONE_G_MG]),
            Acceleration::new(0, 0, ONE_G_MG)
        );
    }

    /// Many: all six faces, exactly as measured on the board. These numbers are evidence, not
    /// a restatement of the implementation — each row was read off the serial heartbeat with
    /// the board physically in that pose.
    #[test]
    fn every_measured_face_rotates_to_the_axis_that_names_it() {
        // Standing on the USB-C port: the top of the stick is up, so board +X carries it.
        assert_eq!(
            into_board_frame([-7, 1010, 5]),
            Acceleration::new(1010, 7, 5)
        );
        // Standing on the top edge, upside down.
        assert_eq!(
            into_board_frame([-30, -984, 92]),
            Acceleration::new(-984, 30, 92)
        );
        // Resting on the left edge: board +Y points out of that edge, which is now down.
        assert_eq!(
            into_board_frame([988, -3, 85]),
            Acceleration::new(-3, -988, 85)
        );
        // Resting on the right edge.
        assert_eq!(
            into_board_frame([-1011, 24, 97]),
            Acceleration::new(24, 1011, 97)
        );
        // Face down.
        assert_eq!(
            into_board_frame([-15, 5, -959]),
            Acceleration::new(5, 15, -959)
        );
    }

    /// The rotation preserves magnitude — it turns the vector, it does not scale it. A
    /// "rotation" that failed this would quietly break the at-rest test every pose depends on.
    #[test]
    fn the_rotation_preserves_magnitude() {
        let chip: [i32; 3] = [300, -400, 500];
        assert_eq!(
            into_board_frame(chip).magnitude_squared_mg2(),
            Acceleration::new(chip[0], chip[1], chip[2]).magnitude_squared_mg2()
        );
    }

    /// Four applications return to the start: this is a quarter turn about Z, so the rotation
    /// has order four. Catches a sign slip that a single-axis check would let through.
    #[test]
    fn four_quarter_turns_are_the_identity() {
        let chip: [i32; 3] = [123, -456, 789];
        let once: Acceleration = into_board_frame(chip);
        let twice: Acceleration = into_board_frame([once.x_mg, once.y_mg, once.z_mg]);
        let thrice: Acceleration = into_board_frame([twice.x_mg, twice.y_mg, twice.z_mg]);
        let full: Acceleration = into_board_frame([thrice.x_mg, thrice.y_mg, thrice.z_mg]);
        assert_eq!(full, Acceleration::new(chip[0], chip[1], chip[2]));
    }

    /// Z is untouched by a rotation about Z, whatever the other axes are doing.
    #[test]
    fn the_screen_normal_is_left_alone() {
        assert_eq!(into_board_frame([11, 22, 33]).z_mg, 33);
        assert_eq!(into_board_frame([-11, -22, -33]).z_mg, -33);
    }
}
