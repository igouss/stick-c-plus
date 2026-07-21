//! `mpu6886` — a thin M5StickC Plus MPU6886 IMU driver: read the accelerometer.
//!
//! The MPU6886 six-axis IMU sits on the internal I2C bus (SDA G21 / SCL G22, address
//! [`ADDRESS`]) beside the AXP192 PMIC and the BM8563 RTC. It is the part that knows which
//! way the board is pointing.
//!
//! No `embedded-hal` 1.0 driver crate exists for it — the published `mpu6886` crate is
//! `embedded-hal` 0.2 — and this project does not bridge old HALs with compatibility shims
//! (see `kb/guides/rust-driver-crates.md`), so we own a thin register-map driver instead. It
//! is generic over any [`embedded_hal::i2c::I2c`], so it names no concrete bus and the one
//! internal bus can be *shared* (via `embedded-hal-bus` at the composition root) with the
//! PMIC that sits on the same two pins.
//!
//! [`Mpu6886::init`] performs the register writes the M5 **factory** firmware does at boot,
//! in the factory order (ported from the pinned
//! `kb/sources/m5stack-m5stickc-plus/src/utility/MPU6886.cpp` `Init()`), so bring-up is the
//! sequence the shipped board is known-good with. Two deliberate departures are marked at
//! their write sites: the sample-rate divider and the accelerometer's full-scale range, both
//! of which are application choices rather than bring-up incantations.
//!
//! ## Accelerometer only
//!
//! The gyroscope is configured exactly as the factory does but never read. An orientation
//! readout wants the *gravity* vector, which the accelerometer measures directly; integrating
//! a gyro's angular rate would add a heading that drifts away from the truth with no way to
//! correct it, since this board has no magnetometer to correct it against. Reading one honest
//! vector beats fusing in a quantity nothing can keep true.

use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c;

/// The MPU6886's 7-bit I2C address on the M5StickC Plus internal bus.
pub const ADDRESS: u8 = 0x68;

/// Register 0x75 — the device identity.
const REG_WHO_AM_I: u8 = 0x75;
/// The value [`REG_WHO_AM_I`] holds on a genuine MPU6886.
const WHO_AM_I_MPU6886: u8 = 0x19;

/// Register 0x6B — power management: sleep, reset, and the clock source.
const REG_PWR_MGMT_1: u8 = 0x6B;
/// [`REG_PWR_MGMT_1`]: clear every bit — wake from sleep.
const PWR_WAKE: u8 = 0x00;
/// [`REG_PWR_MGMT_1`] bit 7 — a full device reset.
const PWR_DEVICE_RESET: u8 = 0x80;
/// [`REG_PWR_MGMT_1`]: run from the best available clock (the PLL) rather than the
/// less-stable internal oscillator.
const PWR_CLOCK_AUTO: u8 = 0x01;

/// Register 0x1C — the accelerometer's full-scale range.
const REG_ACCEL_CONFIG: u8 = 0x1C;
/// Register 0x1D — the accelerometer's own filter and rate path.
const REG_ACCEL_CONFIG2: u8 = 0x1D;
/// Register 0x1B — the gyroscope's full-scale range.
const REG_GYRO_CONFIG: u8 = 0x1B;
/// [`REG_GYRO_CONFIG`]: ±2000 °/s, the factory value. Configured, never read — see the
/// module docs.
const GYRO_FULL_SCALE_2000DPS: u8 = 0x18;

/// Register 0x1A — the digital low-pass filter configuration.
const REG_CONFIG: u8 = 0x1A;
/// [`REG_CONFIG`]: DLPF setting 1, the factory value — a gentle filter that takes the
/// highest-frequency noise off the samples in the sensor rather than in our code.
const CONFIG_DLPF: u8 = 0x01;

/// Register 0x19 — the sample-rate divider.
const REG_SMPLRT_DIV: u8 = 0x19;
/// [`REG_SMPLRT_DIV`]: no division, so the sensor updates at its full 1 kHz.
///
/// **A deliberate departure from the factory sequence**, which writes `0x05` for a 166 Hz
/// update. The readout polls at 200 Hz precisely so that a movement reaches the glass
/// promptly; against a 166 Hz sensor a third of those polls would re-read a sample already
/// seen, adding lag for nothing. At 1 kHz every poll gets a genuinely fresh reading.
const SMPLRT_DIV_NONE: u8 = 0x00;

