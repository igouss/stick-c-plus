# Handoff — the picture always faces the reader

**Epic:** `stick-c-plus-screen-rotation-platform-ce1`
**Plan:** `docs/plans/screen-rotation-platform-capability.md` — the inventory the plan was
written against, the architectural decision, and the hazards. Read it after this file.

Four of nine beads are settled: **ce1.1, ce1.2, ce1.3 done; ce1.8 dropped.** Five remain.
This file is the context around them that is not in the tracker.

Last updated 2026-07-21, after ce1.2 landed and the user narrowed the scope.

---

# Part 1 — The idea

This part is the user's. It is the thing being built; everything in Part 2 is implementation
serving it. Where they were quoted, the quotes are verbatim.

## What they want

> "when I change the orientation, I want to make it easier for me to read the text and graphs
> and whatever is on the UI to always face me and be easy to read, i.e when it's pointing up
> or down a different version of the view that the current application is displaying should be
> displayed. I.e views should react to orientation change."

## How wide it reaches

Asked explicitly, and the answer has two halves that must not be confused with each other.

**It is a platform capability, not an orientation-app feature.** Rotation reaches the render
loop and is handed to whichever render function is installed, so no app author can forget to
honour it. That half is settled and is the architectural spine of the epic.

**But taking it is opt-in, and not every app takes it.** In the user's words:

> "host monitor does not neet to handle orientation, this should be an opt-in capability, not
> all apps use it, for example now no one handles different pannel views that are based on
> orrientation, so the binaries should not be linked it, and app should have no memory or size
> pennalties if it chooses not to link with the orrientation updates."

So: **three apps** — orientation, pomodoro, plant-monitor. **`host-monitor` is out entirely**:
no IMU, no rotation source, no portrait layout.

Read the second sentence of that quote as a hard requirement, not a preference. A binary that
does not opt in must not *link* the capability and must pay nothing in flash or RAM for its
existence. It is a requirement you can check, and Part 2 tells you how.

## How fast to go

> "this should be a capability they should be aware of, and their code modified to handle the
> rotation cases, but for now keep the existing behaviour i.e don't update views to handle
> rotation changes."

Read that as: **make the capability exist and make every participating app take it; do not
author new portrait layouts until asked.** It is still in force. ce1.6 and ce1.7 are the
deferred part.

## How you will know it worked

The acceptance test for the whole epic is physical, and it is one sentence:

> *Pick the board up, stand it on its USB-C port, and what is on the glass is drawn upright
> and is easy to read.*

Not a passing suite. The board, in a hand. **And the user has offered to be that hand:**

> "and I can help with rotating it, tell me when."

Take them up on it — see "Working with the user on the board" in Part 2. Nobody can close
ce1.4 or ce1.5 honestly without someone turning the stick and reporting what they saw.

---

# Part 2 — Your instructions

## Start here

```sh
br show stick-c-plus-screen-rotation-platform-ce1     # the epic, with the settled scope
br ready                                             # ce1.4 is the next P1
cat docs/plans/screen-rotation-platform-capability.md
```

Take **ce1.4** first, then **ce1.5**. ce1.4 is the one that can waste an afternoon on a panel
that lies, and ce1.5 needs it. **ce1.9** (wiring pomodoro and plant-monitor) can go any time
after ce1.4; it is independent of ce1.5.

Do **not** start ce1.6/.7 (the portrait layouts) without asking. They are the part the user
deferred. They show as ready because the seam they need exists, not because they are wanted.

## What is already true

Verified, not assumed. Do not re-derive any of it.

- **The rotation domain lives in `platform-core`** (`src/rotation.rs`): `ScreenRotation`,
  `RotationSettler`, `rotation_for`, the measured `UpAxis` table, `ROTATION_SETTLE_MS = 250`.
  It has its own Gherkin suite — `platform/platform-core/tests/features/rotation.feature`,
  6 scenarios, run by `tests/cucumber.rs`. **The `UpAxis` table is measured, not derived** —
  it cost a board in hand. Change those four lines only with the board in yours.
- **`Screen::show(state, elapsed, rotation)`** — the platform hands the rotation to every
  screen, so no app can fail to be offered it.
- **`spawn_display(display, source, rotation, clock, config)`** — the rotation input is a
  `FnMut(Tick) -> ScreenRotation`. All four composition roots currently pass
  `let landscape = |_now: Tick| ScreenRotation::Deg0;`. **Replacing that one line is the whole
  of "wire in a real source"** — and leaving it alone is the whole of opting out.
- **`Shown` carries the rotation** inside `platform-runtime`'s repaint-suppression comparison,
  so a turn repaints by the same mechanism as every other change. Two tests pin it. This is
  deliberate: turn the panel without repainting and the glass holds the previous quadrant's
  pixels, correctly rotated and wrong.
