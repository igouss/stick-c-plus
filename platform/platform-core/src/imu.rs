//! The IMU port: the proper acceleration the board feels, three axes at a time.

/// One standard gravity, in milli-g — the scale every axis is measured against.
///
/// A board lying still feels exactly `1 g` of proper acceleration, pointing *away* from the
/// centre of the earth along whichever axis happens to be up. That is the whole reason an
/// accelerometer can report an orientation at all: at rest, the acceleration vector *is* the
/// up vector.
pub const ONE_G_MG: i32 = 1_000;

/// A three-axis proper acceleration, in milli-g.
///
/// Milli-g as `i32`, not a float: the port stays exact, `Copy`, allocation-free and
/// `no_std`-friendly, and the ±16 g an MPU6886 can be configured for is four orders of
/// magnitude inside an `i32`. The axes are the *sensor's own*, as the board mounts it —
/// deciding which one points at the sky, and what to call that, is the app's job, not the
/// port's.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Acceleration {
    /// Acceleration along the sensor's X axis, in milli-g.
    pub x_mg: i32,
    /// Acceleration along the sensor's Y axis, in milli-g.
    pub y_mg: i32,
    /// Acceleration along the sensor's Z axis, in milli-g.
    pub z_mg: i32,
}

impl Acceleration {
    /// An acceleration of `x_mg`, `y_mg`, `z_mg` milli-g.
    pub const fn new(x_mg: i32, y_mg: i32, z_mg: i32) -> Self {
        Acceleration { x_mg, y_mg, z_mg }
    }

    /// The vector's squared magnitude, in milli-g².
    ///
    /// Squared, and `i64`, so the answer is exact: a real square root would need a float and
    /// `libm` here in the kernel, and every question this crate's callers ask — "is this
    /// close to 1 g?", "which axis dominates?" — is answered as well by comparing squares.
    ///
    /// One squared `i32` always fits an `i64`; their *sum* does not, so it saturates. Real
    /// readings are nowhere near it — a ±16 g full scale is 16 000 mg, and the saturation
    /// point is past 1.7 billion mg on every axis at once — so saturating here only bounds a
    /// regime in which the sensor has already stopped telling the truth. Better a pinned
    /// maximum than a wrapped magnitude that reads as *small*.
    pub const fn magnitude_squared_mg2(&self) -> i64 {
        let x: i64 = self.x_mg as i64;
        let y: i64 = self.y_mg as i64;
        let z: i64 = self.z_mg as i64;
        (x * x).saturating_add(y * y).saturating_add(z * z)
    }
}

/// A three-axis accelerometer.
///
/// The driven port for "which way is the board being pulled?" — sibling to
/// [`PowerSource`](crate::PowerSource) and [`Button`](crate::Button). The adapter owns its own
/// failure type and every register, address and bus detail; this port names no hardware.
pub trait Imu {
    /// The adapter's own read-failure type.
    type Error;

    /// The proper acceleration the board feels right now.
    fn acceleration(&mut self) -> Result<Acceleration, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zero: a free-falling (or unpowered) sensor reads nothing on every axis.
    #[test]
    fn a_zero_acceleration_has_zero_magnitude() {
        assert_eq!(Acceleration::default().magnitude_squared_mg2(), 0);
    }

    /// One: a board resting flat feels exactly one gravity, so its magnitude squared is
    /// `1000²` — the invariant every orientation reading is judged against.
    #[test]
    fn a_board_at_rest_reads_one_gravity() {
        let flat: Acceleration = Acceleration::new(0, 0, ONE_G_MG);
        assert_eq!(
            flat.magnitude_squared_mg2(),
            i64::from(ONE_G_MG) * i64::from(ONE_G_MG)
        );
    }

    /// Many: the magnitude sums all three axes, and is blind to their signs — a vector and
    /// its negation are the same length.
    #[test]
    fn the_magnitude_sums_every_axis_and_ignores_sign() {
        let tilted: Acceleration = Acceleration::new(300, -400, 500);
        let mirrored: Acceleration = Acceleration::new(-300, 400, -500);
        assert_eq!(tilted.magnitude_squared_mg2(), 90_000 + 160_000 + 250_000);
        assert_eq!(
            tilted.magnitude_squared_mg2(),
            mirrored.magnitude_squared_mg2()
        );
    }

    /// An absurd reading saturates rather than wrapping. Three `i32::MAX` axes overshoot an
    /// `i64` sum, and a wrapped magnitude would come back *negative* — a violently shaken
    /// board reading as weightless is exactly the convenient lie this project refuses.
    #[test]
    fn an_absurd_reading_saturates_instead_of_wrapping_negative() {
        let absurd: Acceleration = Acceleration::new(i32::MAX, i32::MAX, i32::MAX);
        assert_eq!(absurd.magnitude_squared_mg2(), i64::MAX);
    }

    /// The whole physical range is exact, not saturated: every axis pinned at the widest
    /// full scale the MPU6886 offers (±16 g) is still ordinary arithmetic.
    #[test]
    fn the_full_scale_range_is_computed_exactly() {
        let full_scale: i32 = 16 * ONE_G_MG;
        let pinned: Acceleration = Acceleration::new(full_scale, -full_scale, full_scale);
        let square: i64 = i64::from(full_scale) * i64::from(full_scale);
        assert_eq!(pinned.magnitude_squared_mg2(), 3 * square);
    }
}
