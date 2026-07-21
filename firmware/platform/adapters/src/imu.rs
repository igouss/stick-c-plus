//! The MPU6886 IMU as the platform [`Imu`] port: which way is the board being pulled?

use board_support::{Mpu6886, Mpu6886Error};
use embedded_hal::i2c::I2c;
use platform_core::{Acceleration, Imu};

/// The M5StickC Plus MPU6886 IMU as the [`Imu`](platform_core::Imu) port.
///
/// Wraps a brought-up [`Mpu6886`] and answers [`acceleration`](Imu::acceleration) with one
/// burst read, already converted to milli-g at the driver's configured full-scale range. The
/// tilt arithmetic and the naming of poses live inward in `orientation-core`; this adapter
/// only reports the vector and owns the bus error type, so the port itself names no hardware.
///
/// Generic over the I2C bus, so the composition root decides ownership: on the M5StickC Plus
/// the IMU and the AXP192 PMIC share the one internal bus, so the root hands each an
/// `embedded-hal-bus` device over a single controller. Either way the sampler sees only
/// `impl Imu + Send`.
pub struct Mpu6886Imu<I2C> {
    imu: Mpu6886<I2C>,
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
        let [x_mg, y_mg, z_mg]: [i32; 3] = self.imu.acceleration_milli_g()?;
        Ok(Acceleration::new(x_mg, y_mg, z_mg))
    }
}
