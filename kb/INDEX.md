# kb index

Curated one-liners, grouped by topic. Within a topic they run **raw → derived**:
`source` / `experiment` (raw) first, then the `finding` / `guide` distilled from
them — so each cluster reads as a lineage. Skim here, read the files. See
[README.md](README.md) for how the two-voice system works.

## The board — identity, hardware, bring-up

- `source` [m5stack-m5stickc-plus](sources/m5stack-m5stickc-plus.md) — M5Stack's
  Arduino library + the shipped **FactoryTest** demo (git submodule, MIT, pinned
  `4c87db9`/`0.1.1`). The clearest bring-up reference: `begin()` shows the AXP192 →
  LCD → Beep → RTC power-on order the datasheets only imply.
- `source` [m5stickc-plus-datasheets](sources/m5stickc-plus-datasheets.md) — board
  schematic + ESP32/AXP192/ST7789/MPU6886/BM8563/SPM1423 datasheets (PDFs
  gitignored; `fetch.sh` reproduces them). Most relevant: ESP32 **TRM** (RMT clocks
  WS2812) + schematic + AXP192.
- `source` [m5stickc-plus-factory-image](sources/m5stickc-plus-factory-image.md) —
  the **whole 4 MB flash read off our board** while stock, committed (not
  gitignored) because it's **irreplaceable** — once we flash, the factory demo
  can't be regenerated, only restored from this file (`espflash write-bin 0x0`).
  Verified complete; sha256 in its `SHA256SUMS`.
- `experiment` [2026-07-03 · Is our board running FactoryTest?](experiments/2026-07-03-identify-factory-firmware/README.md)
  — **confirmed**: dumped live `app0`, matched FactoryTest's three random BLE UUIDs
  + private strings. Not a name-match — a fingerprint match.
- `finding` [board-runs-factorytest-demo](findings/board-runs-factorytest-demo.md)
  — `high` · the flashed app *is* FactoryTest (Apr-2022 / `0.0.7`-era build),
  unmodified — until we flash our own, at which point re-verify.
- `finding` [axp192-powers-lcd-backlight](findings/axp192-powers-lcd-backlight.md)
  — `high` · display stays dark until the AXP192 (`0x34`) is programmed; backlight
  is the **LDO2** rail (I2C reg `0x28`), not a GPIO/PWM. Bring the PMU up first.
  (Its old "Grove 5 V needs no PMU" aside is **corrected** — see below.)
- `experiment` [2026-07-08 · Does AXP192 EXTEN gate the Grove 5 V rail?](experiments/2026-07-08-probe-rail-gating/README.md)
  — **confirmed**: clearing reg `0x12` bit 6 drops the Earth Unit's node from a
  saturated 4095 to a hard 0 in 500 ms, three for three. Measured with the ADC, not
  a multimeter: an open-electrode probe reports its own rail.
- `finding` [axp192-exten-gates-grove-5v](findings/axp192-exten-gates-grove-5v.md)
  — `high` · **EXTEN** (reg `0x12` bit 6) is the Grove 5 V pin's enable, so probe
  power-gating (qhw.31) is firmware-only. Corrects the aside above. The settle delay
  is *not* established — re-measure it against a conducting probe.
- `finding` [ws2812-grb-byte-order](findings/ws2812-grb-byte-order.md) — `high` ·
  WS2812 latches **G-R-B**; the WS2812 boundary adapter (qqh.1) does the one swap —
  keep `Rgb` as RGB in the domain and don't double-swap at the boundary.
- `finding` [esp-idf-managed-component-wiring](findings/esp-idf-managed-component-wiring.md)
  — `high` · pulling an IDF managed component (`espressif/mdns`, qhw.8) needs both
  `extra_components` metadata **and** `ESP_IDF_SYS_ROOT_CRATE` — a virtual workspace
  has no `root_package()`, so esp-idf-sys silently reads no component metadata.
- `source` [m5stickc-plus-pinout](sources/m5stickc-plus-pinout.md) — M5Stack's
  **official GPIO PinMap** captured verbatim (`pinmap.md`; `fetch.sh` re-diffs it
  against upstream). The vendor doc that *names the pin numbers*; every one is
  cross-checked against the factory library's `#define`s and they agree.
- `guide` [m5stickc-plus-board-reference](guides/m5stickc-plus-board-reference.md)
  — pin map + I2C addresses + the exposed header pins (G0/G25/G26/G36); **why the
  strip defaults to G32** (G2 = buzzer, G0/G34 = mic are taken).

## Development — toolchain & flashing

- `guide` [esp-rust-toolchain](guides/esp-rust-toolchain.md) — the `esp` rustc fork
  for Xtensa (`espup`, target **`xtensa-esp32-espidf`**, `std` on ESP-IDF), why
  `firmware/` is a detached **workspace** (board-support / firmware-infra /
  adapters / bins), and why no `~/export-esp.sh` is needed (esp-idf-sys
  self-provisions).
- `finding` [esp-rs-ota-version-matrix](findings/esp-rs-ota-version-matrix.md) —
  **⚠️ superseded** by the std/ESP-IDF pivot (WiFi via `esp-idf-svc`, OTA via
  `EspOta`). Kept as the record of the **no_std** esp-hal 1.1 WiFi/OTA stack — the
  path not taken (why `esp-hal-smartled` capped it; the `esp-radio`/`esp-rtos`
  renames).
