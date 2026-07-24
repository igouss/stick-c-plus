# Handoff — the `generative-art` gallery: four sketches + an original plume, button-switched

You are picking up a task on the **stick-c-plus** multi-app board platform (M5StickC Plus,
Rust, std/ESP-IDF, hexagonal/ECB). Read `CLAUDE.md` and `README.md` first, then the existing
**plume** app (`apps/plume/`, `firmware/apps/plume/bin/`) — it is the template and the first
member of what you are building.

This document is the *what*, not the *how*. You are smart; figure out the how. But two things
below are non-negotiable because they were paid for in blood on the metal: the **hardware
truth** and the **verification bar**.

---

## The goal

Collect **plume** and **four more generative-art sketches** under a single new bounded context,
`apps/generative-art/`, presented as a **gallery**: the board shows one sketch at a time, and a
**button press cycles to the next**. Every sketch animates smoothly.

The five sketches:

1. **plume** — the existing feathered frond. Move it in / adapt it to the gallery's sketch
   abstraction. Do not regress it.
2. **squares** — a port of Dwitter A (below): a grid of breathing, sign-pulsing nested squares.
3. **fan** — a port of Dwitter B: HSB radial folding quads, hue by distance from centre.
4. **orbits** — a port of Dwitter C: an acos/cos distance-field bloom over a static noise
   texture. The most expensive of the three, and the one with the biggest optimisation win.
5. **your own original plume** — NOT a port. Design a *new* generative frond from scratch,
   built for this pipeline from the first line, aiming for genuinely fluid motion. This is the
   creative ask: make something beautiful that is cheap to animate. Document your design.

### The three Dwitters to port (p5.js, canvas is `W`×`W`)

```js
// A — squares
f=0
draw=_=>{
  f++||createCanvas(W=500,W); background(0)
  for(x=0;x<600;x+=50)for(y=0;y<600;y+=50){
    fill(W); rect(x,y,C=50*sin(x+y*2+f/30))
    fill(0); rect(x+C/4,y+C/4,C/2)
  }
}

// B — fan (HSB)
f=0
draw=_=>{
  f++||createCanvas(W=500,W); background(0); colorMode(HSB,1); d=43
  for(x=0;x<W;x+=d)for(y=-x/2,t=1;y<W;y+=25,t*=-1){
    fill((r=dist(x,y,250,250))/W%1,1,1)
    s=sin((f+r)/30)
    quad(x,y, x+d*t*s,y+25*t, x,y+50*t)
  }
}

// C — orbits (acos + noise)
f=0
draw=_=>{
  f++||createCanvas(W=500,W); noStroke()
  for(x=0;x<W;x+=10)for(y=0;y<W;y+=10){
    c=0
    for(n=0;n<30;n++){
      C=$=>W/PI*acos(cos((f-n)*2*$(1)/30))
      c=max(c, W-(abs(C(cos)-x)+abs(C(sin)-y))*(3+n))
    }
    fill(c*noise(x,y)); rect(x,y,10)
  }
}
```

---

## The hardware truth — read this before optimising anything

**There is no vector unit on this board. Do not try to use one.** The M5StickC Plus is a
classic **ESP32 (Xtensa LX6)** — confirm it yourself: the firmware target is
`xtensa-esp32-espidf`, *not* `-esp32s3`, and the boot log says `chip: esp32 (revision v1.1)`.
The SIMD / PIE vector instructions are an **ESP32-S3** feature and physically are not on this
die; even on the S3 they are reachable only through `unsafe`, which this workspace forbids.
Any "SIMD-optimized ESP32" build is a false green — refuse to write one.

What the chip **actually** gives you, and where the real wins are (optimise in this order —
the ceiling is the panel, not the math):

1. **SPI throughput to the ST7789 is the true frame ceiling.** 135×240×2 bytes ≈ 64 KiB/frame;
   at 40 MHz SPI that is ~13 ms just to move pixels, before any math. So:
   - **Compute into an offscreen framebuffer and blit once** per frame (plume already does this
     — one addressed window, not thousands of pixel pokes). Non-negotiable for all sketches.
   - **DMA + double buffering** is the single biggest smoothness win: two framebuffers, kick a
     DMA blit of frame N while the CPU computes frame N+1, so SPI time hides behind compute
     instead of adding to it. Verify esp-idf-hal SPI DMA support and whether the current `Panel`
     adapter uses it; extend it if not. This is where "smooth" comes from.
