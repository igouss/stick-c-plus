---
id: rust-driver-crates
title: "M5StickC Plus — Rust driver crates, per component (esp-idf-hal / embedded-hal 1.0)"
kind: guide
scope: board:m5stickc-plus
reviewed: 2026-07-04
distils: [m5stickc-plus-board-reference, esp32-toolchain-and-pins]
---

Candidate driver crates for each on-board part, checked against crates.io on
2026-07-03. The board's firmware is **`std` on ESP-IDF** — the foundation is
**`esp-idf-hal`** / **`esp-idf-svc`**, whose peripheral types (`I2cDriver`,
`SpiDeviceDriver`, `AdcChannelDriver`, `TxRmtDriver`, …) implement **`embedded-hal`
1.0** traits. So the one number that still decides "drops in" vs "needs work" is
whether a *device* driver is on **`embedded-hal` 1.0** or the old **0.2**. That
column is the whole story.

**Project stance: greenfield, latest versions, no legacy-compat shims.** Where a
part's only driver is stuck on `embedded-hal` 0.2, we do **not** pull in
`embedded-hal-compat` to bridge it — we write a thin eh-1.0 driver and own it,
exactly as the firmware owns its WS2812 RMT encoder (over `esp-idf-hal` RMT)
rather than lean on a smart-LED crate. A small crate we control beats a legacy
crate behind a shim.

## Per-component crate map

| Component (pin) | Crate | Latest | embedded-hal | Verdict |
|---|---|---|---|---|
| ESP32-PICO-D4 SoC | **`esp-idf-hal`** / `esp-idf-svc` | 0.46 / 0.52 | peripheral types impl **1.0** | ✅ foundation (std/ESP-IDF) |
| AXP192 PMU (I2C `0x34`) | **`axp192`** | 0.2.0 | **^1.0** | ✅ drops in |
| ST7789V2 LCD (SPI) | **`mipidsi`** | 0.10.0 | **^1.0** | ✅ use its ST7789 model — **not** the `st7789` crate (it's eh 0.2) |
| — 2D drawing | **`embedded-graphics`** | 0.8.2 | n/a (own traits) | ✅ pairs with mipidsi |
| MPU6886 IMU (I2C `0x68`) | *(none on eh-1.0)* | `mpu6886` is eh-0.2 | ⚠️ **^0.2.4** | ✍️ **own it** — no eh-1.0 driver exists (see below) |
| BM8563 RTC (I2C `0x51`) | **`pcf8563`** | 0.2.1 | **^1.0** | ✅ BM8563 is PCF8563 register-compatible |
| WS2812 strip (G32) | **own encoder** over `esp-idf-hal` RMT (`TxRmtDriver` + `FixedLengthSignal`) | — | n/a | ✅ own the encoder (qqh.1) |
| — SPI-driven alt | `ws2812-spi` | 0.5.1 | ^1.0 | ✅ if you clock it off SPI instead of RMT |
| IR TX (G9) | *(none on eh-1.0)* | `infrared` is eh-0.2 | ⚠️ **^0.2.4** | ✍️ **own it** — raw RMT carrier (see below) |
| SPM1423 mic (G0/G34, PDM) | — none — | — | — | raw PDM via **`esp-idf-hal` I2S**; no driver crate |
| Buttons G37/G39, buzzer G2 | — none — | — | — | `esp-idf-hal` **GPIO** / **LEDC** (PWM) directly |

## The cross-cutting crate

- **`embedded-hal-bus`** (0.3) — the internal I2C bus (**G21/G22**) is shared by
  three devices (AXP192 + MPU6886 + BM8563). `esp-idf-hal`'s `I2cDriver` is
  eh-1.0, so wrap the one bus in `RefCellDevice`/`AtomicDevice` and hand each
  driver its own handle. It's the eh-1.0 successor to `shared-bus` — use it.

## The two parts with no eh-1.0 driver — own them

`mpu6886` and `infrared` are still on `embedded-hal` 0.2. Greenfield + no
legacy-compat means we **don't** shim them with `embedded-hal-compat`; we write
small eh-1.0 drivers we control:

- **MPU6886 IMU** — a thin register-map driver over `embedded-hal::i2c`
  (WHO_AM_I `0x75` = `0x19`; accel/gyro block reads; the factory library's
  `utility/MPU6886.cpp` is the reference register sequence). ~1 file, a driven
  adapter. In-character: the firmware already owns its WS2812 RMT encoder rather
  than depend on a smart-LED crate.
- **IR TX (G9)** — no driver needed at all: the transmitter is a modulated
  carrier, so `esp-idf-hal`'s **RMT** emits NEC/custom codes directly. Reach for a
  protocol crate only if we later need to *decode* arbitrary remotes.

## Notes

- **Display:** two crates match ST7789 — pick `mipidsi` (eh 1.0, actively
  maintained, generic MIPI DCS) over `st7789` (eh 0.2, effectively superseded).
- **RTC:** no `bm8563` crate exists; `pcf8563` is the correct match because the
  BM8563 is a PCF8563 clone — same registers, same I2C address `0x51`.
- **Concurrency:** ESP-IDF ships FreeRTOS, so the imperative shell uses
  `esp-idf-hal`/`esp-idf-svc` threads and the blocking accept/sample loops — no
  Embassy executor on this stack (that was the no_std path).
- **Logging:** `esp-idf-svc`'s `EspLogger` behind the `log` crate (`info!` reaches
  the serial monitor) — not `esp-println`.

## See also

- Pin assignments each driver needs: [m5stickc-plus-board-reference](m5stickc-plus-board-reference.md).
- Which chip is which: [m5stickc-plus-datasheets](../sources/m5stickc-plus-datasheets.md).
- Toolchain the crates build under: [esp-rust-toolchain](esp-rust-toolchain.md).
- The eh-1.0 display/PMU crate versions: [embedded-driver-crates](../sources/embedded-driver-crates.md).
