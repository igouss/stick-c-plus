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

/// The [`REG_POWER_OUTPUT`] bits the on-board display needs: LDO3 (TFT panel),
/// LDO2 (TFT backlight), DCDC1 (the 3.3 V main rail) — `0b0000_1101`.
///
/// Deliberately excludes [`EXTEN`]: the display is lit by these three, and gating
/// the probe rail must not make a display self-check read as failed.
const DISPLAY_RAILS: u8 = 0x0D;

/// The [`REG_POWER_OUTPUT`] EXTEN bit (bit 6) — the board's external 5 V boost,
/// which is what feeds the **Grove port's 5 V pin** and hence the Earth Unit.
///
/// This is the only rail the probe cares about. The Earth Unit regulates it down
/// to 3.3 V locally (HT7533) and pulls its analog output up through a 10 kΩ
/// resistor, so *the electrodes are DC-biased whenever this bit is set* —
/// independently of whether the ESP32 ever samples the ADC. Cutting it is
/// therefore the only way to stop the electrolysis that corrodes the probe
/// (qhw.31); sampling less often does nothing.
const EXTEN: u8 = 0x40;

/// Every bit [`Axp192::power_on`] sets: the display rails plus EXTEN
/// (`0b0100_1101`). OR-ed into the register so the other rails' state is
/// preserved — this is the write that actually turns the LCD on.
const RAILS_ON: u8 = DISPLAY_RAILS | EXTEN;

/// The [`REG_POWER_OUTPUT`] LDO2 bit (bit 2) — the **TFT backlight**, and only the
/// backlight.
///
/// Deliberately separate from LDO3 (the panel itself), because that separation is
/// what makes a backlight toggle cheap: cutting this leaves the ST7789 powered and
/// holding its framebuffer, so the glass goes dark and comes back instantly with no
/// re-initialisation. Cutting LDO3 as well would save a little more and cost a full
/// panel bring-up on every wake.
const LDO2: u8 = 0x04;

/// Register 0x00 — the input-power status byte (which sources are present/usable).
const REG_POWER_STATUS: u8 = 0x00;

/// Register 0x46 — the PEK (power button) press-status latch.
///
/// The power button is not on a GPIO: it is wired to the PMIC, which does its own
/// debouncing and its own press-duration timing against the thresholds written to
/// `0x36` in [`Axp192::power_on`]. All the firmware can see is this latch, which
/// records *that* a press of a given kind happened since it was last cleared.
///
/// The factory `begin()` never enables the IRQ registers (`0x40`–`0x44`) and yet
/// `GetBtnPress` works, so these bits latch unconditionally — confirmed against the
/// pinned `kb/sources/m5stack-m5stickc-plus/src/AXP192.cpp`, not assumed.
const REG_PEK_STATUS: u8 = 0x46;

/// The [`REG_PEK_STATUS`] short-press bit (bit 1).
const PEK_SHORT_PRESS: u8 = 0x02;

/// The [`REG_PEK_STATUS`] long-press bit (bit 0).
///
/// Read only so that it can be *cleared*. A long press is not the firmware's to act
/// on: `0x36` sets the power-off press at four seconds, at which point the PMIC cuts
/// the rails and the ESP32 is gone. Leaving the bit set would make the next short
/// press read as stale.
const PEK_LONG_PRESS: u8 = 0x01;

/// Every [`REG_PEK_STATUS`] bit, written back to clear the latch (write-1-to-clear).
const PEK_ALL: u8 = PEK_SHORT_PRESS | PEK_LONG_PRESS;