- `guide` [flashing-and-serial-access](guides/flashing-and-serial-access.md) — the
  board is `/dev/ttyUSB0` (FT232); the three traps that make a working `espflash`
  look broken (missing `dialout` group → `/usr/bin/sg`; `sg` shadowed by ast-grep;
  FT232 fails baud > 115200); and the read/monitor/flash recipes.
- `finding` [serial-open-resets-esp32](findings/serial-open-resets-esp32.md) —
  `high` · opening the port reboots the ESP32 (FT232 auto-reset) and the port is
  single-owner, so a shared console must fan out from **one** holder — never
  `ttyd espflash monitor` (one child per browser tab = reset + byte race).
- `finding` [esp-idf-socket-dup-needs-fullduplex](findings/esp-idf-socket-dup-needs-fullduplex.md)
  — `high` · a socket `dup()` (`TcpStream::try_clone`) fails on ESP-IDF without
  `LWIP_NETCONN_FULLDUPLEX`; the server accepts then **EOFs the handshake** with no
  panic — HA reports a missing `api`. Host `dup()` always works, so the oracle
  stays green (host-green/device-red). Fix: borrow `&TcpStream`, don't clone (qhw.9).
- `guide` [rust-driver-crates](guides/rust-driver-crates.md) — the driver crate
  per component (foundation **`esp-idf-hal`** / embedded-hal 1.0), with the
  **eh-1.0 vs eh-0.2** column that decides drop-in vs work: `axp192`/`mipidsi`/
  `pcf8563` ready; MPU6886 + IR have **no eh-1.0 driver**, so (greenfield, no
  legacy compat) we own thin drivers. Share the I2C bus with `embedded-hal-bus`.
- `guide` [sharing-the-serial-console](guides/sharing-the-serial-console.md) — let
  many viewers (or the web) watch the UART at once: tmux-mirror (recommended) or
  `ser2net`, fronted by the host's `ttyd`/`oauth2-proxy` stack.
  Package availability + configs verified on-host; setups not yet run on the board.

## ESPHome native API & the std/ESP-IDF pivot

- `source` [aioesphomeapi](sources/aioesphomeapi.md) — the ESPHome client HA
  actually speaks (MIT submodule, pinned `1e16d71`, native-API **1.14**). Our
  **oracle**: `api.proto` (the vendored proto source), `_frame_helper` (the frame
  codec's golden capture), `connection.py` (the FSM + Noise flow).
- `source` [ubihome-esphome-native-api](sources/ubihome-esphome-native-api.md) — a
  Rust native-API **server** (MIT submodule, pinned `79c5066`). Design reference,
  not a dep; its `encryption.md` + `packet_encrypted.rs` are the qhw.10 Noise example.
- `source` [esphome-core-api](sources/esphome-core-api.md) — esphome/esphome
  `components/api`, **GPL-3.0 · REFERENCE-ONLY, never copy/vendor**. The C++ ground
  truth for edge cases; read it, implement from the MIT sources.
- `source` [esphome-native-api-protocol](sources/esphome-native-api-protocol.md) —
  the prose spec (developers.esphome.io + HA integration/Noise pages). Framing =
  `0x00` + varuint size + varuint type + payload — **one** length, size before type.
- `source` [rust-on-esp-idf](sources/rust-on-esp-idf.md) — the std/ESP-IDF stack for
  the pivot: the Rust-on-ESP book, `esp-idf-template`, and `esp-idf-sys` **0.37.2** /
  `-hal` **0.46.2** / `-svc` **0.52.1** (ESP-IDF v5.3.x).
- `source` [prost-and-noise](sources/prost-and-noise.md) — `prost` **0.14.4** (the
  wire types, in use) + the Noise stack (`noise-protocol` 0.2, `noise-rust-crypto`
  0.6) and the spec: ESPHome is **`Noise_NNpsk0_25519_ChaChaPoly_SHA256`**, device = responder.
- `source` [embedded-driver-crates](sources/embedded-driver-crates.md) — the eh-1.0
  drivers `mipidsi` **0.10** / `embedded-graphics` **0.8** / `axp192` **0.2** (display +
  PMU); the matching silicon lives in [datasheets](sources/m5stickc-plus-datasheets.md).
- `source` [m5-earth-unit](sources/m5-earth-unit.md) — the **resistive** soil probe
  (U019) on **ADC1 ch5 / G33** (ADC2 dies under WiFi); power-gated, two-endpoint
  calibration. Schematic via `fetch.sh` (PDF gitignored). Records the **divider**:
  local HT7533 3.3 V, 10 kΩ pull-up, **soil in the lower leg**.
- `finding` [saturated-adc-reading-is-not-a-measurement](findings/saturated-adc-reading-is-not-a-measurement.md)
  — `high` · `raw` 4095 (electrodes open) or 0 (rail down) is a **diagnostic, not a
  reading**; the calibration curve happily turns both into a confident 0 %/100 %.
  Reject the rails at the adapter → *unavailable*. A **floating** (unplugged) probe
  lands mid-scale and needs an excitation delta instead.

## Workflow — issue tracking & triage

- `guide` [beads-triage](guides/beads-triage.md) — how we run **beads**: `br`
  (beads_rust) is the store + the git-tracked `issues.jsonl`; `bv` ranks the graph
  (PageRank / betweenness) into robot-JSON triage. The `just` recipes
  (`ready`/`triage`/`next`/`plan`/`bead-sync`/`bead-check`), the "br never runs
  git" sync discipline, the `--json` / `br schema` contract, and why the cycle
  gate reads `br dep cycles` — not bv's lazily-`null` `Cycles` (a false red).
