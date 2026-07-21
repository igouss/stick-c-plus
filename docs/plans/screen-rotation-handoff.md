# Handoff — the picture always faces the reader

**Epic:** `stick-c-plus-screen-rotation-platform-ce1`
**Plan:** `docs/plans/screen-rotation-platform-capability.md` — the verified inventory, the
architectural decision, and the hazards. Read it after this file.

Two of nine beads are done. This file is the context around the rest that is not in the
tracker.

---

## Part 1 — The idea

This is the user's, in their words. It is the thing being built; everything else is
implementation.

> "when I change the orientation, I want to make it easier for me to read the text and graphs
> and whatever is on the UI to always face me and be easy to read, i.e when it's pointing up
> or down a different version of the view that the current application is displaying should be
> displayed. I.e views should react to orientation change."

On how wide that reaches — asked explicitly, answered explicitly: **a platform capability,
all four apps** (orientation, pomodoro, plant-monitor, host-monitor), not a feature of the
orientation app.

On pacing, given the size of that:

> "this should be a capability they should be aware of, and their code modified to handle the
> rotation cases, but for now keep the existing behaviour i.e don't update views to handle
> rotation changes."

Read that as: **make the capability exist and make every app take it; do not author new
portrait layouts until asked.** It is the instruction that shaped the two beads already
landed, and it is still in force for the ones below unless the user lifts it.

The acceptance test for the whole epic is physical, and it is one sentence: *pick the board
up, stand it on its USB-C port, and what is on the glass is drawn upright and is easy to
read.* Not a passing suite. The board, in your hand.

---

## Part 2 — Your instructions

### Start here

```sh
br show stick-c-plus-screen-rotation-platform-ce1     # the epic
br ready                                             # ce1.2 and ce1.4 are unblocked
cat docs/plans/screen-rotation-platform-capability.md
```

Take **ce1.2** first (the shared rotation source), then **ce1.4**, then **ce1.5**. That order
is not the dependency graph's preference — ce1.4 is technically unblocked too — it is because
ce1.5 needs both and ce1.4 is the one that can waste an afternoon on a panel that lies.

Do **not** start ce1.6/.7/.8 (the three portrait layouts) without asking. They are the part
the user deferred. They are marked ready because the seam they need exists, not because they
are wanted yet.

### What is already true

- `ScreenRotation`, `RotationSettler`, `rotation_for` and the measured `UpAxis` table live in
  **`platform-core`** (`src/rotation.rs`), with their own Gherkin suite
  (`platform/platform-core/tests/features/rotation.feature`, 6 scenarios). The table is
  *measured*, not derived — it cost a board in hand. Do not re-derive it.
- `Screen::show(state, elapsed, rotation)` — the rotation is handed to every screen by the
  platform, so no app can forget to be offered it.
- `spawn_display(display, source, rotation, clock, config)` — the rotation source is a
  `FnMut(Tick) -> ScreenRotation`. All four composition roots currently pass
  `let landscape = |_now: Tick| ScreenRotation::Deg0;`. **Replacing that one line per binary
  is the whole of "wire in a real source."**
