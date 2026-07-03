---
id: esp-rust-toolchain
title: "The esp Rust toolchain for Xtensa, and why the firmware is a detached workspace"
kind: guide
scope: chip:esp32
reviewed: 2026-07-03
distils: [m5stack-m5stickc-plus]
---

The ESP32 on this board is **Xtensa LX6**, not RISC-V, and Xtensa has no upstream
Rust target. You need Espressif's **`esp` rustc fork**.

## Setup

```sh
# Install the esp fork + Xtensa toolchain via espup:
cargo install espup
espup install --targets esp32

# Source the env in EVERY shell that builds firmware (exports the esp toolchain,
# clang, and the Xtensa target):
source ~/export-esp.sh

# Flasher:
cargo install espflash
```

- Target triple: **`xtensa-esp32-none-elf`** (`no_std`).
- `firmware/rust-toolchain.toml` selects the `esp` channel; `firmware/.cargo/config.toml`
  sets the target and the `espflash flash --monitor` runner.

## Why `firmware/` is a detached workspace

The host workspace (`../../Cargo.toml`, member `domain`) builds on **stable rustc
for the host** and is what `cargo test` runs. The firmware builds on the **`esp`
fork for Xtensa**. Keeping them in one workspace would force the Xtensa target and
esp toolchain onto every `cargo test`. So `firmware/Cargo.toml` declares its own
`[workspace]` and the root `Cargo.toml` lists it under `exclude`. The firmware
depends on `domain` by path. Dependencies still point inward — the domain never
knows the firmware exists.

## esp-hal version: 1.1 (the smartled ceiling is gone)

The firmware pins **`esp-hal 1.1`**; the RMT peripheral needs its **`unstable`**
feature. We used to be capped at 1.0.x because `esp-hal-smartled 0.17` requires
`esp-hal ~1.0` — but the WiFi/OTA stack (`esp-radio`, `esp-rtos`, `esp-storage`)
requires `~1.1`, a disjoint range. So we **dropped smartled** and own the WS2812
RMT encoder in-tree (`firmware/src/adapters/ws2812.rs`). Full compatibility matrix
and the pinned OTA crate set:
[esp-rs-ota-version-matrix](../findings/esp-rs-ota-version-matrix.md).

## Build & flash

```sh
# Domain — host, stable, no device:
cargo test -p led-core

# Firmware — Xtensa:
source ~/export-esp.sh
cd firmware && cargo build --release
cargo run --release        # flash + monitor; serial traps in
                           # ../guides/flashing-and-serial-access.md
```
