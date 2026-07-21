# Handoff — the picture always faces the reader

**Epic:** `stick-c-plus-screen-rotation-platform-ce1`
**Plan:** `docs/plans/screen-rotation-platform-capability.md` — the inventory the plan was
written against, the architectural decision, and the hazards. Read it after this file.

Five of nine beads are settled: **ce1.1, ce1.2, ce1.3, ce1.4 done; ce1.8 dropped.** Four
remain. This file is the context around them that is not in the tracker.

Last updated 2026-07-21, after ce1.4 was confirmed on the glass.

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
br ready                                             # ce1.5 is the next P1
cat docs/plans/screen-rotation-platform-capability.md
```

Take **ce1.5** first. Every piece it needs now exists and is proven, so it should be small —
if it is not, read "What ce1.5 actually is" below before writing anything, because the most
likely cause is that you are rebuilding something that already works.

**ce1.9** (wiring pomodoro and plant-monitor) is independent of ce1.5 and can go either side
of it.

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
- **The panel turns at runtime.** `Panel::set_rotation` writes MADCTL and mipidsi recomputes
  the window; `PanelScreen` applies it *before* each render, because turning changes the shape
  of the draw target. **Confirmed on the glass at all four rotations** (2026-07-21): window
  aligned, `RED` corner top-left beside `UP` at every stop. `panel_rotation`'s +90° phase is
  measured, not derived — the two scales share a step but not an origin, and one turns the
  image while the other turns the memory scan.
- **Rotation is opted into by TYPE, not by a flag.** `PanelScreen::new` → `Fixed`, the panel
  never turns and the binary links none of the machinery. `PanelScreen::turning` → `Turning`,
  it follows. This exists because an unconditional `set_rotation` on the render path cost
  `host-monitor` **744 bytes** for a branch it can never take; `Fixed::apply` is an empty
  function on a zero-sized type, so nothing is generated. All four apps are still `Fixed`.
- **`just run-bin display-rotation-check`** is the on-glass instrument: a thick band flush to
  every edge (lopsided ⇒ offset bug) and four coloured corner squares (turned ⇒ mapping bug).
  It reads the two faults independently because they have different fixes. Built twice — a
  1-px border proved unreadable on the real panel, and a fill-then-cover frame flashed white
  on every redraw, imitating the very artefact it exists to expose.
- **`orientation-display` already picks its layout from the rotation.**
  `screen.rs:92` — `let layout: Layout = Layout::for_rotation(rotation);` — and
  `Layout::for_rotation` (`layout.rs:112`) maps `Deg0`/`Deg180` to `LANDSCAPE` and
  `Deg90`/`Deg270` to `PORTRAIT`, with four unit tests pinning it. The `Layout` value carries
  its own `canvas`, and the invariants sit in a `const fn` that fails the *build*. It renders
  in `just screens` (14 screens, all four quadrants). **This is why ce1.5 is wiring and not
  work** — and it has never been seen on glass.
- **The other three display crates take the rotation and ignore it**, and say so in the
  signature.

## What is not true yet, and is the work

**No app rotates on the device.** The panel *can* turn and is proven to (ce1.4), but no binary
feeds it a real rotation: all four still pass the constant `landscape` closure and take
`PanelScreen::new`.

### ce1.5 — the orientation readout turns, end to end *(next; needs the board)*

The first app to actually rotate, and the bead that proves ce1.1–ce1.4 together by putting
them all on the glass at once.

**What ce1.5 actually is.** Four things in one composition root,
`firmware/apps/orientation/bin/src/main.rs`, and nothing else:

1. Make a `SharedRotation`.
2. Feed it. See the obstacle below.
3. `PanelScreen::turning(panel, orientation_display::render)` instead of `::new` — this is the
   opt-in, and without it the panel stays landscape however good the rotation source is.
4. Pass `rotation.source()` to `spawn_display` in place of `let landscape = |_now: Tick| …`.

**Everything below that line already works and is tested.** The settler, the panel, and the
layout selection are all in place — `orientation-display::render` has picked its layout from
the rotation since before this epic began. **If you find yourself writing domain logic, stop**:
something above you was under-built, and the fix is there, not here.

**The one real obstacle: orientation already owns the IMU.**
`main.rs:145` *moves* the sensor into `spawn_sampler`, so this binary cannot also call
`spawn_rotation` — a single I2C device cannot have two owners. It has to feed the same
`SharedRotation` from readings it already takes.

The recommended shape, and why. `SharedOrientation` already publishes an `Orientation` that
carries `.acceleration`, and `SharedRotation::update` wants exactly an `Acceleration` and a
`Tick`. So the rotation source closure can do both jobs at once, in the composition root:

```rust
let rotation_source = {
    let pose: SharedOrientation = shared.clone();
    let turned: SharedRotation = SharedRotation::new();
    move |now: Tick| {
        turned.update(pose.last_known().acceleration, now);
        turned.current()
    }
};
```

No second device owner, no second thread, no duplicated settler. It runs in the display thread
at 25 Hz — 40 ms a fold, against a 250 ms settle window, so it is nowhere near tight.

Why the composition root rather than inside the sampler: `orientation-shell` is
`context = "orientation"` and `platform-runtime` is `context = "shared"`, and `hex-lint`
enforces that axis in the gate. The root has no context and may name both. Putting the feed in
the sampler would mean `orientation-shell` depending on `platform-runtime`, which is a
cross-context edge — check it before you try it.

Weigh this against alternatives if you like, but do not reach for `spawn_rotation` here; it is
for the apps in ce1.9 that have no IMU thread.

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

**This is true today, and it was measured rather than argued** — twice, because the first
version of ce1.4 broke it.

**`size` is the signal. `nm` is not sufficient, and believing it once already cost a
regression.** An unconditional `set_rotation` on the render path added **744 bytes** to
`host-monitor` for a branch it can never take, and `nm` still reported *zero* rotation symbols,
because the call was inlined into `Screen::show` and left no symbol of its own. Run both, but
trust the byte count.

That regression is why the opt-in is a **type** (`Fixed` / `Turning`) rather than a runtime
branch: `Fixed::apply` is an empty function on a zero-sized type, so the turn is not skipped at
run time, it is never generated.

Baseline, `size` on the release elfs at commit `7979b6d` (all four still `Fixed`):

| binary | text | data | bss |
|---|---|---|---|
| host-monitor | 1 000 949 | 203 356 | 23 321 |
| pomodoro | 391 644 | 105 220 | 6 633 |
| plant-monitor | 981 417 | 190 360 | 25 353 |
| orientation | 394 736 | 97 428 | 6 641 |

Judgement on what a diff means: **anything under ~±50 B that drifts in both directions across
the four binaries is inlining jitter**, not linked logic — that pattern has now appeared twice.
A consistent several-hundred-byte rise in one direction is a real link and needs explaining.

**ce1.5 and ce1.9 will legitimately grow the binaries they touch** — that is what opting in
means. What must not move is `host-monitor`, which opts into nothing. Before closing either
bead:

```sh
just build
for b in host-monitor pomodoro plant-monitor orientation; do
  printf '%-14s %s\n' "$b" "$(size firmware/target/xtensa-esp32-espidf/release/$b | tail -1)"