/// The [`REG_POWER_STATUS`] VBUS-present bit (bit 5) — set while USB 5 V (VBUS) is
/// present at the PMIC, so it tracks the USB cable being plugged in or pulled out,
/// independent of whether the board draws charge from it.
const VBUS_PRESENT: u8 = 0x20;

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

    /// Whether every display rail ([`DISPLAY_RAILS`]) reads back as enabled.
    ///
    /// Does not consider [`EXTEN`], which [`set_exten`](Self::set_exten) toggles at
    /// runtime: a probe powered down between samples is the healthy steady state,
    /// not a failed bring-up.
    pub fn display_rails_enabled(&mut self) -> Result<bool, I2C::Error> {
        Ok(self.power_output()? & DISPLAY_RAILS == DISPLAY_RAILS)
    }

    /// Switch the external 5 V boost ([`EXTEN`]) — the Grove port's 5 V pin — on
    /// or off, preserving every other rail.
    ///
    /// A read-modify-write, so it can never disturb DCDC1 (the main 3.3 V rail the
    /// ESP32 itself runs on) or the display's LDO2/LDO3. This is the mechanism
    /// behind the probe's [`ProbePower`](firmware_core::ProbePower) gating — see
    /// [`EXTEN`] for why the probe corrodes without it.
    ///
    /// Returns once the register write is acknowledged. The rail itself takes time
    /// to rise and (much longer) to bleed down through the probe's reservoir caps;
    /// a caller that intends to *read* the probe must settle afterwards rather than
    /// assume this call is sufficient.
    pub fn set_exten(&mut self, on: bool) -> Result<(), I2C::Error> {
        let power: u8 = self.read(REG_POWER_OUTPUT)?;
        let next: u8 = if on { power | EXTEN } else { power & !EXTEN };
        self.write(REG_POWER_OUTPUT, next)
    }

    /// Light the TFT backlight ([`LDO2`]) or cut it, preserving every other rail.
    ///
    /// A read-modify-write, like [`set_exten`](Self::set_exten), so it can never disturb
    /// DCDC1 (the rail the ESP32 runs on) or LDO3 (the panel). Cutting only the
    /// backlight is what makes the toggle instant in both directions — see [`LDO2`].
    pub fn set_backlight(&mut self, lit: bool) -> Result<(), I2C::Error> {
        let power: u8 = self.read(REG_POWER_OUTPUT)?;
        let next: u8 = if lit { power | LDO2 } else { power & !LDO2 };
        self.write(REG_POWER_OUTPUT, next)
    }

    /// Drain the power button's press latch ([`REG_PEK_STATUS`]): was there a short press
    /// since the last call?
    ///
    /// Destructive, and deliberately so — the latch records *that* a press happened, not
    /// how many, so a press is reported exactly once and a caller must poll on a steady
    /// cadence or presses will queue behind one another.
    ///
    /// Clears the long-press bit too without reporting it. A long press ends with the PMIC
    /// cutting the rails, so there is nothing to act on; leaving the bit set would only make
    /// the next short press read as stale. The register is write-1-to-clear, and it is
    /// written only when something was actually latched, so a quiet poll costs one read.
    pub fn take_power_button_press(&mut self) -> Result<bool, I2C::Error> {
        let status: u8 = self.read(REG_PEK_STATUS)?;
        if status & PEK_ALL != 0 {
            self.write(REG_PEK_STATUS, PEK_ALL)?;
        }
        Ok(status & PEK_SHORT_PRESS == PEK_SHORT_PRESS)
    }

    /// Whether the external 5 V boost ([`EXTEN`]) reads back as enabled.
    pub fn exten_enabled(&mut self) -> Result<bool, I2C::Error> {
        Ok(self.power_output()? & EXTEN == EXTEN)
    }

    /// Whether USB power (VBUS) is present, from [`REG_POWER_STATUS`] bit 5.
    ///
    /// The board primitive behind the platform power-source port: a plain status-bit read,
    /// no interpretation. All debounce and the plug/unplug edge decision live inward in the
    /// pure `platform-core`; this only reports the raw level the PMIC sees, so a USB
    /// plug or pull shows up here as this bit going high or low.
    pub fn vbus_present(&mut self) -> Result<bool, I2C::Error> {
        Ok(self.read(REG_POWER_STATUS)? & VBUS_PRESENT == VBUS_PRESENT)
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
