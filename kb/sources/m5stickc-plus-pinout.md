---
id: m5stickc-plus-pinout
title: "M5StickC Plus — official GPIO PinMap"
type: reference
author: M5Stack
publisher: M5Stack (docs.m5stack.com)
url: https://docs.m5stack.com/en/core/m5stickc_plus
retrieved: 2026-07-03
license: "© M5Stack; local reference copy of the PinMap section, not redistributed"
material: ./m5stickc-plus-pinout/    # pinmap.md (verbatim), fetch.sh (regenerate)
seeds: [m5stickc-plus-board-reference]
---

## Citation

M5Stack. *StickC-Plus — PinMap.* M5Stack Docs,
<https://docs.m5stack.com/en/core/m5stickc_plus> (§ PinMap). Canonical markdown:
`m5stack/m5-docs`, `docs/en/core/m5stickc_plus.md` (master). Retrieved 2026-07-03.

## What it is

The vendor's authoritative statement of **which ESP32 GPIO drives which
peripheral** on this board — the one document that names the pin numbers rather
than leaving them to be read off the schematic. It is the raw backing for the
derived [board-reference guide](../guides/m5stickc-plus-board-reference.md);
every number in that pin map traces here (and is independently confirmed by the
factory library's `#define`s — see below).

Verbatim capture in [`m5stickc-plus-pinout/pinmap.md`](./m5stickc-plus-pinout/pinmap.md):
LCD SPI (G15/G13/G23/G18/G5), RED LED G10, IR G9, buttons G37/G39, buzzer G2,
mic G0/G34, internal I2C G22/G21, Grove G33=SCL/G32=SDA, and the externally
exposed header pins **G0, G25/G36, G26, G32, G33** (G25↔G36 share a port).

**Cross-checked against the shipped firmware.** The same numbers appear as
compile-time constants in the factory library
([m5stack-m5stickc-plus](m5stack-m5stickc-plus.md)):
`src/utility/Config.h` (`M5_IR 9`, `M5_LED 10`, `BUTTON_A_PIN 37`,
`BUTTON_B_PIN 39`, `SPEAKER_PIN 2`), `src/utility/In_eSPI_Setup.h`
(`TFT_MOSI 15`, `TFT_SCLK 13`, `TFT_CS 5`, `TFT_DC 23`, `TFT_RST 18`), and
`Wire1.begin(21, 22)` in `AXP192.cpp`/`RTC.cpp`/`MPU6886.cpp`. Doc and code agree.

## Regenerate (the reproducibility primitive)

```sh
# Pull the canonical page markdown next to pinmap.md, then diff the PinMap section:
kb/sources/m5stickc-plus-pinout/fetch.sh
```

`pinmap.md` is committed (small, diffable text); the full upstream page
(`*.upstream.md`) is re-fetchable and gitignored.

## What to read, and why

- [`pinmap.md`](./m5stickc-plus-pinout/pinmap.md) — the whole pinout on one
  screen. Start here when wiring anything to the board.
- The **Grove port** (G32/G33) is labelled SDA/SCL for external I2C but the pins
  are plain GPIO — which is exactly why the firmware repurposes **G32** as the
  WS2812 data line (free, brought out, 5 V-adjacent). See the board-reference
  guide's "why G32" note.
- The **AXP192 rail table** confirms LDO2 = TFT backlight, LDO3 = TFT IC — the
  basis of [axp192-powers-lcd-backlight](../findings/axp192-powers-lcd-backlight.md).
