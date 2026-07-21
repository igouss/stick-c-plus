# Plan — the picture always faces the reader

**Goal, in the user's words:** when the board's orientation changes, the UI should stay easy to
read — whatever the current app is showing turns to face you, in a layout built for the shape
it is turning into.

This is a **platform capability**, not an orientation-app feature. All four apps get it.

## What existed when this plan was written

Verified 2026-07-21, not assumed — and now **deliberately kept as the pre-work inventory**.
Several rows are no longer true: ce1.1 moved the rotation domain into `platform-core`, ce1.3
put the rotation parameter on `Screen::show` and `spawn_display`, and ce1.2 added the shared
source. For what is true *now*, read the handoff's "What is already true"; this table records
the ground the plan was reasoning from.

| Piece | Where | State |
|---|---|---|
| `ScreenRotation`, `RotationSettler`, the measured `UpAxis` table | `apps/orientation/orientation-core/src/rotation.rs` | Complete, host-tested, **never wired into firmware** |
| Portrait layout + `Layout::for_rotation` | `apps/orientation/orientation-display/src/layout.rs` | Complete (landed 5589bce) |
| `OrientationView.rotation` | `.../view.rs` | Exists; **only** the screenshots example ever sets it |
| Rotation on the glass | — | Nothing. The device renders `Deg0` always |
| `Screen::show(state, elapsed)` | `platform/platform-core/src/screen.rs:17` | No rotation parameter |
| `Panel::new` | `firmware/platform/adapters/src/panel.rs:183` | `Rotation::Deg90` is a **compile-time literal**; no `set_orientation` anywhere |
| CGRAM offsets | `panel.rs:68,70` | `OFFSET_X = 52`, `OFFSET_Y = 40`, one pair, valid under one rotation only |
| IMU | `firmware/apps/orientation/bin/src/main.rs` | Wired into **one** binary of four |

So this is mostly *new wiring*, not a refactor of working code. The domain logic and the
picture both exist; the legs between them — shell→view and view→panel — do not.

## The seam

Rotation reaches the render loop, **not** each app's view.

The alternative is to keep doing what `OrientationView` does today: carry a `rotation` field in
every app's state, and have every app's source closure remember to populate it. That works and
costs the platform nothing, which is exactly why it is wrong here — four apps would each
re-implement it, and nothing would catch the one that forgot. A capability the platform
supplies cannot be forgotten by an app author.

So:

- `Screen::show(&mut self, state: S, elapsed: Tick, rotation: ScreenRotation)`.
- `spawn_display` takes a rotation input beside its `source`.
- `Shown` gains a `rotation` field, so it enters the `PartialEq` that suppresses repaints. A
  turn therefore repaints by the same mechanism every other change already uses.
- `OrientationView.rotation` is **removed** — it becomes the loop's business. That is a small
  undo of the bead that introduced it, and it is the right trade.

Change-detection is where a rotation bug would otherwise hide: turn the panel without
repainting and the glass holds the previous quadrant's pixels, correctly rotated. Putting
rotation inside the compared unit makes that unrepresentable.

## The structural blocker: `SCREEN_SIZE`

`platform_display::SCREEN_SIZE` is one const, and `pomodoro-display`, `plant-display` and
`host-display` pin every origin against it, each asserting `width > height` at build time
(`pomodoro/layout.rs:35`, `plant/layout.rs:44`, `host/layout.rs:99`).

Canvas size has to become **a property of a layout** rather than a global. `orientation-display`
already did this — `Layout { canvas, .. }` with `LANDSCAPE`/`PORTRAIT` consts and the invariant
block moved into a `const fn` every layout is held to. The other three follow that pattern.

`SCREEN_SIZE` itself stays: it is the panel's native landscape, which is a true fact. What
changes is that layouts stop reading it directly.

Each app's `render` grows the rotation parameter and selects its layout — the same shape in all
four, so `PanelScreen`'s injected closure keeps working unchanged.

## The work

Nine beads as planned, eight of them live — (8) is dropped. Everything through (5) is
infrastructure that changes nothing visible until (5) lands; (6) and (7) are independent of
each other once (3) exists.

1. **Carve the rotation domain into `platform-core`.** `ScreenRotation`, `RotationSettler`, the
   `UpAxis` table. They are facts about the board, not about one app's domain, and
   `context = "orientation"` makes them illegal for the other three to use. Breaks three known
   call sites plus the cucumber suite; no other crate depends on `orientation-core`.
