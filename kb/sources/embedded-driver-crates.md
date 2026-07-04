---
id: embedded-driver-crates
title: "Display + power driver crates (mipidsi, embedded-graphics, axp192)"
type: reference
author: embedded-graphics + embedded-hal community
publisher: docs.rs, crates.io
url: https://docs.rs/mipidsi
retrieved: 2026-07-04
license: "MIT OR Apache-2.0 (each crate). Reference links."
material: none    # crates by pinned version; datasheets pinned separately
seeds: []
---

## Citation

The `eh-1.0` driver crates for the board's ST7789 TFT and AXP192 PMU, consulted
2026-07-04. References for the display adapter (qhw.6) and the AXP192 power-on
(qhw.20).

## What it is

The reusable Rust drivers the firmware adapters wrap, chosen thin over
`embedded-hal 1.0` per the greenfield "own a thin eh-1.0 driver, no eh-0.2
bridge" rule:

- **mipidsi** — the ST7789 TFT controller driver (the 1.14″ 135×240 panel).
- **embedded-graphics** — the 2D drawing primitives rendered onto it (moisture
  readout).
- **axp192** — the AXP192 PMU driver; the PMU must be brought up first or the
  panel and its backlight stay dark (see
  [axp192-powers-lcd-backlight](../findings/axp192-powers-lcd-backlight.md)).

The **hardware datasheets** for these parts (ST7789V2, AXP192) are already pinned
under [m5stickc-plus-datasheets](m5stickc-plus-datasheets.md) — this note pins
the *software* drivers; that note pins the silicon.

## The references (pinned versions)

| Crate | Version | Part | Where |
|-------|---------|------|-------|
| `mipidsi` | **0.10.0** | ST7789V2 TFT | <https://docs.rs/mipidsi/0.10.0> |
| `embedded-graphics` | **0.8.2** | 2D rendering | <https://docs.rs/embedded-graphics/0.8.2> |
| `axp192` | **0.2.0** | AXP192 PMU | <https://docs.rs/axp192/0.2.0> |

## Regenerate (the reproducibility primitive)

Crates pinned by version in `firmware/Cargo.toml` when qhw.6/qhw.20 land. The
matching datasheets: `kb/sources/m5stickc-plus-datasheets/fetch.sh`.