2. **Two 240 MHz cores.** This is our parallelism — the honest substitute for the vector unit,
   and better leverage anyway. Split each frame's per-pixel/per-cell math across both cores
   (e.g. top half on core 0, bottom half on core 1), join, then blit. Pin tasks with
   esp-idf's `ThreadSpawnConfiguration { pin_to_core }` (safe, no `unsafe`). ~2× on math-bound
   frames. Measure that it actually helps before committing to the complexity.
3. **Single-precision FPU.** Stay in `f32` everywhere; `f64` is soft-float and 10–50× slower.
4. **~520 KB SRAM, no PSRAM.** Two Rgb565 framebuffers is ~128 KiB — fits, but budget it.
   Consider computing at reduced internal resolution and scaling if a sketch is tight.

### Optimisation doctrine — the "think very hard" part

The Dwitters look expensive but most of their cost is **loop-invariant** — hoist it into a
startup table and each frame becomes cheap:

- **Reuse `plume_core::SinTable`** (a startup-built sine LUT, proven within 1e-3 of libm) for
  *all* trig. It already exists; lift it into a shared place the gallery can use.
- **squares (A):** the 12×12 grid's `x + y*2` is constant per cell → precompute once. Per frame
  per cell is then one `+ f/30`, one LUT-sine, and two integer rect fills. Trivially 60 fps.
- **fan (B):** `dist(x,y,250,250)` and the hue `r/W%1` are constant per cell → bake a distance
  table and a hue table at startup. Per frame per cell is one LUT-sine of `(f+r)/30` and a quad
  fill. Add an **HSB→Rgb565 LUT** (256 hues × full S/V is plenty). No `dist`, no `sin`, no HSB
  math on the hot path.
- **orbits (C):** two headline wins.
  - `acos(cos(θ))` **is a triangle wave** — it has no business calling `acos` or `cos` at all.
    Replace the whole `C=$=>W/PI*acos(cos(...))` with a cheap triangle-wave function of the
    phase. This alone removes 50×50×30×2 transcendentals per frame.
  - `noise(x,y)` is **static** (no `f`) → bake a 50×50 value-noise texture once at startup and
    read it. (You'll need a small value/Perlin noise generator; it runs once, so it can be
    plain.) Also precompute the per-`n` direction the triangle waves sweep.
  This is the sketch where the optimisation mindset matters most; the naïve port will not keep
  cadence, the optimised one will.
- Hoist every loop invariant; avoid per-pixel division; prefer integer scanline/Bresenham fills
  into the RAM buffer over generic embedded-graphics primitives on the hot path.
- **Target 30 fps (33 ms/frame) and *measure it on the metal*** — log frame time, do not guess.
  A paint that overruns its tick budget starves the FreeRTOS idle task and trips the task
  watchdog (see `platform-runtime`'s `MIN_YIELD` docs and the `freertos-tick-floors-thread-cadence`
  lesson). Report the real fps you achieve per sketch; if one can't hit 30, say so and why.

---

## Architecture — solve the broader problem

Don't build five apps. Build **one gallery over a `Sketch` abstraction**:

- **`apps/generative-art/art-core`** (role `domain`, context `generative-art`, `no_std`,
  no `unsafe`): the `Sketch` seam and the gallery selector. A sketch is a *pure* producer of a
  frame from a phase — pick the shape (a trait with `fn render(&self, canvas, phase, size)`, or
  a plain `fn`, or an enum of sketch kinds) that keeps the domain framework-free and testable.
  The **selector** is the pure state: which sketch is current, and "advance to next" on a
  button event, wrapping. Move `plume-core`'s field in here (or have `art-core` depend on it) —
  and lift `SinTable` somewhere both reach without a context violation (it is board-generic; it
  may belong in `platform-*`, e.g. beside or inside `platform-display`/a new `platform-`
  numerics crate — decide and keep `hex-lint` happy).
- **`apps/generative-art/art-display`** (role `port-and-adapter`, context `generative-art`,
  `no_std` + `alloc`): the framebuffer(s), the double-buffer/DMA blit strategy, each sketch's
  rasterisation, and the colour helpers (HSB→Rgb565, grayscale→Rgb565, both LUT-backed). The
  `Animated` view carries the *selected sketch index* (so switching forces a repaint) and
  answers "always animating". On a sketch switch the `anchor` changes so the animation clock
  resets for the new piece (see how the render loop uses `Animated::anchor`).
