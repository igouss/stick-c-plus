---
id: board-runs-factorytest-demo
title: "Our board ships running M5Stack's FactoryTest demo (unmodified)"
confidence: high
scope: board:m5stickc-plus
derived-from: [2026-07-03-identify-factory-firmware]
supersedes: []
reviewed: 2026-07-03
check: manual   # needs the board on /dev/ttyUSB0; recipe in the experiment
---

**Claim:** The app flashed on our M5StickC Plus (`app0 @0x10000`) is M5Stack's
**FactoryTest** example from [m5stack-m5stickc-plus](../sources/m5stack-m5stickc-plus.md),
not merely "an app built on the same library." It has never been reflashed.

**Evidence:**
[2026-07-03-identify-factory-firmware](../experiments/2026-07-03-identify-factory-firmware/README.md).
A live `app0` dump contains FactoryTest's three **random** BLE-UART UUIDs
(`1bc68b2a`/`1bc68da0`/`1bc68efe-f3e3-11e9-81b4-2a2ae2dbcce4`) plus its private
strings (`check Hardware`, `Test Mode`, `Drawdisplay`, `MicRecordfft`). Random
UUIDs appear only in this exact source; a coincidental match is impossible. Boot
banner `M5StickCPlus initializing...` and build fingerprint (`arduino-lib-builder`,
`Apr 20 2022`, IDF `v4.4.1`) corroborate.

**Holds when:** the board is the stock unit as received. Its binary was built
**Apr 2022** = the `0.0.7`-era FactoryTest (commit `9b1a17f`).

**Breaks when:** anyone flashes it — the moment we `cargo run` our firmware, this
is false. Re-confirm identity by re-running the experiment, not by memory.

**How to apply:** Treat the FactoryTest source as an accurate, verified reference
for how *this* board is configured (pins, power, peripheral init) — it's the
program that actually runs on it. If you want to preserve the factory image before
overwriting, `espflash read-flash 0x0 0x400000 factory-backup.bin` first (whole
flash, not just app0).
