---
id: flashing-and-serial-access
title: "Flashing and serial access to the M5StickC Plus from this host"
kind: guide
scope: project:stick-c-plus
reviewed: 2026-07-03
distils: [2026-07-03-identify-factory-firmware]
---

How the board is reached from the Fedora host, and the three traps that make a
working `espflash` look broken. All verified 2026-07-03.

## The board

Reaches the host as **`/dev/ttyUSB0`** via an **FTDI FT232** (USB `0403:6001`) —
not the usual CP2104. DTR/RTS auto-reset is wired, so `espflash` connects via the
flash stub with default reset. Chip: ESP32 rev v1.1, 4 MB, dual-core 240 MHz,
MAC `DE:AD:BE:EF:00:01`.

Flash layout (stock M5 OTA map): `nvs @0x9000`, `otadata @0xe000`,
**`app0 @0x10000` (3 MB)**, `spiffs @0x310000`.

## Trap 1 — the agent shell lacks the `dialout` group

The *account* is in `dialout`, but a shell spawned before that grant doesn't carry
the group in its supplementary set, so `espflash` fails with
`Failed to open serial port … Permission denied` — **misleading**: it's `EACCES`,
not a busy port (`fuser`/`lsof` show it free). Fix without sudo — run every serial
command through the group:

```sh
/usr/bin/sg dialout -c 'espflash board-info -p /dev/ttyUSB0 -c esp32'
```

## Trap 2 — `sg` is shadowed by `ast-grep`

On this host `sg` resolves to `ast-grep` (an alias), which swallows the command
with `unrecognized subcommand 'dialout'`. Always call the real binary by full
path: **`/usr/bin/sg`** (→ `newgrp`).

## Trap 3 — this FT232 fails baud > 115200

`-B 460800` / `-B 921600` connect via the stub and then die with
`Error while connecting to device` (after a "higher than 115,200 can cause issues"
warning). Use the **default 115200**. A full 3 MB `app0` read then takes ~4–5 min.

> ModemManager is active on this host but was *not* the cause — it holds no modem
> and the port scans clean. The symptom above is always one of the three traps.

## Recipes

```sh
# Board info (quick connectivity check):
/usr/bin/sg dialout -c 'espflash board-info -p /dev/ttyUSB0 -c esp32'

# Read the app partition off the board (read-only; non-destructive):
/usr/bin/sg dialout -c 'espflash read-flash -p /dev/ttyUSB0 -c esp32 0x10000 0x300000 app0.bin'

# Monitor (no flash). `--non-interactive` skips espflash's crossterm input
# reader, so no controlling TTY is needed — it streams serial to stdout. (The
# old `script -qec … /dev/null` pty shim is no longer required.)
/usr/bin/sg dialout -c 'espflash monitor -p /dev/ttyUSB0 -c esp32 --non-interactive'

# Flash our firmware — from firmware/, no esp env to source (esp-idf-sys
# self-provisions the toolchain; see ../guides/esp-rust-toolchain.md):
#   cargo run --release -p plant-monitor   # espflash flash --monitor, per .cargo/config.toml
#   (or `just run` from the repo root, which also handles the sg/pyshim wrapping)
```

Only one process may own the port at a time. The board now runs **our**
firmware — the `plant-monitor` std/ESP-IDF boot skeleton (qhw.2), not the stock
FactoryTest demo. The factory image is preserved and restorable from
[m5stickc-plus-factory-image](../sources/m5stickc-plus-factory-image.md); the
original fingerprinting is recorded in
[board-runs-factorytest-demo](../findings/board-runs-factorytest-demo.md).
