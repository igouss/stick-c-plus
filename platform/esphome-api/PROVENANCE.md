# esphome-api — provenance

The ESPHome native-API wire vocabulary, vendored so this project owns a
framework-free (no-tokio) copy of the prost message types and the
message-type-id registry.

## Source (pinned)

- **Proto**: `proto/api.proto` and `proto/api_options.proto`, taken verbatim
  from [`esphome/aioesphomeapi`](https://github.com/esphome/aioesphomeapi) at
  commit **`1e16d71420b987a3403875fc979aaaa343fcb398`** (`aioesphomeapi/`
  directory). aioesphomeapi is **MIT** (Copyright © 2018 Otto Winter); the
  license text is retained at `proto/LICENSE-aioesphomeapi`.
- **Never** `esphome/esphome`. The same `api.proto` lives there too, but that
  repository is **GPLv3**; taking it from aioesphomeapi keeps our provenance MIT.
- The proto targets ESPHome native-API version **1.14** — the `(major, minor)`
  aioesphomeapi's client negotiates (`connection.py`: `api_version_major=1,
  api_version_minor=14`). Exposed as `esphome_api::API_VERSION`.

## Generated types (committed, no build-time protoc)

`src/generated/api.rs` is the prost-generated Rust, produced once with
`prost-build` + `protoc` and **committed** so that no downstream build — CI or
the firmware's Xtensa build — needs `protoc`. There is deliberately **no
`build.rs`**. Do not hand-edit the generated file.

### Regenerating (only when re-pinning the proto)

```sh
# from a throwaway crate depending on prost-build = "0.14"
# (keep it aligned with this crate's runtime `prost` version):
#   prost_build::Config::new()
#       .out_dir("out")
#       .compile_protos(&["proto/api.proto"], &["proto"])?;
# then prepend the @generated header and copy out/_.rs -> src/generated/api.rs
```

Bump the commit SHA above, regenerate `src/generated/api.rs`, regenerate
`src/generated/ids.rs`, re-capture `tests/fixtures/message_ids.tsv` from the new
`core.py`, add the new `api-<major>-<minor>` feature, and let the golden test
confirm the three still agree.

## Id registry

`src/generated/ids.rs` exposes `MESSAGE_IDS: &[(u32, &str)]` — every message
paired with its wire type id, taken from the `option (id) = N` annotation on
each message in `api.proto` (extension field 1036 on `MessageOptions`, defined
in `api_options.proto`). Sorted by id. `esphome_api::message_id` /
`message_name` read from it.

## Golden test — two independent oracles

`tests/golden_ids.rs` asserts `MESSAGE_IDS` matches **both**:

1. the `option (id)` annotations re-parsed from the vendored `proto/api.proto`
   (guards `ids.rs` against drift or a hand edit), and
2. aioesphomeapi's own `MESSAGE_TYPE_TO_PROTO` table, captured from `core.py`
   into `tests/fixtures/message_ids.tsv` — a separate, hand-maintained oracle in
   Python.

Both currently list **148** messages and agree exactly. Two sources agreeing
proves faithfulness to the upstream; a round-trip against one source would only
prove self-consistency.

## Licensing of dependencies

- **prost** (runtime dependency): Apache-2.0 — compatible.
- **aioesphomeapi** proto: MIT — retained at `proto/LICENSE-aioesphomeapi`.
- **UbiHome/esphome-native-api**: MIT — retained at `vendor/LICENSE-UbiHome`.
  This crate was informed by UbiHome's approach but is generated clean-room from
  aioesphomeapi's MIT proto rather than forked from UbiHome's (tokio-based,
  edition-2024) sources; the license is retained regardless.
- **noise-rust-crypto** (Noise encryption, deferred to qhw.10): the Unlicense —
  MIT-compatible. Recorded here so the licensing decision is settled before that
  bead adds the dependency.

## Open item resolved

> *Does UbiHome ship committed prost `.rs` (no build-time protoc)?*

**No.** UbiHome gitignores its generated proto (`src/proto/.gitignore`) and
regenerates it via its `generator/` subcrate at build/publish time. **Worked
around**: this crate commits its own generated `src/generated/api.rs`, so no
`protoc` is required to build against it.