- **`SharedRotation` and `spawn_rotation`** live in `platform-runtime/src/rotation.rs`, beside
  the other `spawn_*` threads (it is already `driving-adapter` / `context = "shared"`, so no
  new crate was needed). 13 unit tests, mutation-checked.
  - `SharedRotation` owns the `RotationSettler` **behind its lock**, so a caller cannot fold a
    reading in without the settle rule applying, and two feeders cannot disagree about how long
    a candidate has been held. `update(acceleration, now)` writes; `current()` reads;
    `source()` hands back the closure `spawn_display` wants.
  - `spawn_rotation` is a thread that owns an `Imu` and feeds it. Poll period **50 ms**, not
    the orientation sampler's 10 ms: the settle window is what the eye sees, and the polls
    saved are I2C transactions that would otherwise contend.
  - **They are two halves on purpose.** The sensor *moves* into whichever thread takes it, so
    an app that already runs a sampler cannot also `spawn_rotation` — see ce1.5 below.
- **`orientation-display` has a finished portrait layout** — `Layout` value, `LANDSCAPE` and
  `PORTRAIT` consts, invariants in a `const fn` that fails the *build*. It renders in
  `just screens` (14 screens, all four quadrants). **It has never been seen on glass.**
- **The other three display crates take the rotation and ignore it**, and say so in the
  signature.

## What is not true yet, and is the work

**Nothing rotates on the device.** The device draws `Deg0` always, because the panel is never
told to scan differently (ce1.4) and no binary spawns the rotation source (ce1.5, ce1.9).

### ce1.4 — turn the panel *(next; needs the board)*

`firmware/platform/adapters/src/panel.rs` bakes the rotation in at construction:

- `panel.rs:186` — `.orientation(Orientation::new().rotate(Rotation::Deg90))`, a compile-time
  literal.
- `panel.rs:68,70` — `OFFSET_X = 52`, `OFFSET_Y = 40`, one pair, applied at `:185` via
  `.display_offset(...)`.
- There is **no `set_orientation` call anywhere in the crate.**

Add the runtime path, and the CGRAM offsets that belong to each orientation. Then prove it on
glass — `Panel::colour_check` (`panel.rs:221`) is the model to follow: a falsifiable on-glass
test, not a host assertion. ce1.4 deserves its equivalent.

### ce1.5 — the orientation readout turns, end to end

The first app to actually rotate; proves ce1.1–ce1.4 together. Everything it needs will exist.
**If it turns out to need new domain logic, something above it was under-built** — stop and
look upward rather than adding logic here.

One real obstacle, found during ce1.2 and not yet solved: **orientation already owns the IMU.**
`firmware/apps/orientation/bin/src/main.rs:145` *moves* the sensor into `spawn_sampler`, so
this binary cannot also call `spawn_rotation`. It must feed the same `SharedRotation` from
what it already reads. Where that call goes is an open design question with a layering
constraint on it: `orientation-shell` is `context = "orientation"` and `platform-runtime` is
`context = "shared"`, and `hex-lint` enforces that axis. The composition root can name both.
Weigh that against putting the feed inside the sampler before you write anything.

### ce1.9 — wire pomodoro and plant-monitor

Two binaries, not three. Neither has an IMU thread today, so both can `spawn_rotation`
directly — the easy case. Get the sensor onto each one's I2C bus, spawn the source, replace
the `landscape` closure. **Re-measure sizes before closing** (see below).

### ce1.6 / ce1.7 — portrait layouts *(deferred — ask first)*

Follow the pattern `orientation-display/src/layout.rs` established: a `Layout` value,
`LANDSCAPE`/`PORTRAIT` consts, invariants in a `const fn` that fails the build. `just screens`
is the review surface. Note the structural blocker the plan describes: `SCREEN_SIZE` is one
global const and the three other display crates pin every origin against it, each asserting
`width > height` at build time. Canvas size has to become a property of a layout.

## The requirement that is easy to quietly break

> A binary that does not opt in must not link the capability, and must pay nothing in flash or
> RAM for its existence.

**This is true today, and it was measured rather than argued** (2026-07-21):

- The shared rotation source is **byte-identical** across all four binaries. `spawn_rotation`
  is generic, so it is never instantiated unless called, and `SharedRotation` is dead-stripped.
- `nm` finds **zero** rotation, settler, or up-axis symbols in `host-monitor`.
- The ce1.3 seam itself costs `host-monitor` +260 B of text out of ~1 MB (0.03%) and the other
  two +4 B, while `orientation` *shrank* 40 B — i.e. inlining jitter, not linked logic.

Baseline, `size` on the release elfs at commit `f8aa0ef`:

| binary | text | data | bss |
|---|---|---|---|
| host-monitor | 1 000 941 | 203 356 | 23 321 |
| pomodoro | 391 648 | 105 220 | 6 633 |
| plant-monitor | 981 441 | 190 360 | 25 353 |
| orientation | 394 720 | 97 428 | 6 641 |

