# Handoff — a portrait layout for the orientation readout

**Bead:** `stick-c-plus-orientation-portrait-layout-xsy`
**Status:** ready, unblocked. Specifies *what*, not *how* — the constraints and seams below are
load-bearing and were paid for; the design is yours.

Read the bead first (`br show stick-c-plus-orientation-portrait-layout-xsy`). This file is the
context around it that is not in the tracker.

## Where this sits

The user asked for the orientation readout to rotate with the board, so it can be read in any
position. That was split into three beads:

1. `…-rotation-core-1wj` — **done** (commit `01341d8`). The pure decision: which quarter turn
   puts the picture's top toward the sky, and the settling that stops it spinning mid-turn.
2. `…-portrait-layout-xsy` — **this one.** The largest piece.
3. `…-rotate-panel-8mh` — blocked on this. Hardware MADCTL rotation and the wiring.

You need only bead 2. Do not start bead 3 in the same change; it has its own hazards (see the
last section) and deserves its own on-metal verification.

## What already exists

`orientation_core::ScreenRotation` — `Deg0` / `Deg90` / `Deg180` / `Deg270`, with
`is_portrait()` (true for the two quarter turns) and `opposite()`. `RotationSettler` decides
and holds the current rotation. All of it is pure, host-tested, and **already correct against
the board** — the `UpAxis → ScreenRotation` table was measured, not guessed. Don't re-derive
it; `rotation.rs` carries the measurement and the reasoning.

`Deg0` is the panel's native landscape, legible with the stick held horizontally, USB-C to the
right.

## The constraint that shapes everything

The panel is **240×135** with **`FONT_10X20`** (`platform_display::{SCREEN_SIZE, FONT}`).

- Landscape: 240 / 10 = **24 characters** per line, 6 lines.
- Portrait: 135 / 10 = **13 characters** per line, 12 lines.

Thirteen. The current landscape layout puts a 10-character face label and a 10-character angles
field *side by side* on one header line, and each axis row is a name + a 130 px bar + a
6-character value across the full width. None of it fits. `apps/orientation/orientation-display/src/layout.rs`
says so at compile time:

```rust
assert!(SCREEN_SIZE.width > SCREEN_SIZE.height, "the canvas is landscape once rotated");
```

This is why a transform wrapper is not an option and a second layout is the work.

## What has to hold

**The compile-time invariant block is the point of the current design.** `layout.rs` proves at
build time — on host *and* Xtensa — that no field runs off an edge, no bar collides with its
value, and the last row does not fall off the bottom. A portrait layout that can violate any of
those must fail the **build**, not the eye. Whatever shape you choose (a `Layout` value handed
to the renderer, two modules, a trait — your call), that guarantee survives in some form or the
change is not done.

**The 6-character milli-g field does not shrink.** `VALUE_WIDTH = 6` exists because `-8000` is
five characters at full scale and the sixth is headroom: truncating a reading into a
plausible-looking *smaller* number is the one thing this readout must never do. Abbreviate
anything else first.

**Erase-in-place.** The readout repaints without clearing, so every field is padded to a fixed
width and a shorter value must fully erase a longer predecessor. `screen.rs` has a test for
this (`a_smaller_reading_fully_replaces_a_larger_one`); portrait needs the equivalent.

**The `NO SIGNAL` state.** `OrientationView::label()` returns either the face name or
`"NO SIGNAL"` (9 chars), and a lost signal dims the whole readout to a quarter brightness. Both
layouts carry it. See `signal.rs` and the `Signal` handling in `screen.rs`.

Roughly what has to give in portrait: the header stacks onto two lines rather than one, and the
bars shorten to ~65 px. That is a sketch, not a spec.

## A gotcha that will bite in the first hour

`platform_display::testing::Framebuffer` is **hard-wired to `SCREEN_SIZE`** — 240×135, landscape
(`testing.rs:33`). Its `escaped()` counter is the entire "nothing is drawn off-screen"
guarantee, and it is the reason this crate does not use `SimulatorDisplay` (which silently
clips and returns `Ok`, making such a test unable to fail — the crate docs explain this).

