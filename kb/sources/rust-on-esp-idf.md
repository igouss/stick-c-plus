---
id: rust-on-esp-idf
title: "Rust on ESP-IDF (std) — the book, template, and esp-idf-* crates"
type: reference
author: esp-rs working group (Espressif)
publisher: esp-rs (github.com/esp-rs), docs.rs
url: https://docs.esp-rs.org/book/
retrieved: 2026-07-04
license: "esp-rs crates: MIT OR Apache-2.0; the book: Apache-2.0/MIT. Reference links."
material: none    # crates by pinned version + docs URLs
seeds: []
---

## Citation

The esp-rs `std`/ESP-IDF stack, consulted 2026-07-04. These are the references
for the std/ESP-IDF pivot (qhw.1, qhw.7, qhw.8, qhw.12, qhw.15).

## What it is

The toolchain and crate set the firmware moves onto in the std pivot (superseding
the no_std esp-hal stack — cross-link [esp-rs-ota-version-matrix](../findings/esp-rs-ota-version-matrix.md)).
`esp-idf-sys` builds/links ESP-IDF; `esp-idf-hal` and `esp-idf-svc` wrap its HAL
and services (WiFi, mDNS, OTA, NVS) as safe Rust.

## The references (pinned versions)

| Source | Version / ref | Where |
|--------|---------------|-------|
| The Rust on ESP book (std chapters) | live | <https://docs.esp-rs.org/book/> |
| `esp-idf-template` (project skeleton) | `master` | <https://github.com/esp-rs/esp-idf-template> |
| `esp-idf-sys` (ESP-IDF bindings + build) | **0.37.2** | <https://docs.rs/esp-idf-sys/0.37.2> |
| `esp-idf-hal` (peripherals: ADC, RMT, I2C, SPI) | **0.46.2** | <https://docs.rs/esp-idf-hal/0.46.2> |
| `esp-idf-svc` (WiFi, mDNS, OTA, NVS, HTTP) | **0.52.1** | <https://docs.rs/esp-idf-svc/0.52.1> |

Versions are the qhw.1 build targets (esp-idf-svc 0.52 / -hal 0.46 / -sys 0.37,
ESP-IDF v5.3.x). `esp-idf-sys` build knobs (`ESP_IDF_VERSION`, `sdkconfig.defaults`,
embuild) are documented in its README and the book's "Configuration" chapter.

## Regenerate (the reproducibility primitive)

Crates are pinned by version in `firmware/Cargo.toml` once qhw.1 lands; the book
and template are cited live. `cargo doc -p esp-idf-svc --open` for the local API.