/// Register 0x38 — which events raise the interrupt pin.
const REG_INT_ENABLE: u8 = 0x38;
/// Register 0x37 — how the interrupt pin behaves.
const REG_INT_PIN_CFG: u8 = 0x37;
/// Register 0x6A — FIFO/I2C-master control.
const REG_USER_CTRL: u8 = 0x6A;
/// Register 0x23 — which sensors feed the FIFO.
const REG_FIFO_EN: u8 = 0x23;

/// Register 0x3B — the first of the six accelerometer output bytes
/// (`XOUT_H, XOUT_L, YOUT_H, YOUT_L, ZOUT_H, ZOUT_L`), read as one burst.
const REG_ACCEL_XOUT_H: u8 = 0x3B;

/// A count of milli-g per raw LSB, expressed as a fraction to keep the conversion in
/// integers: `1000 mg/g * range_g / 32768 counts` reduces to `125 / (2048 >> range_index)`.
const MILLI_G_NUMERATOR: i32 = 125;
/// The denominator at the narrowest (±2 g) range; each wider range halves it.
const MILLI_G_DENOMINATOR_2G: i32 = 2_048;

/// How long the device needs after a reset or a wake before it will answer, in milliseconds.
const SETTLE_MS: u32 = 10;
/// How long the sensor needs after configuration before its outputs are trustworthy.
const STARTUP_MS: u32 = 100;

/// The accelerometer's full-scale range.
///
/// A range is a trade: narrower means finer resolution per count, wider means a hard knock
/// reads its true size instead of clipping.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccelRange {
    /// ±2 g — the finest resolution, but a brisk shake clips.
    G2,
    /// ±4 g — twice the tilt resolution of the factory setting, with room for ordinary
    /// handling. What the orientation readout uses: a board being turned by hand rarely
    /// passes 2 g, and the extra headroom means a knock is reported rather than pinned.
    G4,
    /// ±8 g — the M5 factory firmware's choice.
    G8,
    /// ±16 g — the widest, and the coarsest.
    G16,
}

impl AccelRange {
    /// The [`REG_ACCEL_CONFIG`] bits selecting this range (bits 4:3).
    const fn config_bits(self) -> u8 {
        (self.index() as u8) << 3
    }

    /// This range's position in the doubling sequence: ±2 g is 0, ±16 g is 3.
    const fn index(self) -> i32 {
        match self {
            AccelRange::G2 => 0,
            AccelRange::G4 => 1,
            AccelRange::G8 => 2,
            AccelRange::G16 => 3,
        }
    }

    /// Convert one raw axis count to milli-g at this range.
    ///
    /// Exact integer arithmetic: `raw * 125 / (2048 >> index)`. The widest intermediate is
    /// `32767 * 125`, about 4.1 million, so an `i32` holds it with three orders of magnitude
    /// to spare — no float, and no rounding to reason about beyond the single truncating
    /// divide, which is well under the sensor's own noise floor.
    pub const fn to_milli_g(self, raw: i16) -> i32 {
        let denominator: i32 = MILLI_G_DENOMINATOR_2G >> self.index();
        (raw as i32) * MILLI_G_NUMERATOR / denominator
    }
}

/// The M5StickC Plus MPU6886 IMU, over a shared or owned I2C bus.
pub struct Mpu6886<I2C> {
    i2c: I2C,
    range: AccelRange,
}

/// A bring-up failure: the bus refused, or the part on the bus is not an MPU6886.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mpu6886Error<E> {
    /// The I2C bus rejected a transfer.
    Bus(E),
    /// `WHO_AM_I` answered, but with something other than an MPU6886's identity. Carries what
    /// was actually read, because "the wrong chip" and "a floating bus reading `0x00`" are
    /// different faults and the byte tells them apart.
    NotAnMpu6886(u8),
}

