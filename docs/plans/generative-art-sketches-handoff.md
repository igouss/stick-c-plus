# Handoff — the `generative-art` gallery: flash it, then build the four remaining sketches

You are picking up mid-stream on the **stick-c-plus** board platform (M5StickC Plus, Rust,
std/ESP-IDF, hexagonal/ECB). The gallery **skeleton is built and on-glass-ready**; your job is
to (1) **verify it on the metal**, then (2) fill in the four unbuilt sketches, one commit each,
then (3) the DMA/dual-core pipeline and the docs.

Read these first, in order:
- `CLAUDE.md` (laconic voice, hexagonal/ECB, no unsafe, Gherkin, distrust convenient greens).
- `docs/plans/generative-art-gallery-handoff.md` — **the original brief**. It carries the three
  Dwitter sources (squares/fan/orbits), the hardware truth, and the optimisation doctrine
  verbatim. Everything it says still holds; this document does **not** repeat the JS or the
  per-sketch optimisation notes, so keep it open.
- The current code: `apps/generative-art/` (art-core, plume-core, art-display, art-shell) and
  `firmware/apps/generative-art/bin`.

## Where things stand

Branch **`generative-art`**, two commits ahead of `main` (the `platform-numerics` lift and the
`Sketch`/`Selector` domain already landed on `main` in an earlier session):

- `bcaac91` — the gallery renders, folding the plume in.
- `da8ca20` — the front button cycles the gallery on the board.

What exists and is **host-verified** (tests + goldens + clippy + hex-lint + effect-audit clean
both workspaces; the firmware bin cross-compiles and links for Xtensa):

- **`art-core`** [domain] — `Sketch` (the running order: `Plume, Squares, Fan, Orbits, Willow`)
  and `Selector` (advance/wrap). `Sketch::ALL` is the single source of order; a
  compiler-exhaustive witness test fails if a variant is left out.
- **`plume-core`** [domain, now in the `generative-art` context] — the plume's parametric field
  and phase clock. Moved here from the retired standalone app; the standalone `plume` and
  `plume-display` are **gone** (one copy of the frond).
- **`art-display`** [port-and-adapter] — `Frame` (one heap `Rgb565` offscreen buffer, blit-once,
  also a `DrawTarget`), `GalleryView` (Animated; its **anchor is the selected `Sketch`**, so a
  switch resets the animation clock), and `Gallery::render` — an **exhaustive match on `Sketch`**
  that dispatches to a per-sketch rasteriser. The plume is real; the other four draw an honest
  `placeholder` ("SQUARES / coming soon", etc.). Goldens pin every screen.
- **`art-shell`** [driving-adapter] — `SharedSelector` and `spawn_input`: the front-button
  (G37) thread that advances the selector on a click. Host-tested through a full press-release.
- **`firmware/apps/generative-art/bin`** — the composition root, pinned portrait, 16 KiB display
  stack, 30 fps. `just run-generative-art` builds/flashes/monitors it.

The gallery's plume at phase 0 is **byte-identical** to the standalone's old render (verified with
`cmp`), so nothing regressed.

## STEP 0 — flash it and watch, before anything else

**No one has put this on the board yet.** A green host build is not a working device (see the
`firmware-green-host-not-device` lesson). Before writing a single sketch:

```sh
just run-generative-art     # build + flash + monitor  (board on /dev/ttyUSB0)
```

Confirm on the serial log and the glass:
- It boots clean — **no reboot loop, no double-exception, no task-watchdog**. (If it boot-loops,
  suspect a stack overflow from a large buffer built on a stack — everything panel-sized must be
  `vec!`/heap; see `large-buffer-heap-not-stack`.)
- The **plume breathes** (piece 1), and a **front-button press cycles** through SQUARES → FAN →
  ORBITS → WILLOW → back to the plume. The four are "coming soon" placeholders for now — that is
  correct and expected.
- Note the plume's **on-device frame time / fps** (the display thread logs its Hz; measure real
  paint time if you can). This is your baseline before the expensive sketches.

If it does not behave, fix that first — the skeleton is the foundation every sketch rides.

## Then: the four sketches, one commit each

Order: **squares → fan → orbits → Willow** (cheap to expensive, original last). For each:

1. **Measure the fit first.** Each Dwitter is authored for a square 500×500/600×600 canvas; the
   panel is 135×240 portrait. Write a throwaway host sim (Python/JS, as was done for the plume),
   measure the real content bounding box, and derive scale + offset from data. Decide per sketch:
   fill-width-and-crop, or fit-whole-and-letterbox. **Document the choice.**
