---
id: m5stickc-plus-datasheets
title: "M5StickC Plus — board + component datasheets and schematic"
type: datasheet
author: Espressif, X-Powers, Sitronix, TDK InvenSense, Knowles, M5Stack
publisher: various (see per-doc source column)
url: https://docs.m5stack.com/en/core/m5stickc_plus
retrieved: 2026-07-03
license: "© respective vendors; local reference copies, not redistributed"
material: ./m5stickc-plus-datasheets/    # PDFs gitignored; fetch.sh reproduces them
seeds: [m5stickc-plus-board-reference, axp192-powers-lcd-backlight]
---

## Citation

Datasheets and the board schematic for the M5StickC Plus and its on-board parts,
from each component vendor and M5Stack. Canonical board page:
<https://docs.m5stack.com/en/core/m5stickc_plus>. Retrieved 2026-07-03.

## What it is

The primary hardware references for the board. The PDFs are large immutable
binaries and are **not committed** — this note is the version-controlled record
(what each doc covers, its source) and `./m5stickc-plus-datasheets/fetch.sh`
reproduces the binaries locally. Most relevant to this LED driver: the **ESP32
TRM** (RMT peripheral that clocks WS2812) and the **schematic** + **AXP192** (the
PMU must be alive to power the board and the LCD backlight).

## Regenerate (the reproducibility primitive)

```sh
# Idempotent: existing valid PDFs are skipped, missing/corrupt ones re-fetched.
kb/sources/m5stickc-plus-datasheets/fetch.sh
```

## The documents

| File | Component | Covers | Source |
|------|-----------|--------|--------|
| `m5stickc_plus_schematic.pdf` | whole board | Wiring, nets, connectors | M5Stack (k016-p) |
| `esp32-pico-d4_datasheet_en.pdf` | ESP32-PICO-D4 | The SiP on this board (ESP32 + 4 MB flash) | Espressif |
| `esp32_datasheet_en.pdf` | ESP32 | SoC electrical/peripheral summary | Espressif |
| `esp32_technical_reference_manual_en.pdf` | ESP32 | **Peripheral registers — RMT (WS2812), GPIO, I2C, timers** | Espressif |
| `axp192_pmu.pdf` | AXP192 | Power management — rails, LDOs, LCD backlight, battery | X-Powers / M5Stack |
| `st7789v2_display.pdf` | ST7789V2 | 1.14" 135×240 TFT controller | Sitronix / M5Stack |
| `mpu6886_imu.pdf` | MPU6886 | 6-axis accel + gyro | TDK InvenSense / M5Stack |
| `bm8563_rtc_cn.pdf` | BM8563 | Real-time clock (PCF8563-compatible), 中文 | M5Stack |
| `spm1423_mic.pdf` | SPM1423 | PDM MEMS microphone | Knowles / M5Stack |

The **pin map + I2C addresses** distilled from the schematic live in the derived
[board-reference guide](../guides/m5stickc-plus-board-reference.md) — this note
stays a raw catalogue.
