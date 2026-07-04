---
id: esp-rs-ota-version-matrix
title: "The WiFi/OTA esp-rs stack needs esp-hal 1.1 — esp-hal-smartled's ~1.0 pin is the wall, and esp-wifi/esp-hal-embassy are superseded by esp-radio/esp-rtos"
confidence: high
scope: project:stick-c-plus
derived-from: []
supersedes: []
reviewed: 2026-07-03
check: manual   # re-query crates.io; the version table below drifts — recipe at foot
---

> **⚠️ SUPERSEDED (2026-07-04) by the std/ESP-IDF pivot.** This finding maps the
> **no_std esp-hal** WiFi/OTA stack — the path **not** taken. The firmware now
> builds `std` on ESP-IDF, where WiFi comes from `esp-idf-svc` and OTA from its
> `EspOta` (write/flip/rollback, qhw.12) — so none of the `esp-hal 1.1` /
> `esp-radio` / `esp-rtos` constraints below apply. Kept as the record of why the
> no_std stack looked the way it did (epic `stick-c-plus-qqh`, retired). Current
> stack: [rust-on-esp-idf](../sources/rust-on-esp-idf.md).

**Claim:** To add WiFi + OTA to this firmware you must move to **`esp-hal 1.1`**,
and that forces dropping **`esp-hal-smartled`** — it pins `esp-hal ~1.0`
(`>=1.0.0, <1.1.0`), which is *disjoint* from what the radio/OTA crates require
(`~1.1`). There is no version of the two stacks that co-resolves. Separately, the
crates you would reach for from memory have been **renamed/superseded**:
`esp-wifi → esp-radio`, `esp-hal-embassy → esp-rtos`. The old names are frozen on
the 1.0 line (last touched Oct 2025) and are the legacy path now.

**Evidence:** crates.io, queried 2026-07-03. Dependency constraints are the
`esp-hal` requirement each crate declares:

| Crate | Latest | Requires `esp-hal` | Cluster |
|---|---|---|---|
| `esp-hal` | **1.1.1** | — | keystone |
| `esp-hal-smartled` | 0.17.0 (2025-11) | `~1.0`  ❌ | caps at 1.0 |
| `esp-radio`  (was `esp-wifi`) | 0.18.0 / 1.0.0-beta.0 | `~1.1` | WiFi/BLE |
| `esp-rtos`   (was part of `esp-hal-embassy`) | 0.3.0 | `~1.1` | scheduler |
| `esp-storage` | 0.9.0 | `~1.1` | flash R/W |
| `esp-bootloader-esp-idf` | 0.5.0 | (standalone) | partitions + `otadata` + rollback |
| `esp-alloc` | 0.10.0 | — | heap (WiFi/OTA need it) |
| `esp-backtrace` / `esp-println` | 0.19.0 / 0.17.0 | (via `esp-config 0.7`) | already on the 1.1 side |

The legacy cluster (`esp-wifi 0.15.1`, `esp-hal-embassy 0.9.1`) still resolves
`esp-hal ~1.0.0-rc.0` **and** an *older* embassy line (`embassy-sync ^0.6`,
`esp-config ^0.5`) that itself conflicts with the 1.1 cluster
(`embassy-sync ^0.8`, `esp-config ^0.7`) — so you cannot half-migrate. It is all
of one cluster or all of the other.

**Resolution (taken 2026-07-03):** own the WS2812 RMT encoder in-tree
(`firmware/src/adapters/ws2812.rs`, implements the domain `LedOutput` port over
the `esp-hal` RMT peripheral), drop `esp-hal-smartled` + `smart-leds`, and pin
`esp-hal = "1.1"`. The firmware builds warning-free on the `esp` toolchain at
1.1.1. This also removes a whole framework dependency from dictating the
firmware's `esp-hal` floor — the LED PHY is now a boundary adapter we control.

**The pinned OTA set** (all latest as of 2026-07-03; add as each bead lands, not
all at once):

```
esp-hal                = "1.1"           # esp32, unstable
esp-radio              = "0.18"          # esp32, wifi, unstable, esp-alloc
esp-rtos               = "0.3"           # esp32, embassy, esp-radio
esp-storage            = "0.9"           # esp32
esp-bootloader-esp-idf = "0.5"           # esp32, validation (rollback)
esp-alloc              = "0.10"
embassy-executor       = "0.10"
embassy-net            = "0.9"
embassy-time           = "0.5"
embassy-sync           = "0.8"
```

**Holds when:** you want WiFi, BLE, esp-now, or OTA on this board — any of those
pull `esp-radio`/`esp-rtos`/`esp-storage`, all `~1.1`.

**Breaks when:** `esp-hal-smartled` publishes an `esp-hal 1.1`-compatible release
(then the conflict is gone and smartled becomes an *option* again — but owning the
encoder is still the cleaner boundary). Also re-check whenever `esp-radio` leaves
beta: 0.18.0 is `max_stable`, 1.0.0-beta.0 wants `esp-hal ~1.1.0` (stable) — pin
the stable line unless a needed feature is beta-only.

**How to apply:** treat `esp-hal 1.1` as the keystone; pick crates from the 1.1
cluster only; reach for `esp-bootloader-esp-idf` (official) for `otadata`/rollback,
not a community OTA crate. Tracked as the **OTA-over-WiFi** epic in beads
(`stick-c-plus-qqh`). Re-verify the table:

```sh
UA="you@example.com"
for c in esp-hal esp-radio esp-rtos esp-storage esp-bootloader-esp-idf; do
  curl -s -H "User-Agent: $UA" "https://crates.io/api/v1/crates/$c" \
    | jq -r --arg n "$c" '.crate | "\($n): \(.max_stable_version) (newest \(.newest_version))"'
done
```
