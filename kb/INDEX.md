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
- `finding` [board-has-three-buttons-two-acquisition-paths](findings/board-has-three-buttons-two-acquisition-paths.md)
  — `high` · the power button is on the PMIC, not a pin, so it needs its own port
- `finding` [axp192-exten-gates-grove-5v](findings/axp192-exten-gates-grove-5v.md)
  — `high` · **EXTEN** (reg `0x12` bit 6) is the Grove 5 V pin's enable, so probe
  power-gating (qhw.31) is firmware-only. Corrects the aside above. The settle delay
  is *not* established — re-measure it against a conducting probe.
- `finding` [ws2812-grb-byte-order](findings/ws2812-grb-byte-order.md) — `high` ·
  WS2812 latches **G-R-B**; the WS2812 boundary adapter (qqh.1) does the one swap —
  keep `Rgb` as RGB in the domain and don't double-swap at the boundary.
- `experiment` [2026-07-09 · RGB or BGR on the onboard TFT?](experiments/2026-07-09-panel-colour-order/README.md)
  — **confirmed**: under `mipidsi`, `ColorOrder::Bgr` renders red as blue. Three
  labelled bands through the production init; green invariant, white stayed white.
- `finding` [st7789-wants-rgb-colour-order](findings/st7789-wants-rgb-colour-order.md)
  — `high` · the TFT needs **`ColorOrder::Rgb`**, though the factory library resolves
  to `TFT_MAD_BGR` and mipidsi's `Bgr` sets that same MADCTL bit. A MADCTL value does
  **not** transfer across driver stacks. White-on-black cannot test colour, so this
  shipped unseen until the first coloured pixel.
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
- `guide` [host-monitor-hostpulse](guides/host-monitor-hostpulse.md) — the
  **host monitor** (current): the bearer-gated **hostpulse** endpoint that returns a
  ready-to-plot CPU/memory series for every homelab host at once (`rate()` done
  server-side), its JSON contract + gap/down/502 semantics, the `[host_monitor]`
  endpoint+token config, the pre-flash `curl` checks, and what the three-row glass shows
  (gaps as baseline ticks, the window-span label, the health token).
- `guide` [host-monitor-node-exporter](guides/host-monitor-node-exporter.md) —
  **superseded** by hostpulse (2026-07-16), kept as history: install node_exporter, the
  four metrics the device read (CPU as a counter-delta rate, memory as a level), and the
  scientific-notation / large-body / counter-reset gotchas the old on-device parser
  handled.

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

## Display — assets

- `source` [claudepix](sources/claudepix.md) — the vendored **ClaudePix** 20×20
  creature animations (13 presets, 216 frames), decoded into `plant-display`'s 4-bit
  sprite format. **No licence is stated upstream** — resolve before publishing.
  Records the `window.PRESET` bleed that made one preset silently become another, and
  the two-path hash verification (`node:vm` vs a real browser) that pins the copy.
- `experiment` [2026-07-09 · Does a 100×100 sprite frame fit a 50 ms tick?](experiments/2026-07-09-sprite-fill-throughput/README.md)
- `experiment` [2026-07-21 · Does turning the panel cost paint time? (no — and what does)](experiments/2026-07-21-paint-cost-by-rotation/README.md)
  — **confirmed**: 400 per-cell `Rectangle` fills = ~85 ms/frame; one `fill_contiguous`
  = 0 over-budget paints. Two flashes, same pixels, only the fill strategy changed.
- `experiment` [2026-07-21 · What blocks the pomodoro paint for 39 ms? (nothing — it was never blocked)](experiments/2026-07-21-what-blocks-the-pomodoro-paint/README.md)
  — **confirmed**: a subtractive sweep over every app thread found *no* breach in 1000
  paints and said so, then the missing stage found it — a paint carrying a **turn**
  costs 59.6 ms against 21.5 ms settled, 19 of 19 real turns over budget. Corrects the
  previous experiment's *reading* (the paint was doing more work, not waiting).
- `finding` [mipidsi-rectangle-fill-costs-an-address-window](findings/mipidsi-rectangle-fill-costs-an-address-window.md)
  — `high` · a `Rectangle` fill costs a `CASET`/`RASET`/`RAMWR` window setup, so cost
  scales with the *fill count*, not the pixel count. Payload arithmetic says 5.9 ms and
  is confidently wrong. Invisible to every host test **and** to the eye.
- `finding` [turning-the-panel-costs-a-full-screen-clear](findings/turning-the-panel-costs-a-full-screen-clear.md)
  — `high` · a paint that *changes* rotation carries a MADCTL write **and a full-screen
  clear**, ~38 ms, inside the timed `show`. Hides because `set_rotation` early-returns
  when unchanged, and only breaches while the picture is animated (50 ms budget, not 1 s).
