---
id: aioesphomeapi
title: "aioesphomeapi — the ESPHome native-API client (proto + Noise oracle)"
type: repo
author: Otto Winter and the ESPHome project
publisher: esphome (github.com/esphome/aioesphomeapi)
url: https://github.com/esphome/aioesphomeapi
retrieved: 2026-07-04
license: MIT
material: ./aioesphomeapi/    # git submodule, pinned 1e16d71
seeds: []
---

## Citation

ESPHome. *aioesphomeapi — Python client for the ESPHome native API.* GitHub,
<https://github.com/esphome/aioesphomeapi>, pinned commit
`1e16d71420b987a3403875fc979aaaa343fcb398` (native-API **1.14**). MIT
(© 2018 Otto Winter). Retrieved 2026-07-04.

## What it is

The **reference implementation** of the client half of the ESPHome native API —
the code Home Assistant actually speaks to a device. It is this project's
authoritative oracle on three fronts:

- **`aioesphomeapi/api.proto`** — the wire message definitions and their
  `option (id)` type ids. This is the exact source the `esphome-api` crate
  vendored (`esphome-api/proto/api.proto`, same commit); see
  `esphome-api/PROVENANCE.md`.
- **`aioesphomeapi/_frame_helper/`** (`packets.py`, `plain_text.py`) — the
  plaintext frame format: preamble `0x00`, varuint length, varuint type, payload.
  The `esphome-api` frame codec is checked byte-for-byte against frames this code
  emits (`esphome-api/tests/golden_frames.rs`).
- **`aioesphomeapi/connection.py`** — the connection handshake and the **Noise**
  parameters, the oracle for the connection FSM (qhw.19) and the encryption
  follow-on (qhw.10).

Pinned to the same commit as the vendored proto so the crate and its oracle can
never drift apart.

## Regenerate (the reproducibility primitive)

```sh
git submodule update --init kb/sources/aioesphomeapi
git -C kb/sources/aioesphomeapi checkout 1e16d71420b987a3403875fc979aaaa343fcb398
```

## What to read, and why

- `aioesphomeapi/api.proto` — every native-API message + its type id (the
  vendored source).
- `aioesphomeapi/_frame_helper/packets.py` — `make_plain_text_packets`: the
  frame wrapper, in ~15 lines.
- `aioesphomeapi/connection.py` — `_connect_hello_login`, `_process_hello_resp`:
  the Hello → (optional auth) → device-info/list/subscribe flow, and the Noise
  handshake for qhw.10.
