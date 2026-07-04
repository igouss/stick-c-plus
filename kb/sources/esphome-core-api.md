---
id: esphome-core-api
title: "esphome/esphome — components/api (GPLv3, REFERENCE-ONLY, never copy)"
type: repo
author: ESPHome project
publisher: esphome (github.com/esphome/esphome)
url: https://github.com/esphome/esphome/tree/dev/esphome/components/api
retrieved: 2026-07-04
license: "GPL-3.0 — REFERENCE ONLY. Do NOT copy code or generated artifacts into this MIT/Apache project."
material: none    # deliberately NOT vendored — see below
seeds: []
---

## ⚠️ Licence boundary

`esphome/esphome` is **GPL-3.0**. The same `api.proto` and the C++ native-API
server live here too, but taking anything from this repo would make our code a
derivative work under the GPL. **This project is MIT/Apache-2.0.** So this repo
is a **read-only behavioural reference**: consult it to understand *what* the
firmware does, then implement independently from the MIT sources
([aioesphomeapi](aioesphomeapi.md), [UbiHome](ubihome-esphome-native-api.md)).

- **Never** copy code, comments, or generated files from here.
- **Never** vendor it as a submodule (hence `material: none`).
- Our proto comes from **aioesphomeapi (MIT)**, not from here — see
  `esphome-api/PROVENANCE.md`.

## Citation

ESPHome. *esphome — components/api.* GitHub,
<https://github.com/esphome/esphome/tree/dev/esphome/components/api>. GPL-3.0.
Consulted 2026-07-04.

## What it is

The canonical device-side native-API server (C++): `api_connection.cpp`,
`api_frame_helper.cpp`, `api.proto`. The ground truth for edge cases the Python
client and the docs leave implicit — connection state machine, the
`ListEntitiesDone` boundary, Noise framing. Read to resolve ambiguity; write
from the MIT sources.

## Regenerate (the reproducibility primitive)

Nothing to reproduce locally — this note is a pointer, on purpose. Browse the
canonical `dev` branch at the URL above; pin a commit in a citation if a specific
behaviour is quoted in a finding.