- **`firmware/apps/generative-art/bin`** (composition root): wire the **button** (front button
  G37 — reuse `platform-input`'s debounce and the firmware `GpioButton`, as pomodoro/orientation
  do) to advance the selector; set up the **dual-core compute + DMA double-buffer** pipeline;
  drive the shared render loop; pin portrait. No sensor, no network.

Then **retire the standalone `plume` app** into this gallery (or keep `plume` as a thin alias —
your call, but don't ship two copies of the frond). Update both workspace `Cargo.toml`s, the
`justfile` (`build-`/`run-`/`screens` recipes), and the `README` app list. Keep `hex-lint` and
`effect-audit` clean in both workspaces.

### Fitting p5 canvases to the panel

Every sketch is authored for 500×500 or 600×600; the panel is 135×240 portrait. **Measure each
sketch's real content bounding box** from a host simulation first (a throwaway Python/JS sim, as
was done for plume), derive scale + offset from data, then match it in Rust and **look at the
rendered output** before trusting any green test. Some sketches are square, not portrait —
decide per sketch whether to fill width (cropping) or fit whole (letterbox). Document the choice.

---

## Verification bar — non-negotiable (this is what "done" means here)

- **Tests prove the rules.** Per core: Gherkin (`cucumber`) for the domain rules + `proptest`
  for numeric fidelity (port tracks an `f64` reference; LUTs track `libm`) + unit tests at
  zero/one/many. Domain stays framework-free, `#![forbid(unsafe_code)]`, host-tested.
- **Screenshots + eyeballs.** A host `--example` renders each sketch (several phases) to PNG via
  the *same* render code the panel runs. A passing pixel-count test does **not** prove the
  picture — *look at it* against your reference sim. (See the `goldens-cannot-see-the-picture`
  and `ask-for-the-shape-not-the-verdict` lessons: green goldens have shipped fabricated
  screens here before.)
- **Gates:** `just hex-lint` and `just audit` clean, both workspaces.
- **The metal is the truth.** Cross-compile (`just build-...`), then **flash and watch serial**
  for a reboot loop / double-exception / watchdog before claiming it works — a green host build
  is not a working device (see `firmware-green-host-not-device`). **Measure on-device frame
  time** and report real fps per sketch. Confirm the button actually cycles on the glass.
- `just fmt` **last**, once, directly (not mid-edit) — the pre-commit hook rejects an
  unformatted tree.

### Gotchas already paid for (don't relearn these)

- **No SIMD** — dual-core + DMA is the parallelism. Don't chase a vector unit.
- **Big buffers go on the heap, never the stack.** plume boot-looped because `SinTable::new()`
  built an 8 KiB array as a stack local before boxing it, overflowing the 8 KiB main-task stack
  into a double-exception. Allocate LUTs/framebuffers with `vec!`/`Box` from the start; a stack
  temporary the size of a LUT will double-fault on bring-up and the host never sees it.
- **Display-thread stack:** plume runs the render loop at **16 KiB** (`DisplayConfig.stack_size`)
  because a full-frame blit streams deeper than the 8 KiB default text renders assumed. A
  colour, double-buffered pipeline may need more — watch the high-water mark on the metal.
- **FreeRTOS tick floor is 10 ms** (`CONFIG_FREERTOS_HZ=100`); a sleep under one tick busy-waits
  instead of yielding. Leave headroom in the frame budget or the idle task starves + WDT fires.
- `unsafe` is forbidden — reach DMA / core-pinning through safe `esp-idf-hal`/`esp-idf-svc`
  wrappers only.
- Screenshots are host-only dev-deps; the firmware build must pull no dev-deps.

### Suggested commit / bead breakdown (small, scoped commits — see `CLAUDE.md`)

1. Lift `SinTable` to a board-generic home; scaffold `generative-art` context + `Sketch` seam +
   gallery selector; move plume in. (host-green)
2. Button-driven sketch switching (the composition root + selector wiring).
3. The DMA double-buffer + dual-core compute pipeline (measure the win).
4. `squares`, then `fan`, then `orbits` — one sketch per commit, each with tests + screenshot.
5. Your original plume — one commit, with a short design note on what it is and why it's cheap.
6. Docs: README + justfile + `just screens`; on-device fps numbers recorded.

Land it behind a branch and merge to main like the plume work did. Flash it and watch it before
you call it done.
