---
id: rust-driver-crates
title: "M5StickC Plus — Rust driver crates, per component (esp-hal 1.1 / embedded-hal 1.0)"
kind: guide
scope: board:m5stickc-plus
reviewed: 2026-07-03
distils: [m5stickc-plus-board-reference, esp32-toolchain-and-pins]
---

Candidate `no_std` crates for each on-board part, checked against crates.io on
2026-07-03. The board's firmware is on **`esp-hal` 1.1**, which implements
**`embedded-hal` 1.0** traits — so the one number that decides "drops in" vs
"needs work" is whether a driver is on **`embedded-hal` 1.0** or still the old
**0.2**. That column is the whole story.

**Project stance: greenfield, latest versions, no legacy-compat shims.** Where a
part's only driver is stuck on `embedded-hal` 0.2, we do **not** pull in
`embedded-hal-compat` to bridge it — we write a thin eh-1.0 driver and own it,
exactly as the firmware already owns its RMT WS2812 encoder rather than depend on
`esp-hal-smartled`. A small crate we control beats a legacy crate behind a shim.

## Per-component crate map

| Component (pin) | Crate | Latest | embedded-hal | Verdict |
|---|---|---|---|---|
| ESP32-PICO-D4 SoC | **`esp-hal`** | 1.1.1 | implements 1.0 | ✅ foundation, already in use |
| AXP192 PMU (I2C `0x34`) | **`axp192`** | 0.2.0 | **^1.0** | ✅ drops in |
| ST7789V2 LCD (SPI) | **`mipidsi`** | 0.10.0 | **^1.0** | ✅ use its ST7789 model — **not** the `st7789` crate (it's eh 0.2) |
| — 2D drawing | **`embedded-graphics`** | 0.8.2 | n/a (own traits) | ✅ pairs with mipidsi |
| MPU6886 IMU (I2C `0x68`) | *(none on eh-1.0)* | `mpu6886` is eh-0.2 | ⚠️ **^0.2.4** | ✍️ **own it** — no eh-1.0 driver exists (see below) |
| BM8563 RTC (I2C `0x51`) | **`pcf8563`** | 0.2.1 | **^1.0** | ✅ BM8563 is PCF8563 register-compatible |
| WS2812 strip (G32, project target) | **`smart-leds`** + esp-hal RMT | 0.4.0 | traits only | ✅ current approach (own RMT encoder) |
| — SPI-driven alt | `ws2812-spi` | 0.5.1 | ^1.0 | ✅ if you clock it off SPI instead of RMT |
| IR TX (G9) | *(none on eh-1.0)* | `infrared` is eh-0.2 | ⚠️ **^0.2.4** | ✍️ **own it** — raw RMT carrier (see below) |
| SPM1423 mic (G0/G34, PDM) | — none — | — | — | raw PDM via **esp-hal I2S**; no driver crate |
| Buttons G37/G39, buzzer G2 | — none — | — | — | esp-hal **GPIO** / **LEDC** (PWM) directly |

## The cross-cutting crate

- **`embedded-hal-bus`** (0.3) — the internal I2C bus (**G21/G22**) is shared by
  three devices (AXP192 + MPU6886 + BM8563). Wrap the one bus in
  `RefCellDevice`/`AtomicDevice` and hand each driver its own handle. It's the
  eh-1.0 successor to `shared-bus` (0.3.1) — use it, not the old one.

## The two parts with no eh-1.0 driver — own them

`mpu6886` and `infrared` are still on `embedded-hal` 0.2. Greenfield + no
legacy-compat means we **don't** shim them with `embedded-hal-compat`; we write
small eh-1.0 drivers we control:

- **MPU6886 IMU** — a thin register-map driver over `embedded-hal::i2c`
  (WHO_AM_I `0x75` = `0x19`; accel/gyro block reads; the factory library's
  `utility/MPU6886.cpp` is the reference register sequence). ~1 file, in the
  domain's driver boundary. In-character: the firmware already owns its RMT
  WS2812 encoder rather than depend on `esp-hal-smartled`
  (see [esp-rs-ota-version-matrix](../findings/esp-rs-ota-version-matrix.md)).
- **IR TX (G9)** — no driver needed at all: the transmitter is a modulated
  carrier, so esp-hal's **RMT** emits NEC/custom codes directly. Reach for a
  protocol crate only if we later need to *decode* arbitrary remotes.

## Notes

- **Display:** two crates match ST7789 — pick `mipidsi` (eh 1.0, actively
  maintained, generic MIPI DCS) over `st7789` (eh 0.2, effectively superseded).
- **RTC:** no `bm8563` crate exists; `pcf8563` is the correct match because the
  BM8563 is a PCF8563 clone — same registers, same I2C address `0x51`.
- **Async:** for an interrupt/timer-driven build, `esp-hal-embassy` (0.9) adds the
  Embassy executor; the WiFi/OTA path uses `esp-radio`/`esp-rtos` instead — see
  [esp-rs-ota-version-matrix](../findings/esp-rs-ota-version-matrix.md).
- **Logging:** `esp-println` (0.17) for `println!` over the UART.

## See also

- Pin assignments each driver needs: [m5stickc-plus-board-reference](m5stickc-plus-board-reference.md).
- Which chip is which: [m5stickc-plus-datasheets](../sources/m5stickc-plus-datasheets.md).
- Toolchain the crates build under: [esp-rust-toolchain](esp-rust-toolchain.md).
