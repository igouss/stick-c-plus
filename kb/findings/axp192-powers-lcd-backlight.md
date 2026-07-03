---
id: axp192-powers-lcd-backlight
title: "The AXP192 PMU must be programmed before the LCD (and board rails) come alive"
confidence: high
scope: board:m5stickc-plus
derived-from: []
supersedes: []
reviewed: 2026-07-03
check: manual   # inspect src/AXP192.cpp::begin in the factory-firmware submodule
---

**Claim:** On the M5StickC Plus the display and its backlight are dark until the
**AXP192** (`0x34` on the internal I2C bus, G21/G22) is initialised. The backlight
is not a GPIO — it's the **LDO2** rail; the TFT panel is **LDO3**. Any firmware that
skips PMU setup gets a black screen even if the ST7789 is driven correctly.

**Evidence:** `kb/sources/m5stack-m5stickc-plus/src/AXP192.cpp::begin()` —
`Write1Byte(0x28, 0xcc)` sets *"LDO2 & LDO3 (TFT_LED & TFT) 3.0V"*, and
`Write1Byte(0x12, Read8bit(0x12) | 0x4D)` enables LDO2/LDO3/DCDC1/Ext. Brightness
is `ScreenBreath()`, which maps 0–100 % into the LDO2 voltage nibble of reg `0x28`
— i.e. **backlight = LDO2 voltage**, set over I2C, not PWM on a pin. `M5.begin()`
calls `Axp.begin()` *before* `Lcd.begin()`; the order is load-bearing.

**Holds when:** driving the on-board 1.14" ST7789V2 display or relying on any rail
the AXP192 gates (it also feeds the ESP32 core rails at boot).

**Breaks when:** — not really; it's structural to this board. (An external strip on
the Grove 5 V pin does **not** need the AXP192, so a pure-LED firmware can ignore
the PMU — until it wants the screen.)

**How to apply:** When a display or IMU adapter is added to `../../firmware`, bring
up an AXP192 adapter **first** in the composition root, mirroring `begin()`'s order
(AXP192 → LCD → …). Backlight brightness is an I2C write to reg `0x28`, not a PWM
channel. Until then our WS2812-only firmware legitimately skips it.
