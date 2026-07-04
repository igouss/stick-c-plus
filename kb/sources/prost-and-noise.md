---
id: prost-and-noise
title: "prost + the Noise stack (noise-protocol, noise-rust-crypto, NNpsk0 spec)"
type: reference
author: prost authors; Guanhao Yin (noise-rust); Trevor Perrin (Noise spec)
publisher: docs.rs, github.com/blckngm/noise-rust, noiseprotocol.org
url: https://noiseprotocol.org/noise.html
retrieved: 2026-07-04
license: "prost: Apache-2.0; noise-protocol/noise-rust-crypto: Unlicense/MIT; Noise spec: public."
material: none    # crates by pinned version + spec URL
seeds: []
---

## Citation

The protobuf and Noise-encryption crates, and the Noise Protocol Framework
specification, consulted 2026-07-04. References for the wire types (already in
use) and the encryption follow-on (qhw.10).

## What it is

- **prost** — the protobuf codegen/runtime the `esphome-api` crate uses for the
  native-API message types (no build-time `protoc`; the `.rs` is committed).
- **noise-protocol / noise-rust-crypto** — a Rust Noise implementation, the
  planned dependency for the native-API Noise transport (qhw.10). Licence
  (Unlicense/MIT-compatible) already settled in `esphome-api/PROVENANCE.md`.
- **The Noise spec** — the framework the ESPHome handshake instantiates as
  **`Noise_NNpsk0_25519_ChaChaPoly_SHA256`** (device = responder), the exact
  parameters qhw.10 must implement.

## The references (pinned versions)

| Source | Version / ref | Where |
|--------|---------------|-------|
| `prost` (runtime) | **0.14.4** | <https://docs.rs/prost/0.14.4> |
| `noise-protocol` | **0.2.1** | <https://docs.rs/noise-protocol/0.2.1> |
| `noise-rust-crypto` | **0.6.2** | <https://docs.rs/noise-rust-crypto/0.6.2> |
| Noise Protocol Framework (rev 34) | rev 34 | <https://noiseprotocol.org/noise.html> |

The concrete ESPHome Noise parameters are cross-checked against
[aioesphomeapi](aioesphomeapi.md) `connection.py` and
[UbiHome](ubihome-esphome-native-api.md) `packet_encrypted.rs`.

## Regenerate (the reproducibility primitive)

Crates pinned by version in the relevant `Cargo.toml`; the Noise spec is a stable
public document cited by URL + revision.
