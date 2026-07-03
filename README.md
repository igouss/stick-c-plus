# stick-c-plus — a NightDriver-style LED driver in Rust

An addressable-LED (WS2812) animation driver for the **M5StickC Plus**
(ESP32, Xtensa LX6), written in `no_std` Rust and inspired by
[NightDriverStrip](https://github.com/PlummersSoftwareLLC/NightDriverStrip).

## Architecture — hexagonal / ECB

The domain is framework-free and dependencies point inward. The heart of the
system (colors, effects, the render use case) never mentions hardware, so it is
verified on the host — Gherkin, property, and unit tests — with no device attached.

```
stick-c-plus/
├─ domain/            # led-core: pure no_std, host-tested. THE system.
│  ├─ src/color.rs    #   Entities  — Rgb, Hsv, hsv_to_rgb
│  ├─ src/effect.rs   #   Entities  — Effect + SolidColor, Rainbow (pure animations)
│  ├─ src/animator.rs #   Control   — Animator: render → push, over ports
│  ├─ src/ports.rs    #   Ports     — Clock, LedOutput (implemented outward)
│  └─ tests/          #   Gherkin plumbing (cucumber) over the effects
├─ firmware/          # stick-led-firmware: the Xtensa boundary. Detached workspace.
│  ├─ src/main.rs             #   Composition root — wire adapters, run the loop
│  ├─ src/adapters/clock.rs   #   Boundary — Clock  via esp-hal monotonic timer
│  └─ src/adapters/strip.rs   #   Boundary — LedOutput via RMT WS2812 (smart-leds)
└─ kb/                # Knowledge base — board facts, sources, findings (kbe-style)
   ├─ sources/        #   Raw: datasheets + the M5 factory firmware (submodule)
   ├─ experiments/    #   Raw: what we probed on the metal (+ results)
   ├─ findings/       #   Derived: falsifiable board facts, one per file
   └─ guides/         #   Derived: toolchain, flashing, pin map
```

`kb/` is a [`~/kbe`](../../../kbe/README.md)-style knowledge base for everything we
learn about this board — cited sources, on-device experiments, and the findings
distilled from them. It never compiles into the firmware; the boundary only
*mirrors* what the hardware sources teach. Its headline source is M5Stack's shipped
**FactoryTest** app ([`m5stack/M5StickC-Plus`](https://github.com/m5stack/M5StickC-Plus),
a pinned submodule) — the AXP192 / ST7789 bring-up we port into `firmware/`,
**verified to be the exact app on our board**. Start at [`kb/INDEX.md`](kb/INDEX.md).
Fresh checkouts: `git submodule update --init`.

The two build under **different toolchains**, on purpose:

| Crate      | Toolchain | Target                    | Tested |
|------------|-----------|---------------------------|--------|
| `led-core` | stable    | host                      | yes — `cargo test` |
| firmware   | `esp` fork| `xtensa-esp32-none-elf`   | on device |

`firmware/` is its own workspace (`[workspace]` + root `exclude`) so its Xtensa
target and `esp` toolchain never touch `cargo test` at the root.

## Prerequisites

- The `esp` rustc fork + Xtensa toolchain, via [`espup`](https://github.com/esp-rs/espup):
  `espup install --targets esp32`, then `source ~/export-esp.sh` in each shell.
- [`espflash`](https://github.com/esp-rs/espflash) for flashing.

## Build & test

```sh
# Domain — host, stable, no device needed:
cargo test -p led-core

# Firmware — Xtensa (source the esp env first):
source ~/export-esp.sh
cd firmware
cargo build --release
```

## Flash & run

Connect the board (appears as `/dev/ttyUSB0`), then:

```sh
source ~/export-esp.sh
cd firmware
cargo run --release            # espflash flash --monitor (see .cargo/config.toml)
# or explicitly:
espflash flash --monitor --port /dev/ttyUSB0 target/xtensa-esp32-none-elf/release/stick-led-firmware
```

## Hardware wiring

Data line defaults to **GPIO32** (M5StickC Plus Grove port); strip length is
`LED_COUNT` in `firmware/src/main.rs`. Set both to match your strip.

## Roadmap

More effects (comet, fire, palette cycling), a button-driven effect selector
(M5 button on GPIO37), and optional Wi-Fi control — each an effect in the domain
or an adapter at the boundary, never a change to the other.
