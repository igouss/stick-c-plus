---
id: claudepix
title: "ClaudePix — a library of 20×20 pixel creature animations"
type: asset-library
author: unattributed
publisher: claudepix.vercel.app (Vercel)
url: https://claudepix.vercel.app/
retrieved: 2026-07-09
license: "NONE STATED — see 'Licence' below. Frames vendored into plant-display; not cleared for redistribution."
material: ../../plant-display/gen/frames.json
seeds: []
---

## Citation

*ClaudePix — Pixel Animation Library*, v0.1. <https://claudepix.vercel.app/>.
13 presets on a 20×20 grid, 216 frames total. Retrieved 2026-07-09.

## Licence — unresolved, and load-bearing

The site states **no licence**. There is no `LICENSE` route (404), no repository
link, no copyright line, and no attribution anywhere in `index.html` or `app.js`.
The artwork is therefore vendored into this repo **without a grant to redistribute
it**. That is a decision to revisit before this project is published anywhere.

Nothing about the code we wrote around it is encumbered; the encumbrance is the 216
frames in `plant-display/gen/frames.json` and the `generated.rs` derived from them.

## What it is

Two families, which the site's own UI quietly distinguishes:

| Family | Presets | Palette | Storage |
|---|---|---|---|
| **engine** | 9 | 3 colours (empty / body / eye) | sparse patches over one base `CREATURE` grid, in `creature-engine.js` |
| **standalone** | 4 | up to 10 colours | own `PAL[]` + `FRAMES[]`, no shared engine |

The standalone four (`work_coding`, `dance_bounce_dj`, `dance_sway_dj`,
`dance_djmix`) carry the site's generic blurb *"Pixel creature animation preset."*
precisely because they export no metadata. That cosmetic detail is the tell for the
split, and the split matters: an extractor that assumes one family silently produces
the wrong art for the other (see the trap below).

## How the frames were obtained

A frame is not stored anywhere on the site — it is *computed* at page load by the
site's own JavaScript (`patch`, `shift`, `parseFrame` over a base grid). So the
frames were resolved by **executing that JavaScript**, once, outside this repo:
`creature-engine.js` plus each preset's inline `<script>`, run in a `node:vm` with a
DOM stub, one fresh context per preset. Reimplementing `patch`/`shift` in another
language was rejected — a reimplementation agrees with its author's assumptions, not
with the site.

The output is `plant-display/gen/frames.json`: explicit 20×20 grids of palette
indices, plus each frame's hold in milliseconds. It contains no code. **No
JavaScript lives in this repository**; `plant-display/gen/generate.clj` (babashka)
turns that JSON into `plant-display/src/sprite/generated.rs`.

### The trap, recorded because it produced a plausible wrong answer

Every engine preset ends with `window.PRESET = PRESET`. A later *standalone* preset
therefore still sees a live `PRESET` binding — the **previous creature's**. An
extractor that detects the family with `typeof PRESET` gets `work_coding` = whatever
engine preset ran before it, with well-formed frames, plausible counts, and no error.
It was caught only because two presets then hashed identically.

The family must be decided by the presence of the `<script src="creature-engine.js">`
tag. `generate.clj` re-asserts distinctness as a standing guard.

## Verification

Each preset's frames were hashed twice, by two independent paths:

1. the headless `node:vm` extraction that produced `frames.json`;
2. the same presets re-derived **inside a real browser** on the live site.

All 13 agreed. Those 16-hex-digit digests are frozen in `generate.clj`
(`verified-digests`), which refuses to emit `generated.rs` unless the vendored JSON
still hashes to them — so an edited or truncated copy fails the build rather than the
plant monitor. `just sprites-check` runs in `just ci`.

Separately, `plant-display`'s unit tests pin the decoded base creature against rows
transcribed from `creature-engine.js` itself, including row 10 — the one asymmetric
row, which is what catches a mirrored or transposed decode. A picture cannot catch
that: the creature is symmetric nearly everywhere, so a transposed decode still looks
like a creature.

## Regenerate

```sh
just sprites          # frames.json -> src/sprite/generated.rs (babashka)
just sprites-check    # fail if generated.rs is stale
just sprite-screens   # render all 13 to target/screens/sprites.png, and look
```

Re-fetching the upstream HTML is deliberately *not* a recipe here: it needs a
JavaScript runtime, which this project does not carry. `frames.json` is the vendored
copy of record, pinned by the digests above.

## What to read, and why

- `plant-display/gen/frames.json` — the copy: 13 presets, 216 frames, palette indices.
- `plant-display/src/sprite/mod.rs` — the on-device format: 4-bit indices, two per
  byte, 200 bytes a frame, 43 KB for the whole library; index 0 is transparent, and
  `frame_at` is the entire animation clock as a pure function of elapsed milliseconds.
