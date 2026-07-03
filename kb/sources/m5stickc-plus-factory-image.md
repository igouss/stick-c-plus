---
id: m5stickc-plus-factory-image
title: "Full 4 MB factory flash image, read off our board before any reflash"
type: reference
author: M5Stack (factory image) — captured by us
publisher: —
url: —
retrieved: 2026-07-03
license: "M5Stack factory firmware (bootloader + FactoryTest + Espressif blobs). Personal restore backup of firmware on hardware we own — do NOT redistribute."
material: ./factory-full-4MB.bin
seeds: [board-runs-factorytest-demo]
sha256: 9544e3ff6e1ce0d51965974820a43dc1b54e8999ba6ac5586447dea1a81ae84a
---

## Citation

Whole-chip SPI-flash image (`0x0`–`0x400000`, 4,194,304 bytes) read from our
M5StickC Plus (MAC `DE:AD:BE:EF:00:01`, ESP32 rev v1.1) on 2026-07-03, while it
still ran the stock factory demo. This file, not an upstream URL, is the artifact.

## What it is

A **complete, restorable** copy of what M5Stack shipped on this board: 2nd-stage
bootloader (`0xE9` magic @ 0x0), partition table (`0x50AA` @ 0x8000), nvs, otadata,
`app0` (the FactoryTest demo — verified: contains its BLE UUID and boot banner),
and spiffs. Identity proven in experiment
[2026-07-03-identify-factory-firmware](../experiments/2026-07-03-identify-factory-firmware/README.md);
what the demo *is* → [board-runs-factorytest-demo](../findings/board-runs-factorytest-demo.md).

## ⚠ This source is NOT re-fetchable — unlike every other source here

The [datasheets](m5stickc-plus-datasheets.md) re-download and the
[library](m5stack-m5stickc-plus.md) is a public repo, but **this image exists only
because the board was still stock.** The first time we `cargo run` our firmware,
`app0`/otadata are overwritten and the original is gone. After that, **this file is
the only copy** — you restore *from* it, you cannot regenerate it. Treat it as
write-once evidence. (Committed to git for exactly this reason; see `.gitignore`.)

Integrity: `sha256sum -c SHA256SUMS` in this directory.

## Regenerate — only while the board is still stock

```sh
/usr/bin/sg dialout -c 'espflash read-flash -p /dev/ttyUSB0 -c esp32 0x0 0x400000 factory-full-4MB.bin'
# Serial traps: ../../guides/flashing-and-serial-access.md
```

## Restore (put the factory demo back)

```sh
# Full-chip restore from the backup:
/usr/bin/sg dialout -c 'espflash write-bin -p /dev/ttyUSB0 -c esp32 0x0 factory-full-4MB.bin'
# Verify it took (should reprint the FactoryTest UUID etc.):
../../experiments/2026-07-03-identify-factory-firmware/read-and-verify.sh
```

## Durability caveat

This repo has **no git remote** — committing puts the image in history but still on
one disk. Real off-machine durability needs a push (add a remote) or an external
copy (NAS/backup). Do that before relying on it.
