---
id: board-has-three-buttons-two-acquisition-paths
title: "The M5StickC Plus has three buttons, not two — and the third cannot be polled as a level"
confidence: high
scope: board:m5stickc-plus
derived-from: []
supersedes: []
reviewed: 2026-07-21
check: grep -q 'const REG_PEK_STATUS: u8 = 0x46' firmware/platform/board-support/src/axp192.rs && grep -q 'pub trait LatchedGesture' platform/platform-input/src/button.rs
---

**Claim:** The board has **three** user-pressable buttons, but only **two** appear in
any GPIO pin map, because the third is not on a GPIO:

| Button | Where | How firmware reads it |
|---|---|---|
| A / front | G37 | a level, polled, active-low |
| B / side | G39 | a level, polled, active-low |
| Power | AXP192 **PEK** pin, over I2C | a **latch**, drained from reg `0x46` |

This is why the sources look like they disagree and do not. M5Stack's official PinMap
and the factory `Config.h` list `BUTTON_A_PIN 37` / `BUTTON_B_PIN 39` and stop there;
the product page separately says "Custom button x 2" and then documents power-on and
power-off as long presses of *the power button*. Both statements are true. "Two"
counts the pins you can assign; the third button is real and is simply wired somewhere
a pin map cannot show it.

**Why it matters:** the power button **cannot be fed through the same debounce as the
other two**. There is no level to sample. The PMIC does its own debouncing and its own
press-duration timing against thresholds written to reg `0x36`, and offers the firmware
one write-1-to-clear latch:

- reg `0x46` bit 1 (`0x02`) — a **short** press happened since the latch was cleared;
- reg `0x46` bit 0 (`0x01`) — a **long** press happened;
- clear by writing `0x03`.

Two consequences fall out of that, and both are load-bearing:

1. **Reading is destructive and rate-limited by your poll.** The latch records *that* a
   press happened, not how many or when, so its timing resolution is the polling period
   and it cannot participate in a double-click window shared with the GPIO buttons.
2. **A long press is not the firmware's to handle.** `0x36 = 0x0C` sets power-off at
   four seconds; when it completes the PMIC cuts the rails and the ESP32 is gone. There
   is no code path in which a long press is observed and acted on.

So a button abstraction that models this board needs **two ports, not one** — a levelled
port for G37/G39 and a latched port for the PEK — converging on a common event type.
Forcing the PEK through a levelled port means synthesizing a press history that never
existed. That is the shape `platform-input` takes.

**Evidence:** the vendored factory driver, `kb/sources/m5stack-m5stickc-plus/src/AXP192.cpp`:

```cpp
// 0 not press, 0x01 long press, 0x02 press
uint8_t AXP192::GetBtnPress() {
    uint8_t state = Read8bit(0x46);
    if (state) { Write1Byte(0x46, 0x03); }
    return state;
}
```

**One thing worth knowing that the datasheet would not lead you to:** `begin()` never
writes the IRQ-*enable* registers (`0x40`–`0x44`), and `GetBtnPress` works anyway — so
these status bits latch unconditionally and **no IRQ configuration is required**. An
implementation that carefully enables IRQs first is doing harmless but unnecessary work;
one that assumes enabling is *required* and skips the feature is wrong for no reason.

**See also:** [axp192-powers-lcd-backlight](axp192-powers-lcd-backlight.md) — the same
register byte (`0x12`) that carries EXTEN carries **LDO2**, the backlight, at bit 2. The
backlight is a separate rail from the panel's LDO3, so cutting LDO2 alone darkens the
glass while the ST7789 keeps its framebuffer — which is what makes a backlight toggle
instant in both directions and worth doing at all.