2. **Pure math in the domain.** Put the sketch's frame-from-phase math in `art-core` (a new
   module per sketch) or `plume-core`-style if it grows large. `f32` throughout (f64 is soft-float,
   10–50× slower). Reuse `platform_numerics::SinTable` for all trig. Hoist every loop-invariant
   into a startup table — the original brief's per-sketch notes tell you exactly which (squares'
   `x+y*2`; fan's distance + hue tables + an HSB→Rgb565 LUT; **orbits' `acos(cos θ)` is a triangle
   wave** — no transcendentals — and its `noise(x,y)` is static, bake it once).
3. **Rasterise in `art-display`.** Add the sketch's rasteriser module under `src/sketch/`, and
   **replace its arm** in `Gallery::render`'s match (turn `placeholder::render(..., Sketch::Foo…)`
   into `foo::render(...)`). Add colour LUT helpers (grayscale→Rgb565, HSB→Rgb565) as a
   `colour` module when the first colour sketch needs them — not before (no dead code; STEP 0
   rule).
4. **Prove it.** Per the verification bar: `proptest` that the `f32`/LUT port tracks an `f64`
   reference (and the triangle-wave tracks `acos(cos)`); unit tests at zero/one/many; then
   **render a screenshot and LOOK at it** against your sim — a green pixel-count test does not
   prove the picture (`goldens-cannot-see-the-picture`, `ask-for-the-shape-not-the-verdict`). The
   screenshot path already exists: `examples/common/scenes.rs` renders every piece; the sketch's
   placeholder still becomes a real screen there.
5. **Re-bless the golden** for that sketch's screen (`just screens-bless`, or
   `BLESS_GOLDENS=1 cargo test -p art-display --test goldens`) — the placeholder→real change is a
   *blessed* golden, and the bless is your record that the picture changed on purpose. **Look at
   the blessed PNG** before trusting it.
6. **Flash and watch** each new sketch on the metal, and **record its real fps**. If a sketch
   can't hold 30 fps, say so and why — do not fake it.

**Willow** (piece 5) is **not a port** — design an original generative frond built for this
pipeline from the first line, cheap to animate, genuinely fluid, with a short design note on what
it is and why it's cheap.

## Then: the pipeline, then close out

- **DMA double-buffer + dual-core compute** (`Task #10`) — do this **after orbits exists**, so
  "measure the win" has real load. Two `Rgb565` framebuffers, DMA-blit frame N while computing
  N+1; split per-cell math across both 240 MHz cores via `ThreadSpawnConfiguration { pin_to_core }`
  (safe, no unsafe). Verify esp-idf-hal SPI DMA support and extend the `Panel` adapter if needed.
  **Measure that it actually helps** before committing the complexity.
- **Docs + fps + merge** (`Task #11`) — record on-device fps per sketch in the README/justfile
  docs; confirm the button cycles on glass; merge `generative-art` → `main` like the plume work
  did.

## Non-negotiables (paid for in blood — see the original brief and the memory)

- **No SIMD.** This is a classic ESP32 (Xtensa LX6). Dual-core + DMA is the parallelism. Refuse
  any "SIMD-optimized ESP32" build — it's a false green.
- **No unsafe.** Reach DMA/core-pinning through safe esp-idf-hal/-svc wrappers only.
- **Big buffers on the heap, never the stack** (`vec!`/`Box` from the start). A panel-sized stack
  temporary double-faults on bring-up and the host never sees it.
- **f32 everywhere; SinTable for trig; hoist every loop-invariant.** The ceiling is the panel
  (SPI), not the math — one offscreen frame, one blit per frame.
- **The metal is the truth.** Flash and watch serial before claiming any sketch works; measure
  real fps. An honest "could not verify on the board" is fine; a fabricated green is the worst
  thing you can do.
- `just fmt` **last**, once, directly. The pre-commit hook (`just precommit` = fmt + hex-lint +
  apps-check) rejects an unformatted tree. `just ci` is the full gate.

## Handy commands

```sh
cargo test -p art-core -p art-display -p art-shell -p plume-core   # host suites
just screens                 # render every gallery screen to target/screens/*.png (then LOOK)
just screens-bless           # accept the current render as the new goldens (after an intended change)
just hex-lint  && just audit # architecture gates, both workspaces
just build-generative-art    # cross-compile the Xtensa bin (links; does not flash)
just run-generative-art      # build + flash + monitor on the board
```

Land each sketch behind its own scoped commit; keep the branch green; flash before you call any
of it done.