**Keep it true.** The opt-in must stay a composition-root choice: `spawn_rotation` stays
generic, and nothing non-generic gets referenced from a shared path every binary walks. Before
closing any bead that touches a composition root or a shared path:

```sh
just build
for b in host-monitor pomodoro plant-monitor orientation; do
  size firmware/target/xtensa-esp32-espidf/release/$b | tail -1
done
nm -C firmware/target/xtensa-esp32-espidf/release/host-monitor | grep -ic "rotation\|settler\|up_axis"   # must be 0
```

## The hazards, which are real and were paid for

- **The CGRAM offsets are orientation-dependent and no recomputation exists.** 52/40 is the
  native-portrait window. It is *not* valid at another rotation. A wrong offset shows as a
  picture shifted by dozens of pixels, or a stripe of stale CGRAM down one edge. **No host test
  can see any of this** — the framebuffer sits below the panel. Only eyes on the glass.
- **MADCTL is not portable, by precedent.** Read
  `kb/experiments/2026-07-09-panel-colour-order/README.md` before ce1.4. The factory driver and
  `mipidsi` set the *same* MADCTL bit and the glass still rendered red as blue, because the
  pixel pipelines around the bit differ. Rotation bits live in that same register. **Derive
  nothing from the factory driver; measure.**
- **Hardware rotation, not a software transform.** A rotating `DrawTarget` maps rows onto
  columns and destroys the `fill_contiguous` fast path — see
  `kb/findings/mipidsi-rectangle-fill-costs-an-address-window.md`. At ~13 700 px/frame that is
  the difference between inside and outside the 25 Hz budget.
- **A green host gate does not mean the device works.** This project's most expensive failure
  mode. Flash it, watch serial for a reboot loop and for
  `paint took … over the … tick budget`.
- **10 ms is the hard floor** for any periodic thread period. `CONFIG_FREERTOS_HZ=100`, so a
  shorter sleep busy-waits instead of yielding and starves the idle task until the watchdog
  fires. Host tests cannot see it — `std::thread::sleep` yields at any duration on Linux.

## Working with the user on the board

They have offered, and ce1.4 cannot be closed without them. Do the work first, get to a
flashable build, then ask — do not ask them to stand by while you write code.

When you ask, give them something specific to answer. For each of the four quadrants, you want
to know whether the picture is:

- **(a) upright and correctly placed** — that rotation's offsets are right;
- **(b) upright but shifted** by a visible margin — wrong CGRAM offset for that rotation;
- **(c) showing a stripe of junk** down one edge — same, worse.

(b) and (c) are the hazard above, and they are invisible to every test you can run yourself.
Ask for the four answers together; a report of "it works" is not usable.

## House rules that are easy to re-break

- **Hexagonal, dependencies inward.** `hex-lint` runs in the gate and enforces both the role
  and the *context* axis. A cross-context dependency is what forced ce1.1 in the first place.
- **Explicit type annotations on every binding and lambda parameter.** Match the surrounding
  code; read a neighbouring file before writing. The voice is consistent across this repo and
  worth matching.
- **Gherkin all the way down**, and the specification lives with the code it specifies — if you
  move a rule between crates, move its scenarios too. (Note the split in practice: pure rules
  get cucumber suites; `platform-runtime`'s threads carry in-module unit tests, because the
  rule they enforce is specified in `platform-core` where it lives.)
- **Verification is not optional.** Gate before committing: **`just ci`** (fmt, hex-lint, clippy
  as errors, host suites, both firmware builds). An honest "could not verify" is fine; a
  fabricated "verified" is the worst thing you can do. When a suite passes first try, that is
  the moment to check it can fail — mutate the code and confirm the right test dies.
- **Scoped commits** (https://scopedcommits.com/): `<scope>: <imperative, lowercase>`, scope
  first, never a Conventional-Commits type. Close the bead as it lands.
- **`br` prose via `--description-file`** (works as of br 0.2.19, both a path and `-` for
  stdin). Backticks inside a double-quoted `br -d "..."` are still command-substituted by bash
  and vanish silently. `br delete` is a *soft* delete — the row stays as `status: tombstone`.
  Whatever you write, **`br show` it afterwards and read the field back.**

## Two things not to let look verified

1. **The `NO SIGNAL` state** (`358d9c0`) is proven host-side only. It was confirmed on the board
   that it never *falsely* fires — 125 heartbeats on a healthy sensor, zero triggers — but
   nobody has ever seen it appear on the glass. That needs a throwaway build with a
   deliberately failing IMU. Not part of this epic; just do not count it as done.
2. **`orientation-display`'s portrait layout** renders correctly into a host framebuffer and
   has never been on glass. `just screens` proves the layout, not the panel. Until ce1.4 lands
   it is an untested picture, and it is exactly the picture ce1.5 will put up first.
