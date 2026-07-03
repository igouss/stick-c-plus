---
id: m5stickc-plus-board-reference
title: "M5StickC Plus — pin map, I2C bus, and what each pin is reserved for"
kind: guide
scope: board:m5stickc-plus
reviewed: 2026-07-03
distils: [m5stickc-plus-pinout, m5stickc-plus-datasheets, m5stack-m5stickc-plus]
---

Distilled from M5Stack's official [PinMap](../sources/m5stickc-plus-pinout.md),
the board schematic, and the factory library's pin usage — all three agree. The
one number that drives our design: the WS2812 strip defaults to **G32** (Grove),
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

Provenance: every number is stated by M5Stack's official
[PinMap](../sources/m5stickc-plus-pinout.md) and independently compiled into the
factory library — `utility/Config.h` (IR/LED/buttons/buzzer),
`utility/In_eSPI_Setup.h` (TFT SPI), and `Wire1.begin(21, 22)` (I2C).

## Externally exposed GPIO (free for your own use)

Brought out on the Grove port and the side/bottom headers — the pins you can
actually wire to without cutting into an on-board function:

| Pin(s) | Where | Caveat |
|--------|-------|--------|
| **G32, G33** | Grove (HY2.0-4P) | Grove labels them SDA/SCL, but they're plain GPIO — **G32 is our WS2812 default.** |
| G0 | header | also mic CLK + boot-strap; usable but shared |
| G26 | header | free ADC/DAC-capable pin |
| G25 / G36 | header | **share one port** — when one is driven, set the other as a floating input |

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

- The raw pin numbers, verbatim from the vendor:
  [m5stickc-plus-pinout](../sources/m5stickc-plus-pinout.md).
- Power: the display and board are dark until the AXP192 is programmed —
  [axp192-powers-lcd-backlight](../findings/axp192-powers-lcd-backlight.md).
- Raw datasheets + schematic: [m5stickc-plus-datasheets](../sources/m5stickc-plus-datasheets.md).
- The RMT peripheral that clocks WS2812 is documented in the ESP32 TRM (see the
  datasheet catalogue).