Render a portrait layout into it and `escaped()` becomes meaningless: real pixels land outside
135 px of width and get counted as escapes. **The framebuffer needs to become size-aware**
before a portrait no-clipping test means anything. That is a `platform-display` change, so mind
the architecture boundary — it is a shared platform crate, not this app's.

`examples/screenshots.rs` uses `SimulatorDisplay` for PNGs and will need the same treatment for
portrait canvases.

## Verification

`just screens` renders every state to `target/screens/` through the *same* `render` the panel
calls — it is the review surface, and the reason a layout that looks wrong is caught by looking.
Ten screens today (six faces, tilted, tilted-but-screen-up, moving, no-signal). **Every quadrant
should land in the gallery**; a portrait screen that ships un-looked-at is the failure mode this
example exists to prevent.

Gherkin lives in `apps/orientation/orientation-core/tests/features/orientation.feature` (27
scenarios). Layout geometry is a display concern and belongs in unit tests, not there.

Gate before committing: **`just ci`**. It runs fmt, hex-lint (architecture roles + context
isolation), clippy with warnings-as-errors, the host suites, and both firmware builds.

## House rules that are easy to re-break

- **A green host gate does not mean the device works.** This is the project's most expensive
  failure mode and it has bitten twice recently. This bead is host-only work and *can* land on a
  green gate — but if you touch anything the panel executes, flash it and watch serial for a
  reboot loop and for `paint took … over the … tick budget` warnings. See
  `kb/findings/` and the memory on this.
- **10 ms is the hard floor** for any periodic thread period or yield fallback.
  `CONFIG_FREERTOS_HZ=100`, so a shorter sleep busy-waits instead of yielding and starves the
  idle task until the watchdog fires. `kb/findings/sub-tick-sleeps-busy-wait-on-esp-idf.md`.
- **Hexagonal, dependencies inward.** `orientation-display` is a domain crate: no framework, no
  hardware. `hex-lint` enforces roles and will fail the gate.
- **Explicit type annotations on every binding and lambda parameter**, matching the surrounding
  code. Read a neighbouring file before writing; the comment density and voice are consistent
  across this repo and worth matching.
- **Scoped commits** (https://scopedcommits.com/): `orientation: <imperative, lowercase>`, scope
  first, not a Conventional-Commits type. Close the bead as it lands.

## What is waiting behind you

Bead 3 (`…-rotate-panel-8mh`) rotates the panel in hardware via MADCTL (`mipidsi`
`set_orientation`) rather than with a software transform — a rotating `DrawTarget` maps rows
onto columns and destroys the `fill_contiguous` fast path that
`kb/findings/mipidsi-rectangle-fill-costs-an-address-window.md` exists to protect, at ~13 700
px/frame.

Its known hazard, so you can leave the seam in a helpful place: this panel's visible window sits
at **col 52 / row 40** in the native portrait orientation, and `firmware/platform/adapters/src/panel.rs`
carries a long comment about a fight between what the datasheet says the MADCTL bits mean and
what the glass actually did. **Those offsets are not the same in portrait and landscape.** A
runtime orientation change walks straight back into it, and a wrong offset shows as a picture
shifted by a few dozen pixels or a stripe of stale CGRAM down one edge. Framebuffer tests cannot
see any of that.

Also unresolved for that bead, worth keeping in mind: the render loop repaints on a *changed
view*, so the rotation has to reach it as part of the view — otherwise the first frame after a
turn is a correctly-rotated panel still holding the previous quadrant's pixels.

## One open item elsewhere

Not yours, but do not let it look verified: the `NO SIGNAL` state
(`…-orientation-no-signal-cly`, commit `358d9c0`) is proven host-side only. It was confirmed on
the board that it never *falsely* fires — 125 heartbeats on a healthy sensor, zero triggers —
but nobody has seen it actually appear on the glass. Doing so needs a throwaway build with a
deliberately failing IMU.