impl<E: core::fmt::Display> core::fmt::Display for Mpu6886Error<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Mpu6886Error::Bus(err) => write!(f, "the I2C bus rejected a transfer: {err}"),
            Mpu6886Error::NotAnMpu6886(who) => {
                write!(f, "WHO_AM_I answered {who:#04x}, not an MPU6886's 0x19")
            }
        }
    }
}

impl<I2C: I2c> Mpu6886<I2C> {
    /// Bind the IMU to `i2c` (a bus device addressing [`ADDRESS`]) at the given range. No I/O
    /// yet — call [`init`](Self::init) to bring it up.
    pub const fn new(i2c: I2C, range: AccelRange) -> Self {
        Mpu6886 { i2c, range }
    }

    /// The full-scale range this driver was configured for.
    pub const fn range(&self) -> AccelRange {
        self.range
    }

    /// Bring the IMU up, in the M5 factory order.
    ///
    /// Identity is checked **first**: an unpowered or mis-wired bus answers reads with a
    /// plausible-looking `0x00` or `0xFF`, and configuring that and then reporting orientation
    /// from it would produce a readout that looks alive and means nothing. So the part proves
    /// it is an MPU6886 before a single configuration byte is written.
    ///
    /// The reset-then-wake dance is the factory's: clear `PWR_MGMT_1`, pulse the reset bit,
    /// then select the PLL clock, with a settling delay after each — a reset that is not
    /// waited out leaves the following writes landing on a device still rebooting.
    pub fn init<D: DelayNs>(&mut self, delay: &mut D) -> Result<(), Mpu6886Error<I2C::Error>> {
        let who: u8 = self.read(REG_WHO_AM_I)?;
        if who != WHO_AM_I_MPU6886 {
            return Err(Mpu6886Error::NotAnMpu6886(who));
        }

        self.write(REG_PWR_MGMT_1, PWR_WAKE)?;
        delay.delay_ms(SETTLE_MS);
        self.write(REG_PWR_MGMT_1, PWR_DEVICE_RESET)?;
        delay.delay_ms(SETTLE_MS);
        self.write(REG_PWR_MGMT_1, PWR_CLOCK_AUTO)?;
        delay.delay_ms(SETTLE_MS);

        // A departure from the factory's fixed ±8 g — see `AccelRange::G4`.
        self.write(REG_ACCEL_CONFIG, self.range.config_bits())?;
        self.write(REG_GYRO_CONFIG, GYRO_FULL_SCALE_2000DPS)?;
        self.write(REG_CONFIG, CONFIG_DLPF)?;
        // A departure from the factory's 166 Hz — see `SMPLRT_DIV_NONE`.
        self.write(REG_SMPLRT_DIV, SMPLRT_DIV_NONE)?;
        self.write(REG_INT_ENABLE, 0x00)?;
        self.write(REG_ACCEL_CONFIG2, 0x00)?;
        self.write(REG_USER_CTRL, 0x00)?;
        self.write(REG_FIFO_EN, 0x00)?;
        self.write(REG_INT_PIN_CFG, 0x22)?;
        self.write(REG_INT_ENABLE, 0x01)?;

        delay.delay_ms(STARTUP_MS);
        Ok(())
    }

    /// Read the three raw accelerometer counts, as one six-byte burst.
    ///
    /// One burst, not three register pairs: the six output registers are latched together, so
    /// reading them in a single transaction guarantees all three axes come from the *same*
    /// sample. Three separate reads could straddle a sensor update and return a vector that
    /// no single instant ever produced — which, on a readout whose whole job is to show a
    /// direction, would show a direction the board was never pointing.
    pub fn acceleration_raw(&mut self) -> Result<[i16; 3], Mpu6886Error<I2C::Error>> {
        let mut buf: [u8; 6] = [0; 6];
        self.i2c
            .write_read(ADDRESS, &[REG_ACCEL_XOUT_H], &mut buf)
            .map_err(Mpu6886Error::Bus)?;
        Ok([
            i16::from_be_bytes([buf[0], buf[1]]),
            i16::from_be_bytes([buf[2], buf[3]]),
            i16::from_be_bytes([buf[4], buf[5]]),
        ])
    }