done
```

## The hazards, which are real and were paid for

- **~~The CGRAM offsets are orientation-dependent and no recomputation exists.~~ Wrong, and
  disproved at ce1.4.** mipidsi 0.10 *does* recompute: `set_address_window` derives every
  orientation's window from the native-portrait offset pair, the display size and the model's
  framebuffer size, and `display_size()` swaps the canvas to match. **Do not write a
  per-rotation offset table** — none is needed, and one would double-count. The live hazard is
  the opposite of what was written: `display_offset` **must stay 52/40 native-portrait**,
  because it is the *input* to that derivation, not the current orientation's answer. Bake a
  rotated offset in and it compounds — perfect at one rotation, tens of pixels out at the other
  three. See `kb/findings/mipidsi-derives-the-cgram-window-per-orientation.md`.
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

They have offered, they are quick to respond, and ce1.5 cannot be closed without them. Do the
work first, get to a flashable build, then ask — do not ask them to stand by while you write
code. You can flash it yourself: `just run-bin <name>`, or `just run` to put the plant monitor
back. Serial needs `timeout N just run-bin … > file 2>&1`; piping to `tail` swallows it.

**ce1.5's question is different from ce1.4's, and easier.** ce1.4 asked about the panel and
needed a careful four-part answer. ce1.5 asks the epic's own acceptance test, and it is one
sentence: *pick the board up, stand it on its USB-C port, and the readout is drawn upright and
is easy to read.* Then set it flat on the desk and confirm it does **not** scramble, and give
it a shake and confirm it does not spin — those are the settler's two promises
(`rotation_for` returns `current` when flat or moving) and this is the first time either has
been on glass.

**Three lessons about asking, all of them paid for during ce1.4:**

1. **A report of "it works" is not usable, and neither is "looks good".** Ask for something
   specific enough that a wrong answer looks different from a right one.
2. **Watch for an answer that reads two ways.** "All the rotations give same results" can mean
   "correct at every stop" or "the picture never moved" — opposite conclusions, one of them a
   false green. When you get one, do not pick the flattering reading: name both readings back
   and ask for the observation that separates them.
3. **If the instrument is hard to read, the instrument is wrong.** The first rotation frame
   drew a 1-pixel border and got "very thin, hard to see" — which is a failed instrument, not a
   passed test. Rebuild it around what a person judges reliably (comparing two thicknesses,
   naming a colour) rather than what is easiest to draw. See the lesson below.

## Show the user the pictures — do not make them squint

**The user asked for this directly**, and it is the cheapest review loop in the project:

> "I think next time you can make screenshots in every orientations to see how it rotates and
> how it looks."

`just screens` renders every state of all four apps to `target/screens/*.png` through the
**same** render functions the panel calls — `orientation-display`'s example already covers all
four quadrants. Send the PNGs. A 1.14″ panel is a poor review surface for layout questions, and
a reviewer looking at a rendered image can tell you "that label is clipped in portrait" in a
way nobody can while turning a stick in their hand.

Use it **before** asking for a board session, not instead of one. The split is exact and worth
holding onto:

- **Screenshots answer layout**: wording, spacing, alignment, what fits in 13 columns instead
  of 24, whether a portrait variant is any good.
- **Only the glass answers the panel**: CGRAM window, MADCTL, colour order, flicker, tick
  budget — everything below `DrawTarget`.

Getting the layout right on the host first means the board session is spent on the questions
only the board can answer.

## A lesson worth keeping: build the instrument for the eye that reads it

`display-rotation-check` was built three times, and the two rebuilds were both defects that
only the bench could find. It is worth stating as a general rule because the next on-glass test
will meet it again:

- **Correct is not the same as legible.** A 1-px line at the extreme edge of a 1.14″ panel
  behind a bezel cannot be judged present-or-absent. It was a perfect test that could not
  falsify anything.
- **Prefer judgements humans are good at.** Comparing two thicknesses, or naming a colour, beat
  detecting a hairline. The band goes *lopsided* under a translated window rather than
  vanishing, and four differently-coloured corners make "which colour is top-left" a complete
  rotation answer that needs no reading of upside-down text.
- **Do not let the instrument imitate the fault.** The second version filled the canvas white
  and laid black back over the interior — two draws instead of five — which flashed the whole
  glass on every redraw. A full-screen flash is exactly what a stale-memory stripe looks like,
  so the test was mimicking the artefact it exists to expose. It now draws four strips and
  never touches the interior.
- **Note where the host is blind.** Fill-then-cover and four-strips leave a framebuffer
  byte-identical; the flicker lives in the *sequence* of writes, which a framebuffer does not
  record. That is said in the test module, so a green suite does not imply more than it proves.

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
   **has still never been on glass.** `just screens` proves the layout, not the panel — a
   framebuffer places every pixel exactly where asked, whatever the controller does. It is
   exactly the picture ce1.5 will put up first, so expect ce1.5 to be the moment it is finally
   tested, and treat a surprise there as information rather than as a setback.