2. **A shared rotation source.** *Landed as two halves, not one.* `SharedRotation` owns the
   `RotationSettler` behind the lock and hands `spawn_display` its source closure;
   `spawn_rotation` is a thread that owns an IMU and feeds it. They are separate because the
   sensor **moves** into whichever thread takes it, so an app that already runs a sampler —
   orientation does — cannot spawn a second owner of one I2C device, and instead feeds the
   same cell. A thread-only design would have forced a second settler into that app. Lives in
   `platform-runtime` (already `driving-adapter`/`shared`) beside the other `spawn_*` threads,
   so no new crate was needed.
3. **The render-loop seam.** Widen `Screen::show`, thread rotation through `spawn_display` /
   `render_loop` / `render_once`, add it to `Shown`'s equality, update all four apps' `render`
   signatures and both `Screen` impls. **No visual change** — every app still selects its
   landscape layout. This is the bead that makes the capability exist.
4. **Runtime panel rotation.** `mipidsi` `set_orientation` plus per-orientation CGRAM offsets.
   Hardware MADCTL rather than a rotating `DrawTarget`, because a transform maps rows onto
   columns and destroys the `fill_contiguous` fast path — see
   `kb/findings/mipidsi-rectangle-fill-costs-an-address-window.md`, ~13 700 px/frame.
   **On-metal verification required.**
5. **The orientation readout turns, end to end.** First app to actually rotate. Proves (1)–(4).
6. **Portrait layout for `pomodoro-display`.**
7. **Portrait layout for `plant-display`.**
8. ~~**Portrait layout for `host-display`.**~~ **Dropped** — `host-monitor` does not rotate.
9. **The IMU into the other two binaries** (pomodoro, plant-monitor).

```
1 → 2 → 3 → 4 → 5
        3 → 6, 7
        2 → 9
```

## Hazards

- **The CGRAM offsets are orientation-dependent and no recomputation exists.** The visible
  window sits at col 52 / row 40 in native portrait; that pair is not valid at another
  rotation. A wrong offset shows as a picture shifted by dozens of pixels, or a stripe of stale
  CGRAM down one edge. **No host test can see this** — the framebuffer is below the panel.
- **MADCTL is not portable, by precedent.** The colour-order bug
  (`kb/experiments/2026-07-09-panel-colour-order/`) is the case study: the factory driver and
  `mipidsi` set the same MADCTL bit and the glass still rendered red as blue, because the pixel
  pipelines around the bit differ. Rotation bits live in that same register. Derive nothing from
  the factory driver; measure. `Panel::colour_check` is the model for a falsifiable on-glass
  test.
- **The tick budget.** The readout runs at 25 Hz. A rotation that repaints every pixel must stay
  inside it; the loop already warns (`paint took … over the … tick budget`) and that warning is
  the gate.
- **Bus contention on `host-monitor`**, which has WiFi plus a display thread that already needed
  16 KiB of stack. Adding an IMU poll there is the riskiest of the three wirings in (9) and
  should go last.
- **10 ms is the hard floor** for any thread period (`CONFIG_FREERTOS_HZ=100`).

## Open questions — answered 2026-07-21

- **Every app, or opt-in per app?** → **Opt-in.** The capability stays always-present in the
  seam; each composition root chooses whether to feed it. No app has a portrait view today, so
  opting in currently buys nothing.
- **Is `host-monitor` worth an IMU at all?** → **No.** It does not handle orientation: no IMU,
  no rotation source, no portrait layout. **(8) is dropped**, and (9) covers two binaries.
- **What happens on apps whose content does not fit portrait?** → They do not rotate. That was
  the pressing question only for `host-display`, and (2) answers it.

### The cost constraint that came with opt-in

A binary that does not opt in **must not link the capability**, and must pay nothing in flash
or RAM for its existence. Measured at (2), not assumed: the shared rotation source is
byte-identical across all four binaries, and `nm` finds zero rotation/settler/up-axis symbols
in `host-monitor`. The seam from (3) costs `host-monitor` +260 B of ~1 MB and the other two
+4 B, while `orientation` shrank 40 B — inlining jitter, not linked logic.

Keeping it true is a constraint on the remaining beads: `spawn_rotation` stays generic, and
nothing non-generic is referenced from a shared path every binary walks. Re-measure `size` on
all four elfs before closing anything that touches a composition root or a shared path.
