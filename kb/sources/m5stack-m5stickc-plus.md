---
id: m5stack-m5stickc-plus
title: "M5StickC-Plus — M5Stack's Arduino library + the shipped FactoryTest demo"
type: repo
author: M5Stack
publisher: M5Stack (GitHub)
url: https://github.com/m5stack/M5StickC-Plus
retrieved: 2026-07-03
license: MIT (see ./m5stack-m5stickc-plus/LICENSE)
material: ./m5stack-m5stickc-plus/    # git submodule, pinned 4c87db9 (tag 0.1.1)
seeds: [2026-07-03-identify-factory-firmware, board-runs-factorytest-demo, axp192-powers-lcd-backlight]
---

## Citation

M5Stack. *M5StickC-Plus — M5StickCPlus Arduino Library.* GitHub,
<https://github.com/m5stack/M5StickC-Plus>, pinned at `4c87db9` (one commit past
tag `0.1.1`, 2025-02-17). Retrieved 2026-07-03. MIT.

## What it is

M5Stack's official Arduino library for the M5StickC Plus (ESP32-PICO-D4), and the
source of the **FactoryTest** app the board ships running. Upstream-deprecated in
favour of M5Unified / M5GFX, but still the clearest single-board *bring-up*
reference: it spells out the power-on order and peripheral init constants that the
datasheets only imply. Nothing here compiles into our firmware — it exists to be
read while porting bring-up into `../../firmware`.

**Confirmed to be the app on our board** by dumping app0 and matching FactoryTest's
unique BLE UUIDs — see experiment
[2026-07-03-identify-factory-firmware](../experiments/2026-07-03-identify-factory-firmware/README.md)
and finding [board-runs-factorytest-demo](../findings/board-runs-factorytest-demo.md).
Nuance: the flashed binary was built Apr 2022 (`0.0.7`-era, commit `9b1a17f`);
this submodule sits at HEAD. Same program, cosmetic drift only. Pin to tag `0.0.7`
if you ever need the source byte-matched to the board.

## Regenerate (the reproducibility primitive)

```sh
# Fresh checkout — populate the submodule at its pinned commit:
git submodule update --init kb/sources/m5stack-m5stickc-plus

# Bump to upstream latest (re-pins; review the new commit, then commit):
git submodule update --remote kb/sources/m5stack-m5stickc-plus
```

## What to read, and why

The value is the **power-on order** and **init constants** — a working sequence,
not a register table.

| Path (under `m5stack-m5stickc-plus/`) | Why it matters for `../../firmware` |
|---|---|
| `src/M5StickCPlus.cpp` | The composition root — `begin()` shows the *order*: AXP192 → LCD → Beep → RTC. Mirror it in `firmware/src/main.rs`. Emits the `M5StickCPlus initializing...` banner our board prints. |
| `src/AXP192.cpp` / `.h` | **Read first.** The PMU must be programmed or the board (and LCD backlight) stays dark. Reg `0x28` = LDO2 (TFT_LED) + LDO3 (TFT) 3.0 V; `0x12` enables them; `ScreenBreath()` dims via LDO2. See [axp192-powers-lcd-backlight](../findings/axp192-powers-lcd-backlight.md). |
| `src/M5Display.cpp`, `src/utility/ST7789_Init.h`, `ST7789_Defines.h` | ST7789V2 init command stream + rotation — for a future display adapter. |
| `src/RTC.cpp` / `.h` | BM8563 (`0x51`) over the internal I2C bus. |
| `src/utility/MPU6886.*` | 6-axis IMU (`0x68`) init + register map. |
| `src/utility/Button.*` | Front (G37) / side (G39) button debounce. |
| `src/utility/Speaker.*` | Passive buzzer on G2 — the pin our strip must avoid. |
| `examples/FactoryTest/FactoryTest.ino` | The demo itself (BLE UART, IMU cube, mic FFT, IR) — how the pieces wire into one app. |

Pins + I2C addresses: [board-reference guide](../guides/m5stickc-plus-board-reference.md).
