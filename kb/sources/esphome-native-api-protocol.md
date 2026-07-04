---
id: esphome-native-api-protocol
title: "ESPHome native-API protocol + Home Assistant integration docs"
type: reference
author: ESPHome / Home Assistant projects
publisher: developers.esphome.io, esphome.io, home-assistant.io
url: https://developers.esphome.io/architecture/api/
retrieved: 2026-07-04
license: "docs © their projects (ESPHome: Apache-2.0 docs; HA: CC-BY-NC-SA). Reference links, not redistributed."
material: none    # web docs; cited by stable URL, not archived
seeds: []
---

## Citation

ESPHome & Home Assistant documentation on the native API and its Noise
encryption, consulted 2026-07-04. Stable entry points below.

## What it is

The prose specification behind [aioesphomeapi](aioesphomeapi.md) — what the wire
format *means*, in the projects' own words. Used to confirm the framing and
connection flow the code implies (the codec/FSM beads cross-checked against
these, not just against a round-trip).

## The pages

| Topic | URL |
|-------|-----|
| Native-API architecture + framing | <https://developers.esphome.io/architecture/api/> |
| API component (config, encryption) | <https://esphome.io/components/api/> |
| Noise encryption key + setup | <https://esphome.io/components/api/#configuration-variables> |
| HA ↔ ESPHome integration | <https://www.home-assistant.io/integrations/esphome/> |
| HA ESPHome device security / Noise | <https://www.home-assistant.io/integrations/esphome/#configuration> |

## Regenerate (the reproducibility primitive)

Web pages, cited by stable URL rather than archived (they track the current
protocol, which is what we want). When a **specific claim** is distilled into a
`kb/findings/` file, quote the sentence and the retrieval date there so the fact
is falsifiable even if the page moves — the derived-findings bead (qhw.24) owns
that. The machine-checkable ground truth stays [aioesphomeapi](aioesphomeapi.md).

## The framing, in one line

Plaintext frame = `0x00` preamble, varuint payload-size, varuint message-type,
payload — **one** length field, size **before** type. Not a 2-byte length (that
is the *encrypted* frame). Verified in `esphome-api/tests/golden_frames.rs`.