- `Shown` (in `platform-runtime`'s render loop) carries the rotation, so a turn repaints by
  the same mechanism every other change uses. Two tests pin it.
- `orientation-display` has a finished **portrait layout** — `Layout` value, `LANDSCAPE` and
  `PORTRAIT` consts, invariants in a `const fn` that fails the *build*. It renders in
  `just screens` (14 screens, all four quadrants). It has never been seen on the glass.
- The other three display crates take the rotation and ignore it, and say so in the signature.

### What is not true yet, and is the work

Nothing rotates on the device. The device draws `Deg0`, always, because no rotation source
exists (ce1.2) and the panel is never told to scan differently (ce1.4).

**ce1.2 — the shared rotation source.** A thread that owns the IMU, folds readings through a
`RotationSettler`, and publishes the settled rotation for any binary to read. The existing
`orientation-shell` sampler is the right shape and the wrong context (`context = "orientation"`,
so the other three apps cannot use it) — generalize it into a shared crate rather than copying
it. The `Imu` port and `Acceleration` are already in `platform-core/shared`, so this needs no
dependency on any app.

**ce1.4 — turn the panel.** `Panel::new` bakes `Rotation::Deg90` in as a compile-time literal
and applies one CGRAM offset pair, `OFFSET_X = 52` / `OFFSET_Y = 40`, valid under that one
rotation. There is no `set_orientation` call anywhere in the crate. Add the runtime path, and
the offsets that belong to each orientation.

**ce1.5 — the orientation readout turns, end to end.** Everything it needs will exist. If it
turns out to need new domain logic, something above it was under-built.

### The hazards, which are real and were paid for

- **The CGRAM offsets are orientation-dependent and no recomputation exists.** 52/40 is the
  native-portrait window. It is *not* valid at another rotation. A wrong offset shows as a
  picture shifted by dozens of pixels, or a stripe of stale CGRAM down one edge. **No host
  test can see any of this** — the framebuffer sits below the panel. Only your eyes on the
  glass.
- **MADCTL is not portable, by precedent.** Read `kb/experiments/2026-07-09-panel-colour-order/`
  before ce1.4. The factory driver and `mipidsi` set the *same* MADCTL bit and the glass still
  rendered red as blue, because the pixel pipelines around the bit differ. Rotation bits live
  in that same register. Derive nothing from the factory driver; measure. `Panel::colour_check`
  is the model — a falsifiable on-glass test — and ce1.4 deserves its equivalent.
  (Note: an earlier handoff described that `panel.rs` comment as being about *rotation*
  offsets. It is not; it is about colour order. The caution transfers, the history does not.)
- **Hardware rotation, not a software transform.** A rotating `DrawTarget` maps rows onto
  columns and destroys the `fill_contiguous` fast path — see
  `kb/findings/mipidsi-rectangle-fill-costs-an-address-window.md`. At ~13 700 px/frame that is
  the difference between inside and outside the 25 Hz budget.
- **A green host gate does not mean the device works.** This project's most expensive failure
  mode. Flash it, watch serial for a reboot loop and for `paint took … over the … tick budget`.
- **10 ms is the hard floor** for any periodic thread period. `CONFIG_FREERTOS_HZ=100`, so a
  shorter sleep busy-waits instead of yielding and starves the idle task until the watchdog
  fires. Host tests cannot see it.

### House rules that are easy to re-break

- **Hexagonal, dependencies inward.** `hex-lint` runs in the gate and enforces both the role
  and the *context* axis. A cross-context dependency is what forced ce1.1 in the first place.
- **Explicit type annotations on every binding and lambda parameter.** Match the surrounding
  code; read a neighbouring file before writing. The voice is consistent across this repo and
  worth matching.
- **Gherkin all the way down**, and the specification lives with the code it specifies — if you
  move a rule between crates, move its scenarios too.
- **Verification is not optional.** Gate before committing: **`just ci`** (fmt, hex-lint, clippy
  as errors, host suites, both firmware builds). An honest "could not verify" is fine; a
  fabricated "verified" is the worst thing you can do.
- **Scoped commits** (https://scopedcommits.com/): `<scope>: <imperative, lowercase>`, scope
  first, never a Conventional-Commits type. Close the bead as it lands.
- **`br` prose via a quoted heredoc or `--description-file`** — backticks in `br -d "..."` get
  command-substituted and silently vanish.

### Open questions — the user's to answer, not yours to assume

1. **Should every app rotate by default, or does each binary opt in?** A pomodoro timer that
   reflows every time you set it down may be worse than one that stays put. The seam supports
   either. Suggested default: capability always present, each binary chooses whether to spawn
   the source.
2. **Does `host-monitor` earn an IMU at all?** It has WiFi plus a display thread that already
   needed 16 KiB of stack; a periodic I2C poll is where bus contention would show. It is the
   riskiest of the three wirings in ce1.9 and should go last if it goes at all.
3. **What does a screen do when its content genuinely does not fit portrait?** `host-display`
   is one row per host across the full width — its whole premise is horizontal room. "It does
   not rotate" is an acceptable answer; ce1.8 is written to permit it.

### One thing not to let look verified

The `NO SIGNAL` state (`358d9c0`) is proven host-side only. It was confirmed on the board that
it never *falsely* fires — 125 heartbeats on a healthy sensor, zero triggers — but nobody has
ever seen it appear on the glass. That needs a throwaway build with a deliberately failing IMU.
Not part of this epic; just do not count it as done.
