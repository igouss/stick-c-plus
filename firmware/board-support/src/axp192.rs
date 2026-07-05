//! `axp192` — a thin M5StickC Plus AXP192 PMIC driver: power on the LCD/TFT rails.
//!
//! The AXP192 sits on the internal I2C bus (SDA G21 / SCL G22, address
//! [`ADDRESS`]) and powers the rails the board cannot run without — most
//! immediately **LDO2** (the TFT backlight) and **LDO3** (the TFT panel). Until it
//! is brought up the display stays dark, no matter how correct the ST7789 init is.
//!
//! [`Axp192::power_on`] performs exactly the register writes the M5 **factory**
//! firmware does at boot, in the factory order (ported from the pinned
//! `kb/sources/m5stack-m5stickc-plus/src/AXP192.cpp` `begin()`), so our bring-up is
//! byte-for-byte the sequence the shipped board is known-good with. The driver is
//! generic over any [`embedded_hal::i2c::I2c`], so it names no concrete bus and the
//! one internal bus can be *shared* (via `embedded-hal-bus` at the composition
//! root) with the MPU6886 IMU and BM8563 RTC that sit on the same two pins.

use embedded_hal::i2c::I2c;

/// The AXP192's 7-bit I2C address on the M5StickC Plus internal bus.
pub const ADDRESS: u8 = 0x34;

/// Register 0x12 — the power-output enable byte (which rails are on).
const REG_POWER_OUTPUT: u8 = 0x12;

/// The bits of [`REG_POWER_OUTPUT`] the display rails need: EXTEN, LDO3, LDO2,
/// DCDC1 (`0b0100_1101`). OR-ed into the register so the other rails' state is
/// preserved — this is the write that actually turns the LCD on.
const RAILS_ON: u8 = 0x4D;

/// The M5StickC Plus AXP192 PMIC, over a shared or owned I2C bus.
pub struct Axp192<I2C> {
    i2c: I2C,
}

impl<I2C: I2c> Axp192<I2C> {
    /// Bind the PMIC to `i2c` (a bus device addressing [`ADDRESS`]). No I/O yet —
    /// call [`power_on`](Self::power_on) to bring the rails up.
    pub const fn new(i2c: I2C) -> Self {
        Self { i2c }
    }

    /// Power the board's rails in the M5 factory order.
    ///
    /// The eleven writes below are the factory `AXP192::begin()` sequence verbatim.
    /// The load-bearing ones for the display are the first (LDO2/LDO3 voltage) and
    /// the read-modify-write of [`REG_POWER_OUTPUT`] (which enables the rails); the
    /// rest set charging, the power button, GPIO0, and battery detection to the
    /// same values the shipped firmware uses, so the board comes up identically.
    pub fn power_on(&mut self) -> Result<(), I2C::Error> {
        // LDO2 (TFT backlight) & LDO3 (TFT panel) = 3.0 V — high/low nibble.
        self.write(0x28, 0xCC)?;
        // ADC enable, all channels.
        self.write(0x82, 0xFF)?;
        // Battery charge: 4.2 V target, 100 mA.
        self.write(0x33, 0xC0)?;
        // Enable EXTEN + LDO3 + LDO2 + DCDC1 — read-modify-write so other rails
        // are preserved. This is the write that lights the LCD.
        let power: u8 = self.read(REG_POWER_OUTPUT)?;
        self.write(REG_POWER_OUTPUT, power | RAILS_ON)?;
        // PEK: 128 ms power-on press, 4 s power-off press.
        self.write(0x36, 0x0C)?;
        // GPIO0 (LDOio0) output = 3.3 V.
        self.write(0x91, 0xF0)?;
        // GPIO0 pin function = LDO output mode.
        self.write(0x90, 0x02)?;
        // VBUS-IPSOUT path: N_VBUSEN / VBUS hold-current limit.
        self.write(0x30, 0x80)?;
        // Battery over-temperature protection threshold.
        self.write(0x39, 0xFC)?;
        // Enable RTC backup-battery (button-cell) charge.
        self.write(0x35, 0xA2)?;
        // Enable battery detection.
        self.write(0x32, 0x46)?;
        Ok(())
    }

    /// Read back the power-output enable byte ([`REG_POWER_OUTPUT`]).
    ///
    /// A boot-time self-check: after [`power_on`](Self::power_on) the composition
    /// root can confirm `output & `[`RAILS_ON`]` == `[`RAILS_ON`] over the wire,
    /// proving the PMIC acknowledged the enable rather than trusting a blind write.
    pub fn power_output(&mut self) -> Result<u8, I2C::Error> {
        self.read(REG_POWER_OUTPUT)
    }

    /// Whether every display rail ([`RAILS_ON`]) reads back as enabled.
    pub fn rails_enabled(&mut self) -> Result<bool, I2C::Error> {
        Ok(self.power_output()? & RAILS_ON == RAILS_ON)
    }

    /// Read one register: write its address, read one byte back.
    fn read(&mut self, reg: u8) -> Result<u8, I2C::Error> {
        let mut buf: [u8; 1] = [0];
        self.i2c.write_read(ADDRESS, &[reg], &mut buf)?;
        Ok(buf[0])
    }

    /// Write one byte to one register.
    fn write(&mut self, reg: u8, val: u8) -> Result<(), I2C::Error> {
        self.i2c.write(ADDRESS, &[reg, val])
    }
}
