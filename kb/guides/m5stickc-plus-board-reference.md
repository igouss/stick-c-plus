---
id: m5stickc-plus-board-reference
title: "M5StickC Plus — pin map, I2C bus, and what each pin is reserved for"
kind: guide
scope: board:m5stickc-plus
reviewed: 2026-07-03
distils: [m5stickc-plus-datasheets, m5stack-m5stickc-plus]
---

Distilled from the board schematic and the factory library's pin usage. The one
number that drives our design: the WS2812 strip defaults to **G32** (Grove),
because the tempting alternatives are already taken (buzzer, mic).

## GPIO pin map

| Function | Pin(s) | Notes |
|----------|--------|-------|
| **Grove port (HY2.0-4P)** | **G32, G33** | + 5V, GND. **G32 is the firmware's WS2812 data default.** |
| LCD ST7789V2 | G15 MOSI, G13 CLK, G23 DC, G18 RST, G5 CS | SPI; backlight via AXP192 LDO2 |
| Button A | G37 | front |
| Button B | G39 | side |
| Red LED | G10 | plain GPIO, active-low |
| IR transmitter | G9 | |
| Passive buzzer | G2 | **avoid for the strip — reserved** |
| Microphone SPM1423 | G0 CLK, G34 DATA | PDM |
| Internal I2C | G22 SCL, G21 SDA | shared bus, below |

## Internal I2C addresses

| Device | Address |
|--------|---------|
| AXP192 (PMU) | `0x34` |
| MPU6886 (IMU) | `0x68` |
| BM8563 (RTC) | `0x51` |

## Why G32 for the strip

G2 carries the buzzer and G0/G34 the mic, so the Grove pins (**G32/G33**) are the
clean choice for external LEDs — free, 5 V-adjacent, and brought out to the
connector. `LED_DATA_PIN` in `../../firmware/src/main.rs` defaults to G32; set it
and `LED_COUNT` to match your strip.

## See also

- Power: the display and board are dark until the AXP192 is programmed —
  [axp192-powers-lcd-backlight](../findings/axp192-powers-lcd-backlight.md).
- Raw datasheets + schematic: [m5stickc-plus-datasheets](../sources/m5stickc-plus-datasheets.md).
- The RMT peripheral that clocks WS2812 is documented in the ESP32 TRM (see the
  datasheet catalogue).
