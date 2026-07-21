# Handoff — the picture always faces the reader

**Epic:** `stick-c-plus-screen-rotation-platform-ce1`
**Plan:** `docs/plans/screen-rotation-platform-capability.md` — the verified inventory, the
architectural decision, and the hazards. Read it after this file.

Three of nine beads are done (ce1.1, ce1.3, ce1.2), one is dropped (ce1.8). This file is the
context around the rest that is not in the tracker.

---

## Part 1 — The idea

This is the user's, in their words. It is the thing being built; everything else is
implementation.

> "when I change the orientation, I want to make it easier for me to read the text and graphs
> and whatever is on the UI to always face me and be easy to read, i.e when it's pointing up
> or down a different version of the view that the current application is displaying should be
> displayed. I.e views should react to orientation change."

On how wide that reaches — asked explicitly, answered explicitly: **a platform capability**,
not a feature of the orientation app.

> **Narrowed 2026-07-21.** That first answer said all four apps. The user has since scoped it
> down: rotation is **opt-in**, and **`host-monitor` is out entirely** — no IMU, no rotation
> source, no portrait layout. Three apps: orientation, pomodoro, plant-monitor. The *platform*
> half of the answer stands unchanged — the capability still lives in the render loop, not in
> any app's view. What changed is who feeds it, and that a binary which does not must pay
> nothing for its existence. See "Open questions", below.

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
br ready                                             # ce1.4 is the next P1
cat docs/plans/screen-rotation-platform-capability.md
```

Take **ce1.4** next, then **ce1.5**. ce1.2 has landed; ce1.4 is the one that can waste an
afternoon on a panel that lies, and ce1.5 needs it.

Do **not** start ce1.6/.7 (the portrait layouts) without asking. They are the part the user
deferred. They are marked ready because the seam they need exists, not because they are wanted
yet. (ce1.8, the third, is closed won't-do.)

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
  is the whole of "wire in a real source."** That line is also where opting *out* lives: leave
  it alone and the binary links none of the capability.
- **`SharedRotation` and `spawn_rotation`** live in `platform-runtime` (`src/rotation.rs`),
  beside the other `spawn_*` threads. `SharedRotation` owns the `RotationSettler` behind its
  lock and `source()` hands back the closure above; `spawn_rotation` is a thread that owns an
  IMU and feeds it. **They are two halves on purpose** — the sensor *moves* into whichever
  thread takes it, so an app that already runs a sampler (orientation, at 100 Hz) cannot spawn
  a second owner of one I2C device and must feed the same cell instead. Poll period is 50 ms,
  not the sampler's 10 ms: the settle window is what the eye sees, and the saved polls are I2C
  transactions that would otherwise contend.
- `Shown` (in `platform-runtime`'s render loop) carries the rotation, so a turn repaints by
  the same mechanism every other change uses. Two tests pin it.
- `orientation-display` has a finished **portrait layout** — `Layout` value, `LANDSCAPE` and
  `PORTRAIT` consts, invariants in a `const fn` that fails the *build*. It renders in
  `just screens` (14 screens, all four quadrants). It has never been seen on the glass.
- The other three display crates take the rotation and ignore it, and say so in the signature.

### What is not true yet, and is the work

Nothing rotates on the device. The device draws `Deg0`, always, because the panel is never told
to scan differently (ce1.4) and no binary spawns the rotation source (ce1.5, ce1.9).

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

### Open questions — answered by the user, 2026-07-21

All three are settled. They were asked as open; they are recorded here as closed.

1. **Every app by default, or opt in per binary?** → **Opt in.** Not every app uses rotation,
   and today none of them has a portrait view to turn into, so opting in currently buys
   nothing. The seam stays always-present; each composition root chooses whether to feed it.
2. **Does `host-monitor` earn an IMU?** → **No.** It does not handle orientation at all: no
   IMU, no rotation source, no portrait layout. ce1.9 therefore covers **two** binaries
   (pomodoro, plant-monitor), and **ce1.8 is closed won't-do**.
3. **What does a screen do when its content does not fit portrait?** → Answered by (2) for the
   only screen where it was pressing. "It does not rotate" was the acceptable answer, and it
   is the one taken.

And one constraint that came with the answer, which is now a **hard requirement on every
remaining bead**:

> **A binary that does not opt in must not link the capability, and must pay nothing in flash
> or RAM for its existence.**

This is currently true, and it was measured rather than assumed (2026-07-21, at ce1.2):

- The shared rotation source is **byte-identical** across all four binaries — `spawn_rotation`
  is generic, so it is never instantiated unless called, and `SharedRotation` is dead-stripped.
- `nm` finds **zero** rotation, settler, or up-axis symbols in `host-monitor`.
- The ce1.3 seam itself costs `host-monitor` +260 B of text out of ~1 MB (0.03%) and the other
  two +4 B, while `orientation` *shrank* by 40 B — i.e. inlining jitter, not linked logic.

Keep it that way: the opt-in must stay a composition-root choice. `spawn_rotation` stays
generic, and nothing non-generic gets referenced from a shared path every binary walks.
**Re-measure `size` on all four elfs before closing any bead that touches a composition root
or a shared path**, and diff against the numbers above.

### One thing not to let look verified

The `NO SIGNAL` state (`358d9c0`) is proven host-side only. It was confirmed on the board that
it never *falsely* fires — 125 heartbeats on a healthy sensor, zero triggers — but nobody has
ever seen it appear on the glass. That needs a throwaway build with a deliberately failing IMU.
Not part of this epic; just do not count it as done.
