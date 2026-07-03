---
id: 2026-07-03-identify-factory-firmware
title: "Is our board actually running M5Stack's FactoryTest demo?"
date: 2026-07-03
domain: [esp32, firmware, provenance]
status: confirmed
hardware: "M5StickC Plus @ /dev/ttyUSB0, FT232, ESP32 rev v1.1, MAC DE:AD:BE:EF:00:01"
artifacts: ./read-and-verify.sh
findings: [board-runs-factorytest-demo]
source: [m5stack-m5stickc-plus]
---

## Question

The [m5stack-m5stickc-plus](../../sources/m5stack-m5stickc-plus.md) repo is the
*named* factory demo. Is that program actually the one flashed on our board — or
did we just match a repo by name?

## Hypothesis

If it's FactoryTest, the flashed `app0` will contain strings that exist **only** in
FactoryTest's source, not in the base M5StickCPlus library — most decisively its
three hard-coded **random** BLE-UART UUIDs. A library banner alone would be
consistent with any app built on the library, so it can't settle it; the UUIDs can.

## Method (reproducible)

Board on `/dev/ttyUSB0` (FT232). Serial access has three traps — see
[flashing-and-serial-access](../../guides/flashing-and-serial-access.md). Read the
3 MB `app0` partition (read-only, non-destructive) and grep for FactoryTest-only
strings pulled from `examples/FactoryTest/FactoryTest.ino`:

```sh
/usr/bin/sg dialout -c 'espflash read-flash -p /dev/ttyUSB0 -c esp32 0x10000 0x300000 app0.bin'
grep -a -c "1bc68b2a-f3e3-11e9-81b4-2a2ae2dbcce4" app0.bin   # the unique service UUID
```

`./read-and-verify.sh` runs the full dump + grep set. `app0.bin` is gitignored
(3 MB, re-fetchable). Toolchain: espflash 4.4.0, default 115200 baud (this FT232
fails higher).

## Raw results

Live `app0.bin` (3,145,728 bytes) string counts:

```
1  1bc68b2a-f3e3-11e9-81b4-2a2ae2dbcce4   (BLE service UUID — FactoryTest only)
1  1bc68da0-f3e3-11e9…                    (RX characteristic UUID)
1  1bc68efe-f3e3-11e9…                    (TX characteristic UUID)
1  check Hardware
1  Test Mode
1  BMP8563 RTC Time
1  Bat Vol error
1  Drawdisplay
1  MicRecordfft
1  M5StickCPlus initializing
0  MicroPhone                             (source has it split/cased differently)
```

Build fingerprint strings in the image:

```
Apr 20 2022
arduino-lib-builder
v4.4.1-1-gb8050b365e
```

Board id (from `espflash board-info`): ESP32 rev v1.1, 4 MB, MAC `DE:AD:BE:EF:00:01`.

## Verdict

**Confirmed.** Ten of eleven distinctive strings present, including all three
random BLE UUIDs — which occur nowhere but this exact FactoryTest source, so the
match is not a coincidence. This *is* the FactoryTest app.
Build date `Apr 20 2022` places the flashed binary at the `0.0.7`-era FactoryTest
(commit `9b1a17f`); the submodule is at HEAD (`0.1.1`) — same program, cosmetic
drift only. → [board-runs-factorytest-demo](../../findings/board-runs-factorytest-demo.md).

## Threats to validity

- *A string match isn't a binary match.* True, but random UUIDs are effectively a
  fingerprint — the risk of a false positive is negligible, and multiple
  independent strings all hit.
- *Could be a fork/re-build.* The build fingerprint (`arduino-lib-builder`,
  Apr 2022) is consistent with an official M5 build, and the board is stock. A
  hostile fork reusing the UUIDs is not a threat model here.
- *Cached/stale dump?* The read went to the metal each time (board reset via
  DTR/RTS, MAC confirmed live); no cached image involved.
