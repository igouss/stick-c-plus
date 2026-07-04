# stick-c-plus — M5StickC Plus firmware in Rust, into Home Assistant

Rust firmware for the **M5StickC Plus** (ESP32-PICO-D4, Xtensa LX6, 4 MB flash,
520 KB SRAM, no PSRAM), built `std` on **ESP-IDF**. Three projects share one
foundation, each surfacing to **Home Assistant** through a home-grown **ESPHome
native-API** crate — so HA does the storing, graphing, and alerting:

1. **plant-monitor** — an M5 **Earth Unit** soil probe → moisture dashboard +
   watering alerts. *The immediate deliverable.*
2. **led-driver** — a NightDriverStrip-style **WS2812** animation driver (the
   repo's original purpose; the `led-core` effects domain lives on).
3. **rover** — a controllable robot. *Future; diverges in hardware.*

## Architecture — hexagonal / ECB

The domain is framework-free and dependencies point inward. The heart of each
system (LED effects, the moisture curve, the protocol logic) never mentions
hardware, so it is verified on the host — Gherkin, property, and unit tests — with
no device attached. The firmware is the thin **imperative shell**: adapters that
implement the domain's ports against real ESP-IDF peripherals, plus a composition
root per binary.

```
stick-c-plus/
├─ domain/            # led-core — pure no_std LED-animation domain (project #2). host-tested.
├─ plant-core/        # pure no_std soil-moisture domain (project #1). host-tested.
├─ firmware-core/     # pure no_std shared kernel — ADC oversampling, probe-power gating. host-tested.
├─ plant-shell/       # std imperative shell — shared moisture cache + sampler thread. host-tested.
├─ esphome-api/       # std ESPHome native-API framework (prost + std::net). host-tested.
├─ firmware/          # the Xtensa boundary — a detached std/ESP-IDF workspace (qhw.2):
│  ├─ board-support/       #   infra          — BSP: AXP192 power-on, pin map, bring-up
│  ├─ firmware-infra/      #   infra          — WiFi STA, mDNS, native-API host, OTA
│  ├─ adapters/            #   driven-adapter — domain-port adapters (adc/st7789/ws2812/clock/wifi)
│  └─ bins/plant-monitor/  #   composition-root — bin #1, the composition root
└─ kb/                # Knowledge base — board facts, sources, findings (kbe-style)
   ├─ sources/        #   Raw: datasheets + the M5 factory firmware (submodule)
   ├─ experiments/    #   Raw: what we probed on the metal (+ results)
   ├─ findings/       #   Derived: falsifiable board facts, one per file
   └─ guides/         #   Derived: toolchain, flashing, pin map, driver crates
```

Each firmware crate carries one `[package.metadata.hex-arch] role` tag; the
mechanical `hex-lint` / `effect-audit` gates that read them land with qhw.14.

`kb/` is a [`~/kbe`](../../../kbe/README.md)-style knowledge base for everything we
learn about this board — cited sources, on-device experiments, and the findings
distilled from them. It never compiles into the firmware. Its headline source is
M5Stack's shipped **FactoryTest** app
([`m5stack/M5StickC-Plus`](https://github.com/m5stack/M5StickC-Plus), a pinned
submodule) — the AXP192 / ST7789 bring-up we port into `firmware/`, **verified to
be the exact app our board shipped with**. Start at [`kb/INDEX.md`](kb/INDEX.md).
Fresh checkouts: `git submodule update --init`.

The two worlds build under **different toolchains**, on purpose:

| Crate(s) | Toolchain | Target | Tested |
|---|---|---|---|
| host — `led-core`, `plant-core`, `firmware-core`, `plant-shell`, `esphome-api` | stable | host | yes — `cargo test --workspace` |
| firmware | `esp` fork | `xtensa-esp32-espidf` (`std`) | on device |

`firmware/` is its own workspace (`[workspace]` + root `exclude`) so its Xtensa
target and `esp` toolchain never touch `cargo test`; firmware crates reach the
host domain crates by **path** across the boundary.

## Status

- **`led-core`**, **`plant-core`** — domains done, host-tested (unit + property +
  Gherkin). `plant-core` includes the `fresh` staleness policy: a cached reading
  goes unavailable once it ages out.
- **`firmware-core`**, **`plant-shell`** — the host-testable shell. `firmware-core`
  is the pure shared kernel (ADC oversampling, probe-power gating); `plant-shell`
  is the imperative shell — the shared moisture cache and the sampler thread
  (read → `step` → publish), with its staleness-on-death and panic-isolation rules
  proven on the host.
- **`esphome-api`** — native-API framework: prost message types, the frame codec,
  the entity model, the connection FSM, and the Light entity. Host-tested against
  golden captures + an `aioesphomeapi` oracle. Plaintext first; Noise is a
  fast-follow.
- **firmware** — the workspace is carved and `bins/plant-monitor` boots on
  hardware. The ADC sampler thread now feeds the shared moisture cache (qhw.21);
  WiFi and the on-device native-API server are the next beads (`just ready`).

## Prerequisites

- The `esp` rustc fork + Xtensa toolchain, via [`espup`](https://github.com/esp-rs/espup):
  `cargo install espup && espup install --targets esp32`. **No `~/export-esp.sh`
  to source** — `esp-idf-sys` self-provisions clang, xtensa-gcc, and a Python venv
  under `firmware/.embuild`. A *fresh* ESP-IDF bootstrap needs Python ≤ 3.12 plus
  `ninja` and `ldproxy` on `PATH` (the justfile handles the Python shim).
- [`espflash`](https://github.com/esp-rs/espflash) for flashing.

## Build & test

```sh
just test          # host domain — stable rustc, no device
just build         # firmware — Xtensa std/ESP-IDF (release)
just ci            # fmt + lint (both worlds) + test + build
```

## Flash & run

Connect the board (appears as `/dev/ttyUSB0`), then:

```sh
just run           # build + flash + monitor  (a.k.a. `just flash`)
just monitor       # serial monitor only — pty-free (espflash --non-interactive)
```

Serial traps (dialout group, the `sg`/ast-grep shadow, the FT232 baud ceiling) are
written up in [`kb/guides/flashing-and-serial-access.md`](kb/guides/flashing-and-serial-access.md).

## Hardware wiring

Per-project and pin-exact in the KB: the plant probe is the M5 Earth Unit on
**G33 (ADC1_CH5)** — ADC1 so it coexists with WiFi — and the LED strip (project
#2) is WS2812 on **G32**. See
[`kb/guides/m5stickc-plus-board-reference.md`](kb/guides/m5stickc-plus-board-reference.md).

## Roadmap

Tracked in beads — `just ready` for unblocked work, `just triage` for graph-ranked
recommendations; the workflow is written up in
[`kb/guides/beads-triage.md`](kb/guides/beads-triage.md). Next up: WiFi STA + mDNS,
the on-device ESPHome native-API server, the ADC→moisture sampler and Sensor entity
(→ the HA dashboard), then Noise encryption, the ST7789 status display, and OTA.
Project #2 (the WS2812 driver) and project #3 (the rover) build on the same
foundation.