    /// Read the three accelerometer axes, converted to milli-g at the configured range.
    pub fn acceleration_milli_g(&mut self) -> Result<[i32; 3], Mpu6886Error<I2C::Error>> {
        let raw: [i16; 3] = self.acceleration_raw()?;
        Ok([
            self.range.to_milli_g(raw[0]),
            self.range.to_milli_g(raw[1]),
            self.range.to_milli_g(raw[2]),
        ])
    }

    /// Read one register: write its address, read one byte back.
    fn read(&mut self, reg: u8) -> Result<u8, Mpu6886Error<I2C::Error>> {
        let mut buf: [u8; 1] = [0];
        self.i2c
            .write_read(ADDRESS, &[reg], &mut buf)
            .map_err(Mpu6886Error::Bus)?;
        Ok(buf[0])
    }

    /// Write one byte to one register.
    fn write(&mut self, reg: u8, val: u8) -> Result<(), Mpu6886Error<I2C::Error>> {
        self.i2c
            .write(ADDRESS, &[reg, val])
            .map_err(Mpu6886Error::Bus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zero: a zero count is zero milli-g at every range.
    #[test]
    fn a_zero_count_is_zero_milli_g_at_every_range() {
        [
            AccelRange::G2,
            AccelRange::G4,
            AccelRange::G8,
            AccelRange::G16,
        ]
        .iter()
        .for_each(|range: &AccelRange| assert_eq!(range.to_milli_g(0), 0));
    }

    /// One: at ±4 g, one gravity is 8192 counts — the conversion's anchor point, and the one
    /// number the whole readout's calibration rests on.
    #[test]
    fn one_gravity_converts_to_one_thousand_milli_g() {
        assert_eq!(AccelRange::G4.to_milli_g(8_192), 1_000);
        assert_eq!(AccelRange::G4.to_milli_g(-8_192), -1_000);
    }

    /// Many: each wider range doubles the milli-g a given count represents.
    #[test]
    fn each_wider_range_doubles_the_milli_g_per_count() {
        let count: i16 = 4_096;
        assert_eq!(AccelRange::G2.to_milli_g(count), 250);
        assert_eq!(AccelRange::G4.to_milli_g(count), 500);
        assert_eq!(AccelRange::G8.to_milli_g(count), 1_000);
        assert_eq!(AccelRange::G16.to_milli_g(count), 2_000);
    }

    /// The extremes convert without overflowing the intermediate multiply.
    #[test]
    fn the_full_scale_extremes_convert_without_overflow() {
        assert_eq!(AccelRange::G4.to_milli_g(i16::MAX), 3_999);
        assert_eq!(AccelRange::G4.to_milli_g(i16::MIN), -4_000);
        assert_eq!(AccelRange::G16.to_milli_g(i16::MAX), 15_999);
    }

    /// The config bits select the range the datasheet says they do, in bits 4:3 and nowhere
    /// else — a stray bit here would silently rescale every reading.
    #[test]
    fn the_config_bits_land_in_the_range_field() {
        assert_eq!(AccelRange::G2.config_bits(), 0x00);
        assert_eq!(AccelRange::G4.config_bits(), 0x08);
        // The factory firmware's ±8 g is 0x10 — the value the vendored `MPU6886.cpp` writes,
        // so this pins our encoding against the known-good reference.
        assert_eq!(AccelRange::G8.config_bits(), 0x10);
        assert_eq!(AccelRange::G16.config_bits(), 0x18);
    }

    /// The conversion is odd-symmetric about zero to within a count, so a board tilted one
    /// way reads the mirror of the same tilt the other way.
    #[test]
    fn the_conversion_is_symmetric_about_zero() {
        [1i16, 100, 4_096, 8_192, 32_000]
            .iter()
            .for_each(|count: &i16| {
                let positive: i32 = AccelRange::G4.to_milli_g(*count);
                let negative: i32 = AccelRange::G4.to_milli_g(-*count);
                assert_eq!(positive, -negative, "asymmetric at {count} counts");
            });
    }
}
