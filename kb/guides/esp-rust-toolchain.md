---
id: esp-rust-toolchain
title: "The esp Rust toolchain for Xtensa, and why the firmware is a detached workspace"
kind: guide
scope: chip:esp32
reviewed: 2026-07-04
distils: [m5stack-m5stickc-plus, rust-on-esp-idf]
---

The ESP32 on this board is **Xtensa LX6**, not RISC-V, and Xtensa has no upstream
Rust target. You need Espressif's **`esp` rustc fork**. The firmware builds
**`std` on ESP-IDF** (target `xtensa-esp32-espidf`); the no_std esp-hal stack was
the path not taken — see [rust-on-esp-idf](../sources/rust-on-esp-idf.md) and the
pivot rationale in `docs/plans/plant-monitor-esphome-native-api.md`.

## Setup

```sh
# Install the esp fork + Xtensa toolchain via espup (one-time):
cargo install espup
espup install --targets esp32

# Flasher:
cargo install espflash
```

- Target triple: **`xtensa-esp32-espidf`** (`std`, ESP-IDF **v5.3.x**).
- `firmware/rust-toolchain.toml` pins the **`esp`** channel — cargo selects it
  automatically inside `firmware/`, so the host workspace stays on stable.
- `firmware/.cargo/config.toml` sets the target, the **`ldproxy`** linker, the
  `espflash flash --monitor` runner, and **`build-std`** (no prebuilt Xtensa std
  ships with the esp fork, so std is built from source).

**No `~/export-esp.sh` to source.** `esp-idf-sys`'s build script self-provisions
clang, xtensa-gcc, and a Python venv under `firmware/.embuild`. Two host caveats:
a **fresh** ESP-IDF bootstrap needs **Python ≤ 3.12** (3.14 breaks ESP-IDF 5.3 —
the justfile prepends `firmware/tools/pyshim`, which maps `python3 → python3.12`),
and it needs **`ninja`** + **`ldproxy`** on `PATH`.

## Why `firmware/` is a detached workspace

The host workspace (`../../Cargo.toml`, members `led-core` (`domain/`),
`plant-core`, `esphome-api`) builds on **stable rustc for the host** and is what
`cargo test` runs. The firmware builds on the **`esp` fork for Xtensa**. Keeping
them in one workspace would force the Xtensa target and esp toolchain onto every
`cargo test`. So `firmware/Cargo.toml` declares its own `[workspace]` and the root
lists it under `exclude`. Firmware crates depend on the host domain crates **by
path** across the boundary (e.g. `plant-monitor` → `plant-core`). Dependencies
still point inward — the domain never knows the firmware exists.

## The firmware workspace (qhw.2)

`firmware/` is itself a workspace of small single-responsibility crates, each with
a `[package.metadata.hex-arch] role` tag (the gate that reads them lands in qhw.14):

| Crate | role | holds |
|---|---|---|
| `board-support` | `infra` | BSP: AXP192 power-on, pin map, bring-up |
| `firmware-infra` | `infra` | WiFi STA, mDNS, native-API host, OTA |
| `adapters` | `driven-adapter` | domain-port adapters (adc, st7789, clock, ws2812, wifi) |
| `bins/plant-monitor` | `composition-root` | bin #1 — the composition root |

The `std`/ESP-IDF stack is pinned per crate: `esp-idf-svc` 0.52, `esp-idf-hal`
0.46, `esp-idf-sys` 0.37, `embuild` 0.33 (build-dep) — see
[rust-on-esp-idf](../sources/rust-on-esp-idf.md).

## Build & flash

```sh
# Host domain — stable rustc, no device:
cargo test --workspace          #  or `just test`

# Firmware — Xtensa (from the repo root; recipes handle pyshim + sg/dialout):
just build                      #  cd firmware && cargo build --release
just run                        #  build + flash + monitor  (needs the board)
```

Serial traps and the pty-free monitor recipe:
[flashing-and-serial-access](flashing-and-serial-access.md).
