---
id: ubihome-esphome-native-api
title: "UbiHome/esphome-native-api — a Rust native-API server (design reference)"
type: repo
author: UbiHome
publisher: UbiHome (github.com/UbiHome/esphome-native-api)
url: https://github.com/UbiHome/esphome-native-api
retrieved: 2026-07-04
license: MIT
material: ./ubihome-esphome-native-api/    # git submodule, pinned 79c5066
seeds: []
---

## Citation

UbiHome. *esphome-native-api — an ESPHome native-API server in Rust.* GitHub,
<https://github.com/UbiHome/esphome-native-api>, pinned commit
`79c5066b1abc417462dbbf571ba25dc84b8e22ee`. MIT. Retrieved 2026-07-04.

## What it is

A Rust implementation of the **server** (device) half of the ESPHome native API
— the same role this project's firmware plays. Kept as a **design reference**,
not a dependency: our `esphome-api` crate is generated clean-room from
aioesphomeapi's MIT proto rather than forked from UbiHome (which is tokio-based,
edition-2024, and gitignores its generated proto — see `esphome-api/PROVENANCE.md`).
The MIT licence is retained regardless (`esphome-api/vendor/LICENSE-UbiHome`).

Its highest value here is the **Noise encryption** path for qhw.10: it is one of
the few worked examples of the NNpsk0 handshake on the device side in Rust.

## Regenerate (the reproducibility primitive)

```sh
git submodule update --init kb/sources/ubihome-esphome-native-api
git -C kb/sources/ubihome-esphome-native-api checkout 79c5066b1abc417462dbbf571ba25dc84b8e22ee
```

## What to read, and why

- `documentation/docs/native_api/encryption.md` — the encryption doc the bead
  calls out: Noise parameters and key handling, prose form.
- `src/packet_encrypted.rs`, `examples/encrypted_server.rs` — a device-side
  NNpsk0 handshake in Rust, the qhw.10 reference.
- `src/` (plaintext packet/codec) — a second opinion on framing, to cross-check
  against aioesphomeapi and our own codec.
