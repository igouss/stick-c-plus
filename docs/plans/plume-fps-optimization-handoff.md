# Plume FPS Optimization — Handoff

**Goal:** push the generative-art gallery's plume from 20 fps to **30+ fps, comfortably**, and make
the motion look fantastic. User directive: "SPI 27→52 MHz + DMA double-buffer + dual-core compute
pipeline." Unsafe is permitted **only** in a dedicated quarantine crate (the rest of the tree keeps
`#![forbid(unsafe_code)]`); `dma-mem` and `esp-metrics` are the existing unsafe crates.

Board: M5StickC Plus, ESP32 Xtensa LX6, `xtensa-esp32-espidf`, 2×240 MHz, ~520 KB SRAM no PSRAM,
`CONFIG_FREERTOS_HZ=100` (10 ms tick). Device on `/dev/ttyUSB0`, user in `dialout`.

---

## STATUS: 50 fps ON THE GLASS, at a *much* fuller frond — **7500 points** (was 6000, was 5000), still 50 fps. SESSION 8 spent a memory campaign turning the paint headroom into +25% density: display stack sized from measurement, field storage chunked, and the frond-compute port reshaped to hand projected pixels. The 100 fps cliff stays closed (SESSION 7). 2× the 25 fps this branch started at, past the 30 fps goal.

Progress ladder (all measured on glass unless noted):
`96 ms (mipidsi gather) → 73 (FastPanel blit) → 68 (SinTable mask) → 43/20fps (plume hoist) →
35 ms / 25 fps (SPI 40 MHz + continuous phase) → 32 ms / 25 fps (memory keystone + root O3) →
27 ms / 33.3 fps (dual-core) → 23.6 ms / 33.3 fps (12-bit RGB444) →
14 ms / 50.0 fps (double-buffer + field shrink, SESSION 6) →
13.1 ms / 50 fps (fast reset, SESSION 7) → 16 ms / 50 fps @ 6000 points (density raised to the
memory edge, SESSION 7) → 17.9 ms / 50 fps @ **7500 points** (memory campaign, SESSION 8)`.

**The 100 fps question is closed.** The next tick line is 10 ms → 100 fps. SESSION 7 measured why
that line can't be crossed at full fidelity, and *proved* it can be crossed by thinning the frond —
then chose fidelity. See SESSION 7 for the cost model and the two null experiments.

---

## SESSION 8 — the memory campaign: 6000 → 7500 points (+25%), still 50 fps

Directive: "memory is a priority — fight fragmentation, optimise stack sizes, show as many points as
fast as we can." Arenas were evaluated and **declined** (they beat fragmentation but not total RAM,
and cost an unsafe allocator crate). The density ceiling was two walls; each got its own lever, all
measured on the glass.

1. **Display stack sized from measurement** (`esp-metrics::minimum_free_stack`, a safe
   `uxTaskGetStackHighWaterMark` wrapper — returns *bytes* on this port, `StackType_t = u8`). The
   render thread peaks at ~3.6 KiB; the 16 KiB was inherited from the host monitor's network
   preemption. Cut to **8 KiB**. Frees 8 KiB and, more to the point, halves the *contiguous* run the
   stack must find — the fragmentation that aborted 6500 (largest hole had shrunk to 14336 B). Zero
   fps cost.
2. **Field storage chunked** (`Vec<Vec<Precomputed>>`, 1024-point/16 KiB chunks, shift+mask index).
   A single field `Vec` — or even two halves — is one big contiguous run the pool-fragmented heap
   can't place past ~6800 (a 56 KiB half aborts). 16 KiB chunks always fit, so the budget is bounded
   by paint and total RAM, not by any one allocation. (Superseded an intermediate two-halves split.)
3. **Frond-compute port reshaped to hand projected pixels.** `FrondCompute::evaluate` now takes the
   canvas size and yields `ScreenPoint`s (i16 x/y + wide) already on the panel, not canvas
   `FieldPoint`s. Two wins: the dual-core far buffer is `Option<ScreenPoint>` (6 B, the `wide` bool's
   niche carries `None`) not `FieldPoint` (12 B) — **frees ~18 KiB** and doubles the largest DMA
   block; and the ~3000 far-half projections move onto the idle Core1 — **paint 16.3 → 14.2 ms at
   6000**. Picture-identical: same projection, only relocated; the `f32→i16` cast saturates so a
   far-off finite point stays off-canvas and clipped. Every golden matched unchanged.

**Landed: 7500 points, measured on the glass — 17.87 ms median paint, 19.06 ms worst case over 1312
frames, 0 over the 20 ms/50 fps cliff, ~1 ms jitter headroom, 49.8 fps.** 7800 also boots and holds
50 fps but leaves only 0.35 ms margin — too thin for a default against thermal drift, so 7500 sits
one step back. The wall now is paint throughput (~8000 crosses 20 ms), not memory.

What did **not** help (measured): the i16 SinTable (frees 4 KiB but adds ~1–2 ms/frame of int→float
on the hot path — a net loss near the ceiling); memory arenas (beat fragmentation, ~6000→~6800, but
never total RAM, and need an unsafe crate). Both were correctly declined.

### DONE this session (uncommitted, host-green; goldens await bless)

**Phase 1 — continuous phase / cadence unlock (host-only, no device dependency).** The 20 fps was
*baked into the phase quantum*, not the hardware: `phase()` floored `elapsed/50ms` to an integer,
capping distinct pictures at 20/sec no matter the repaint rate.
- `plume-core/src/phase.rs`: `phase()` is now a **continuous** function of time —
  `(elapsed_ms % PERIOD_MS) as f32 / FRAME_MS as f32 * PHASE_PER_FRAME`. Same speed, interpolates
  between the source's per-frame steps. Added `PERIOD_MS = PERIOD_FRAMES*FRAME_MS = 24000`. Lands
  exactly on source values at whole-frame multiples of `FRAME_MS`. Tests updated: replaced
  `the_phase_is_constant_within_a_frame` with `the_phase_moves_within_a_frame` +
  `whole_frames_land_on_the_source_steps`. `FRAME_MS=50` stays (it is the *speed* calibration).
- `art-display/src/view.rs`: `FRAME_MS` (33, was cadence+suppression) → **`REPAINT_MS = 10`** (the
  cadence ceiling, set at `MIN_YIELD`). `frame_index(elapsed) = elapsed as usize` (continuous
  suppression key — a continuous animation must not be throttled by a coarse frame window).
- `art-display/src/lib.rs`: re-export `REPAINT_MS` not `FRAME_MS`; doc updated.
- `firmware/apps/generative-art/bin/src/main.rs`: `animation_period = REPAINT_MS`; cadence docstring
  + boot log reworded (no false Hz claim; loop reports achieved fps).
- `art-display/examples/common/scenes.rs`: goldens now sample by `plume_core::FRAME_MS` (50) →
  stills land on true phase-frames 0/24/48/72. (Old goldens sampled at `33*n` ms and *mislabeled*
  frame-048 as what was actually floored-frame 31.)

**Phase 2 — SPI clock (device, boots clean, no sparkle-verify from user yet).**
- `firmware/platform/adapters/src/panel.rs`: `SPI_HZ 27_000_000 → 40_000_000` **and added
  `.write_only(true)`** to the `SpiConfig` in `build()`.
  - WHY write_only: the panel has no MISO. esp-idf rejects >26.7 MHz in **full-duplex** because
    *reads* go unreliable (`spi_hal_cal_clock_conf: ... device cannot read correct data`,
    `spi_bus_add_device: assigned clock speed not supported` → boot loop). `write_only(true)` sets
    `SPI_DEVICE_NO_DUMMY`, which drops the read-timing limit. It is the TRUTH (we never read), not a
    workaround. First 40 MHz flash boot-looped on exactly this; write_only fixed it.
  - WHY 40 not 80: the ESP32 SPI master only synthesizes 80 MHz ÷ N. 27→26.7 (÷3), 40 (÷2), 80
    (÷1). SCLK/MOSI are on GPIO 13/15, which route through the **GPIO matrix, not the SPI2 IOMUX
    pins** — the matrix skew caps reliable *output* near 40 MHz. 80 MHz needs the IOMUX pins the
    board never brought out → off the table without hardware change.
  - Shared const: affects all apps' panels. All are write-only; 40 MHz only helps them. Fine.

Measured after Phase 1+2: **`platform-display: 25.0 fps over 125 frames, paint 35.0 ms`** at
40 MHz, clean boot. "paint" = full `show()` = gallery.paint (compute+flood+plot) + blit (swap+spi).

### PENDING USER ACTION
- **Bless the goldens.** 3 of 4 plume goldens shifted (frame 01 @ elapsed 0 is byte-identical).
  Montage at `target/screens/_compare/plume-goldens-before-after.png` was sent to the user; same
  frond, honest frames. Bless with `just screens-bless` (BLESS_GOLDENS=1) once approved. Until then
  the `goldens` integration test is RED (expected). Lib/unit/property tests are GREEN (23 + 12).
- **Glass check at 40 MHz:** confirm no sparkle/tearing/wrong dots, and that motion reads smoother.

---

## SESSION 2 (2026-07-24): dual-core BUILT + host-proven, blocked by a MEMORY WALL. The keystone is unifying the two full-screen buffers.

**What landed and works (default build, on glass at 25 fps, no regression):** the plume's evaluation
was refactored behind a **`FrondCompute` port** that *streams* the cloud point-by-point straight into
the frame — **no scratch buffer anywhere**. Cleaner architecture, identical pixels, identical fps.
- `plume-core/field.rs`: extracted `point_from` (the shared per-point body); added
  `iter_range(start, len, t, table) -> impl Iterator` (the streaming primitive `frame` and
  `compute_range` both go through) and `compute_range(start, t, table, &mut out)` (buffered form, for
  a worker that must own its output). New proptest `a_split_sweep_is_bit_identical_to_the_whole`
  proves two ranges reassemble the whole frame **bit-identically** — the invariant dual-core rests on.
- `art-display`: new `frond.rs` = `trait FrondCompute { fn evaluate(&mut self, t, plot: &mut dyn
  FnMut(FieldPoint)); }` + `SerialFrond` (one-core default, streams the whole field, zero per-frame
  alloc). `sketch/plume.rs`: slice `plot` → per-point `plot_point`. `gallery.rs`: holds
  `Box<dyn FrondCompute>` + frame (no scratch); `with_frond(closure)` builds the frame **before** the
  frond (big buffers claim their pool runs first). All host-green (28 art-display + 13 plume-core).

**Dual-core (Lever A) is BUILT and host-proven, but does not fit the heap.** It lives in
`firmware/apps/generative-art/bin/src/dual_core.rs` (`DualCoreFrond`: a persistent Core1 worker,
mpsc ping-pong of an owned far-half `Vec`, near half streamed on the render thread — all SAFE, no
`unsafe`). It is behind the **`dual-core` cargo feature (off by default)** — `just`-built default is
one-core; `cargo build -p generative-art --features dual-core` builds the two-core binary (compiles
clean, but OOMs on device — see below). `plume-core` is an optional dep pulled in only by that feature.

**THE MEMORY WALL (measured on device — do not re-derive).** Free heap after rails-up is
**297 KB**, but it is **fragmented across four pools** (`heap_init`: 6 K + 178 K DRAM, 14 K + 111 K
D/IRAM); a large allocation must fit **one pool's contiguous run**. The app's big buffers already are:
DMA frame buffer **63 K** + offscreen Frame **63 K** + precomputed field **~100 K** + sine table 8 K
= **~234 K**. Dual-core needs a **30 K far buffer + a 4 K worker stack**, and the render thread needs
a **16 K stack** — and there is no contiguous run left for them. Four OOM data points walked the wall:
`60 K scratch → 63 K frame → 30 K far → the 16 K (then 12 K) display-thread stack`. Allocating the
big/awkward buffers **first** (frame before frond; far before the 100 K field) pushed the failure all
the way to the display-thread stack, but that is **razor-thin and fragile**, not a fix — the classic
"easy over clean" this project rejects. **Double-buffer (Lever B) needs a *second* 63 K DMA buffer —
hopeless without freeing real memory first.**

**THE KEYSTONE: unify the two full-screen buffers into one.** The Frame (Rgb565, 63 K) and the DMA
buffer (u8 wire-order, 63 K) are *separate*, with a per-frame CPU swap between them (~2.4 ms). Unify
them into a **single DMA-capable buffer the sketches plot into directly** (byte-swapped in place
before the DMA, or stored wire-order): **frees ~63 K** (turns the razor-thin fit comfortable, unblocks
BOTH levers) **and removes the 2.4 ms swap** (a direct fps win). This is the real next step; with it,
`show` ≈ compute + spi and the cadence math below reaches 30 fps (dual-core) / 40 fps (both levers).
- **The one design challenge:** the Frame must abstract over its buffer's *provenance* — host owns a
  `Vec` (tests/goldens), device is handed a `&'static mut [u8]` from `dma_mem`. Cleanest bound: keep
  `Gallery::new()` (host) self-allocating a `Vec`-backed Frame so **all host tests stay unchanged**,
  and give the device path a `Frame::from_dma(buf)` + a `Gallery::with_frond(frame, closure)` that
  takes the pre-built frame — so the ripple is just `frame.rs` + `gallery.rs` + `panel.rs` + `main.rs`.
  Frame stays a `DrawTarget<Color=Rgb565>`; only its storage/blit change. **Watch:** the plume is
  monochrome (white/black survive a byte-swap unchanged) so a swap bug is **invisible on the plume** —
  verify with the *colored* placeholder goldens + a unit test asserting `set(colour)` → wire bytes.

**New diagnostics (kept, clean):** `dma_mem::free_default()` / `dma_mem::largest_free_dma()` (safe heap
queries in the one unsafe-allowed crate); `main` logs `heap after rails up: … free, largest DMA block …`.

---

## SESSION 3 (2026-07-24): the optimization arsenal — "throw everything at it"

User directive: *"throw every optimization you know of, from Hacker's Delight to Quake-engine crazy, at this ESP32."* Here is the full arsenal, **ranked by payoff ÷ risk**, each tagged with the measured
cost it attacks and three honesty markers — architecture reach `[SAFE]`/`[QUARANTINE]`, pixel
fidelity `[BIT-IDENTICAL]`/`[REBLESS]` (within table tolerance, goldens re-blessed)/`[PICTURE]`
(deliberate visual change)/`[MEMORY]` (frees heap, no pixel change), and the expected win. Nothing
here is speculation — it is grounded in the code read this session. **Distrust-convenient-signals
note:** two famous tricks are *traps on this chip* and are listed as such (Tier 4) — do not add them.

### Tier 0 — free & structural, do first (they unblock or amplify everything below)

- **T0.1 Per-crate `opt-level = 3` for `plume-core`/`art-display`.** ~~`[SAFE][BIT-IDENTICAL]` ·
  attacks compute~~ — **TRIED 2026-07-24, NULL RESULT, reverted. Do not re-add.** Adding
  `[profile.release.package.{plume-core,art-display}] opt-level = 3` to `firmware/Cargo.toml`,
  rebuilt and flashed, measured **25.0 fps / paint 35.0 ms — bit-for-bit identical to the `"s"`
  baseline.** The cause is `lto = "fat"`: the whole dependency graph's IR is merged and re-optimized
  as a single module at the **root** profile's `opt-level` (`"s"`), so a per-package override on the
  initial codegen is washed out by the final LTO pass. The *only* way to force `opt-level=3` onto the
  hot path is to raise the **root** `[profile.release] opt-level`, which fights the deliberate
  "`s` everywhere — `-O0` overflows IRAM" convention (line 46) and bloats the whole firmware — and the
  null targeted result is evidence the tight hoisted field loop isn't codegen-bound anyway (it's
  table-lookup + FPU bound). **Skip it; the real compute win is dual-core (T1.2), not the optimizer.**
- **T0.2 THE KEYSTONE — unify Frame + DMA into one wire-order buffer** (Task #17, full design in
  SESSION 2). `[SAFE][BIT-IDENTICAL pixels][MEMORY]` · attacks swap (2.4 ms) + memory wall. Frees
  **~63 KB** (dual-core's 30 KB + stacks then fit with room) and deletes the 2.4 ms byte-swap. This
  is the linchpin: **Levers A and B cannot land on the device without it.** Everything else is
  multiplied by doing this first.

### Tier 1 — the big levers (each measured-large; combine for the ceiling)

- **T1.1 12-bit colour (RGB444, ST7789 `COLMOD 0x3A = 0x03`).** `[SAFE][PICTURE, but invisible on the
  plume]` · attacks SPI (11.6 ms) **and** memory. 16 bpp → 12 bpp packs 2 px into 3 bytes: the frame
  drops **64 800 → 48 600 bytes**, so the DMA burst falls ~25 % (11.6 → **~8.7 ms**) *and* the unified
  buffer shrinks another ~14 KB. The plume is pure white on black — 444 loses **nothing** visible; the
  placeholder sketches use flat colours that survive 444 cleanly. Fold this into the keystone: the one
  unified buffer stores **wire-order 444** and the panel's `COLMOD` is set at bring-up. Re-bless
  goldens (this is a deliberate depth change). *The single highest-leverage idea in this list* — it
  helps compute-free the SPI **and** the heap at once.
- **T1.2 Dual-core compute (Lever A).** `[SAFE][BIT-IDENTICAL]` · attacks compute — already built &
  host-proven (`dual_core.rs`, feature `dual-core`), blocked only by memory. Halves field 18 → ~9 ms.
  Flip on after T0.2 (+ ideally T1.1) frees the heap.
- **T1.3 Non-blocking DMA double-buffer (Lever B).** `[QUARANTINE]` · attacks SPI (11.6 ms) — overlaps
  the transfer with the next frame's compute **and the 10 ms yield**. The elegant payoff with T1.1:
  once SPI is ~8.7 ms it fits **entirely inside the mandatory 10 ms yield** → the transfer costs
  **zero** wall-clock. Needs a 2nd full-screen buffer (hopeless before T0.2). Design in SESSION 2 /
  Lever B below.

### Tier 2 — compute micro-architecture (Hacker's-Delight tier, small but safe)

- **T2.1 Software-pipeline the point loop.** `[SAFE][BIT-IDENTICAL]` · compute. Unroll the
  `point_from` sweep ×2–4 and **interleave independent points** so the LX6 FPU's multi-cycle add/mul
  latencies overlap instead of stalling serially (each point's own op sequence is unchanged →
  bit-identical). The LX6 has no SIMD, so this hand-pipelining is the only ILP lever. Try in
  `iter_range`/`compute_range`; measure — the compiler at `opt-level=3` (T0.1) may already do some.
- **T2.2 Fuse the `sin(c)`/`cos(c)` index computation.** `[SAFE][REBLESS]` · compute. Both use angle
  `c = d − t`; `cos = sin(θ+π/2)` and π/2 is **exactly LEN/4 = 512** table entries, so one
  `theta*INDEX_PER_RADIAN → floorf → frac` serves both lookups (cos reads `(whole+512)&MASK`, same
  `frac`). Saves a `floorf`+multiply per point over 5000 points. **Not** bit-identical (the current
  `cos()` rounds `θ+π/2` before scaling — different bits), but within the table's 1e-3 tolerance;
  cleanest as a new `SinTable::sin_cos(theta) -> (f32, f32)` and one rebless. `trig.rs` already owns
  this contract, so the change is local.
- **T2.3 Hoist the remaining `t`-invariants into `Precomputed`.** `[SAFE][REBLESS]` · compute.
  `y = q·sin(c) + d·39 − 475`: the `d·39 − 475` turns only on the per-index `d` → precompute it as one
  field `d39`. Likewise `q = a + b·(9 + 2·swirl)` distributes to `a + 9b + 2b·swirl` (precompute
  `q_base = a + 9b`, `b2 = 2b` → `q = q_base + b2·swirl`). Each saves a mul+add per point. **Not
  bit-identical** (both reassociate the f32 adds — `A+(B−C) ≠ (A+B)−C`), so rebless; within tolerance.
  Pairs naturally with T3.1 (rebuilding the struct anyway).
- **T2.4 Pin the hot loop in IRAM.** `[SAFE][BIT-IDENTICAL]` · compute. If `point_from`/`iter_range`
  execute from flash, they pay MMU-cache-miss stalls; `#[link_section = ".iram1.text"]` (or the
  esp-idf `IRAM_ATTR` equivalent) makes them single-cycle-fetch. Internal SRAM has no data cache, so
  the *table* placement is already optimal — this is purely about instruction fetch. Cost: scarce
  IRAM (the D/IRAM pools — the same ones the memory wall fights over), so measure the trade vs. the
  stacks it competes with. Try only after the heap is freed (T0.2/T1.1).

### Tier 3 — shrink the field (feeds the memory wall directly)

- **T3.1 Drop `wide: bool` from `Precomputed` → a bitset.** `[SAFE][BIT-IDENTICAL][MEMORY]`. The bool
  costs a byte + 3 padding (struct is 4×f32 aligned = 20 B). Move `wide` to a side `Vec<u64>`
  (5000 bits ≈ 640 B) → `Precomputed` is **16 B**, field **100 → 80 KB, −20 KB**. Same data, same
  pixels. Do this while rebuilding the struct for T2.3.
- **T3.2 (aggressive) `f16` storage for the field invariants.** `[SAFE][PICTURE — glass-verify]`.
  `swirl_base`/`a`/`b`/`d` feed a table that quantizes to 2048 steps anyway; storing them half-width
  could roughly halve the field (~80 → ~40 KB) — but f16's ~3-decimal-digit mantissa may shift an
  angle past a table slot and move dots. High memory reward, real picture risk. Only if the wall is
  still tight after T3.1, and **verify on the glass**, not just host.

### Tier 4 — the famous tricks that DON'T apply here (documented so nobody re-adds them)

- **T4.1 Fast inverse square root (`0x5f3759df`).** *N/A.* The field's one `sqrtf` (`d = |(k,e)|`) is
  **already hoisted off the render path** — paid once in `precompute`, never per frame. Quake's trick
  targets a per-pixel `1/√x` this loop simply does not have.
- **T4.2 Fixed-point / integer field math.** **Wrong on this chip — a myth that's false here.** The
  Xtensa LX6 has a *hardware* single-precision FPU (add/mul/sqrt in a handful of cycles), so `f32` is
  already near-free; fixed-point would add shift/round/convert overhead at every table boundary and run
  **slower**, while giving up the proptest-backed `f64`-reference correctness. Fixed-point wins on
  soft-float MCUs (Cortex-M0), not this one. Do not do it.
- **T4.3 Minsky / rotating-vector angle recurrence (no table).** The demoscene "advance sin/cos by a
  fixed rotation each step" trick. `trig.rs`'s own module docs already rejected it and are right: four
  of the field's angles are **data-dependent** (`d`, `t`, `k`) and no recurrence can produce them, and
  the two index-linear ones (`k`, `sin(i/19)`) are **already fully precomputed** (they're
  `t`-invariant). A *per-frame* recurrence (advance each point's angle by the fixed phase delta) is
  conceivable but would need **+40 KB of per-point sin/cos state** (fighting the very memory wall),
  would **break the "frame N is a pure function of phase" property** the whole test suite rests on, and
  **drifts** (needs periodic re-seed). Verdict: architecturally too expensive; hold unless compute is
  *still* the bottleneck after Tiers 0–2, which the math below says it won't be.

### The ceiling this arsenal reaches (honest projection)

Stacking **T0.1 + T0.2 + T1.1 + T1.2 + T1.3 + T2.x + T3.1**:

| cost | now | after arsenal | how |
|---|---|---|---|
| field compute | 18 ms | ~7–8 ms | dual-core halves (9) + opt-level=3/pipeline/hoists (~−15 %) |
| flood+project+plot | 3 ms | 3 ms | unchanged |
| byte-swap | 2.4 ms | **0** | keystone (wire-order) |
| SPI transfer | 11.6 ms | **0 wall-clock** | 444 → 8.7 ms, hidden under double-buffer + 10 ms yield |
| **P (thread work)** | 35 ms | **~10–11 ms** | |
| **+ MIN_YIELD** | +10 (in P) | +10 ms | the immovable floor (Tier 5) |
| **cadence** | ~40 ms | **~20–21 ms** | |
| **fps** | **25** | **~45–50** | |

**Comfortably past 30 — practical ceiling ~45–50 fps.** Beyond that the two co-dominant costs are the
~10 ms compute and the 10 ms yield; the only further levers are T4.3 (rejected) or Tier 5 (fenced).
Memory freed by the stack: keystone −63 K, 444 −14 K, wide-bitset −20 K ≈ **−97 KB** — the wall is
gone, dual-core + double-buffer both fit.

### Tier 5 — the final boss: the 10 ms MIN_YIELD floor

Once the levers land and P ≈ 10 ms, the mandatory `MIN_YIELD = 10 ms` per frame is **~half the frame
budget** and the hard ceiling. It is fenced for good reasons (HZ=100 is load-bearing across the
platform; sub-tick sleeps busy-wait and starve IDLE/TWDT — see the cost model + memory). The only
*within-rules* nibble is the one T1.3 already takes: run the async DMA **during** the yield so the
transfer is free. Cracking the yield itself (per-N-frame full yield + manual TWDT feed on the others)
is fragile and out of scope — flagged here only so the ceiling's location is understood, not chased.

---

## SESSION 4 (2026-07-24): the keystone is BUILT and frees the memory — but exposed a flash-i-cache fps cliff. UNCOMMITTED, fps-regressed, needs the IRAM fix before it can land.

The keystone (Task #17) is implemented the clean hexagonal way you chose: a **`Canvas` port** in
art-display (`src/canvas.rs`, `Rgb565` `set`/`reset`, panel-agnostic), the host `Frame` as one
adapter, and a new **`wire-canvas` crate** (`apps/generative-art/wire-canvas`) as the device adapter
that stores each pixel in the ST7789's big-endian wire order in a **borrowed** byte buffer — the one
DMA buffer the gallery plots straight into and the panel streams with a bare `RAMWR`. `FastPanel`
lost its owned buffer and the per-frame swap. **Host-green** (art-display 27 + plume-core 13 +
wire-canvas 4) and **host-proven byte-identical**: a `wire-canvas` proptest asserts `WireCanvas` ==
`Frame` byte-for-byte over random plots (the check the monochrome plume can't give on glass). The
goldens fail only in the pre-existing continuous-phase set (plume 02/03/04) — the refactor changed
**zero** pixels.

**Memory win — CONFIRMED on device.** New boot log `heap after buffers`: **122,928 B free, largest
DMA block 94,208 B** (with the ~63 K canvas + ~100 K field + 8 K table all live). Where two
full-screen buffers had left it razor-thin, there is now real slack — dual-core's 30 K far buffer
fits easily. This was the keystone's whole point and it delivered.

**But fps REGRESSED to 95 ms / 10 fps (steady, three reports) — a flash instruction-cache cliff.**
Diagnosis (measured, PM is OFF so it is not frequency scaling — `CONFIG_PM_ENABLE` unset, CPU fixed
at **160 MHz**, not 240):
- Adding a single per-frame `info!`/`Instant::now()` to `show()` drops it **95 ms → 36 ms**
  (≈ the 35 ms baseline). A *code* change moving timing 2.6× while PM is off is the fingerprint of
  **flash i-cache thrashing**: the new WireCanvas code shifted the hot loop into a cache-hostile
  layout, and the extra code shifts it back out.
- It is **not** the data path: in the instrumented build the split was `paint_into 22.8 ms + blit
  13.2 ms`, i.e. plotting *into the DMA buffer is fast there*. Same DMA target, same plot code — only
  the surrounding code layout differs. So the unified-buffer design is sound; the regression is
  layout, not "DMA memory is slow to plot."
- Heisenberg caveat: you cannot measure the slow build's internal split, because instrumenting it
  makes it fast. Trust the mechanism, not a convenient in-loop timer.

**RESOLVED by root `opt-level = 3`, not IRAM.** Setting `firmware/Cargo.toml [profile.release]
opt-level` `"s" → 3` (the apps are small; Rust app code grows flash `.text`, not IRAM) cleared the
cliff: **25 fps / paint 32 ms, steady over three reports**, memory win intact (122 K free). Under
`lto = "fat"` the *root* opt-level reaches the hot path (a per-package override does not — Task #18),
and its tighter codegen also relaid the render loop out of the thrash. Net **better than the 35 ms
pre-keystone baseline** (swap removal + O3), with no IRAM in the domain crate — the cleaner fix. IRAM
(arsenal T2.4) is therefore *not needed* for the keystone and stays a future lever only if a later
change reintroduces a cliff.

**State:** keystone + root-O3 verified on the board (25 fps, 32 ms, 122 K free, byte-identical
picture by construction). Ready to commit, then flip `dual-core` (now fits) and measure toward 30+.
Files: art-display `canvas.rs` (new) + `frame.rs`/`gallery.rs`/`plume.rs`/`placeholder.rs`/`lib.rs`/
`scenes.rs`; new `wire-canvas` crate; `panel.rs` (FastPanel); bin `main.rs`/`Cargo.toml`; workspace
`Cargo.toml` (+ member); `firmware/Cargo.toml` (root O3).

---

## SESSION 5 (2026-07-25): dual-core flipped on — 30 fps GOAL CLEARED at 33.3 fps on the glass.

The memory keystone (SESSION 4) freed the heap the dual-core worker needed, so `dual-core` was
flipped from opt-in to **default** and measured on the board:

- **25.0 fps → 33.3 fps, steady** across 7 consecutive 5 s reports (33.3 fps over 167 frames each).
- **paint 32 ms → 27 ms/frame** (avg == peak == 27 ms — dead flat, no jitter).
- **Heap: 83,328 B free after all three buffers** (wire-canvas frame + worker far-half + 4 KiB
  worker stack), largest DMA block 55,296 B. Comfortable; no fragmentation trouble.
- Clean boot, no panic, no reboot loop, no watchdog. The far/near buffer ping-pong (SAFE, no lock,
  no unsafe) held up in the field exactly as the host property proved.

**Why 5 ms and not the ~9 ms the model predicted:** dual-core halves only the *field compute*; the
*plot* of both halves and the *blit* stay serial on the render thread, and the barrier `recv()` eats
a sliver of FreeRTOS wakeup latency. The near half's compute+plot overlaps the worker's far compute,
so the win is `~min(far_compute, near_compute+near_plot)` ≈ 5 ms, not the full half of compute. The
cost-model row below (`~26 ms`) was close; measured 27 ms.

Landed: `firmware/apps/generative-art/bin/Cargo.toml` — `default = ["dual-core"]` + reworded the
feature comment (now on by default, measured, fits since the keystone). No code change; `dual_core.rs`
and the `#[cfg(feature)]` arm in `main.rs` were already in place from SESSION 2. Default build
recompiles clean (13 s), flashes, runs. Task #10 done.

**Next for the comfortable 40:** Lever B, the non-blocking DMA double-buffer (Task #16, unsafe,
quarantined) — hides the ~13 ms blit under the next frame's compute+yield. Cost model projects
dual-core + double-buffer → ~40 fps. See "NEXT: two levers" → "Lever B" below (design already scoped).

---

## SESSION 6 (2026-07-25): 12-bit RGB444 + non-blocking double buffer → 50 fps. The ceiling.

Two levers landed on top of dual-core, plus the memory work that made the second fit.

**12-bit RGB444 (Task #19, committed 43069f2).** COLMOD 0x33; the wire canvas packs two pixels per
three bytes (`[R0 G0][B0 R1][G1 B1]`). A frame is 48,600 B, 25% less than 16-bit. On the monochrome
plume the colour loss is nil (white 0xFFFF → 0xFFF). **fps stayed 33.3** — cadence is
FreeRTOS-tick-quantised (30 ms = 3 ticks), and 27 ms and 23.6 ms paint fall in the same 20–30 ms
bucket. Its payoff is not fps but (a) heap: buffer 64.8 → 48.6 KiB, and (b) it makes the
double-buffer's *second* full-screen buffer fittable at all. Host-proven byte-identical to the
12-bit-quantised Frame (wire-canvas proptest); glass confirmed correct by the user.

**Field shrink (Task #21, committed b26fdca).** `Precomputed`'s `bool wide` padded the 4-`f32` record
16 → 20 B; moved to a 1-bit-per-point bitset → field 100 → 80 KiB, freeing ~19 KiB. Bit-identical
(the plume-core properties stay green). This was the unblock: without it the double-buffer's second
48.6 KiB buffer left too little contiguous heap for the 16 KiB display-thread stack — `pthread:
Failed to create task!`, boot loop. With it, heap-after-buffers 50,976 → 70,336 B free.

**Non-blocking double buffer (Task #16, committed 2ef93dd).** New quarantine crate `spi-dma`
(`DoubleBuffer`) queues the transfer on interrupts (`spi_device_queue_trans`/`get_trans_result` on
the raw handle) and ping-pongs two buffers. FastPanel keeps RAMWR + DC; `present()` = reap-prior +
RAMWR + queue-current. The prior transfer runs under this frame's compute, so the reap is instant.
**paint 23.6 → 14.0 ms → 50.0 fps**, steady, no tearing (glass), clean boot, 70 KiB free. 14 ms
crosses below the 20 ms tick line — that step is the whole gain (see the tick-quantisation note in
the cost model / the `freertos-tick-floors-thread-cadence` memory).

**Why this is the ceiling for the plume.** Cadence snaps to 10 ms tick lines. To beat 50 fps (20 ms)
the render thread must drop under 10 ms, but its floor is the ~14 ms of near-half compute + plot +
present. Getting there would need the *plot* off the render thread too (a third stage) or a much
cheaper field — diminishing returns. 50 fps is the clean stopping point.

**Soundness note (spi-dma).** The `unsafe` is the two C calls, contained. `queue`/`reap`/`back` are
safe because `DoubleBuffer` alone advances the ping-pong index and never lends the in-flight buffer,
and the buffers are `'static` — no dangling, no tearing. `unsafe impl Send` is justified: moved to
the display thread, used only there (same guarantee esp-idf-hal makes for `SpiDeviceDriver`).

---

## SESSION 7 (2026-07-25): the 100 fps cliff — probed to its floor, proved reachable only by thinning the frond, and declined. Density raised to 6000 instead.

User directive: keep optimizing past 50 fps "for the love of the game." The one remaining tick line
is 10 ms → 100 fps. The steady paint was **14.57 ms**; crossing 10 ms needs a ~3.1 ms cut. This
session found where those milliseconds are and why they won't move at full fidelity.

**The measured breakdown (temporary `Instant` instrumentation in `evaluate` / `show`), steady state:**
`paint_into 14.54 ms` + `present 0.23 ms`. Inside `evaluate`:

| phase | cost | note |
| --- | --- | --- |
| reset (flood 48.6 KB) | ~1.47 ms | byte-wise `chunks_exact_mut(3)` |
| near compute + plot (2500 pts) | 9.55 ms | render thread: ~6 ms compute + ~3.5 ms plot |
| barrier-wait | **0.003 ms** | Core1's far-compute is *fully hidden* — Core1 idles ~7 ms/frame |
| far-plot (2500 pts) | 3.51 ms | plot only |

Per-point: plot ~1.4 µs (~225 cyc for ~50 insns → ~4.5 CPI), compute ~2.4 µs. Both stall-heavy.
Total essential work ≈ compute 12 + plot 7 + reset 1.5 ≈ **20.5 ms**. Two cores → ~10.25 ms floor —
**and the plot (7 ms) can't be split** (points scatter across the whole frame by index, and the
RGB444 middle byte is shared, so two cores racing it corrupt pixels). So ~10.25 ms is the 2-core
floor: still 50 fps. Sub-10 ms needs *less work*, not more parallelism.

**Win that landed — fast reset (committed).** The ground is black, so the flood is a plain `fill(0)`
(memset the compiler vectorises), not a per-3-byte stamp. General: any `r==g==b` ground (black, white)
collapses to one repeated byte. **Reset ~1.5 → ~0.4 ms; paint 14.57 → 13.1 ms**, pixel-identical
(wire-canvas proptest). Still 50 fps (13.1 ms is in the 20 ms tick bucket) — headroom, not a cliff
crossing. `apps/generative-art/wire-canvas/src/lib.rs`.

**Two null experiments (both reverted — do not re-attempt without new evidence):**
- **IRAM the hot loop.** `#[cfg_attr(target_arch="xtensa", link_section=".iram1.text")]` on `evaluate`
  moved ~7 KB into IRAM (free IRAM 71 → 64 KB, so it *did* relocate) — near-compute **9490 vs 9550 µs,
  noise**. The loop is **not** instruction-fetch bound; it's FPU/dependency-chain bound. This kills the
  "2.6× code-layout swing" theory (`flash-icache-cliff-code-layout` memory) *for this loop* — that swing
  belonged to an earlier configuration and does not reproduce here. Release is already `opt-level=3`.
- **Buffer the near half to pipeline it.** Streaming plots via an opaque `dyn FnMut` between every
  compute — a suspected optimizer barrier to cross-point ILP. Computing the near half into a Vec first
  (no call between points) gave near-compute **6195 vs 5940 µs** and paint **rose** to 14.4 ms (the
  buffer round-trip cost ~0.9 ms for zero ILP gain). At `opt-level=3` LLVM already extracts the
  available ILP; the FPU latency is genuinely exposed. Structural pipelining won't help.

**What *does* cross 10 ms — fewer points (proved on glass).** Point count scales both compute and
plot. STEP=3 (5000→3333 pts) → **paint 9.0 ms → 100.0 fps, steady**, +heap. So 100 fps is real —
but it is the *only* lever with enough leverage, and it visibly **thins the frond's barbs**. And the
100 fps is itself invisible (panel refresh ~30 fps). Trading a *seen* downgrade for an *unseen*
number. User declined the trophy.

**The memory ceiling (measured, arena question answered).** Instead, density was raised as far as RAM
allows. Field = 16 B/pt, far buffer = 6 B/pt ⇒ ~22 B/pt; fixed costs (two 48.6 KB DMA buffers, 8 KB
table, 16 KB display stack, 4 KB worker stack) leave ~151 KB for points ⇒ hard ceiling **~6900 pts**.

| points | field | result |
| --- | --- | --- |
| 10000 (STEP=1) | 160 KB | OOM — field alloc fails (`allocation of 160000 bytes failed`) |
| 7000 | 112 KB | OOM — no contiguous field run |
| 6500 | 104 KB | field fits, then **16 KB display stack** can't (largest run 14 KB) → abort |
| **6000** | 96 KB | boots, 50 fps (paint 16 ms), 48 KB free, 16 KB stack fits |

An **arena** (packing the big buffers to defragment) would push ~6000 → ~6800 (+13%, beats
fragmentation) but **never near 10000** — that needs ~220 KB for field+far vs ~151 KB available; the
wall is total RAM, not fragmentation, and only sacrificing the double-buffer or dual-core would free
it (and tank the fps). Judged not worth an allocator for +800 points on a throwaway.

**Committed: 6000 points (was 5000), 50 fps.** Denser, more faithful frond (3/5 of the original
10000, vs 1/2), same fps bucket (16 ms < 20 ms tick). `STEP` (integer stride) is replaced by
`field::index_of` — a rational stride `n·START/POINT_COUNT` that hits any budget across the full
index range (reduces to "every other index" at 5000). Goldens re-blessed (`art-plume-01..04`), the
`5000 points` cucumber scenario updated to `6000`, docs swept for `~5000`/`STEP`. Glass: 49.9 fps
steady, paint 16 ms, no panic.

**If anyone revisits 100 fps at full density:** the only untried structural idea is getting the
*plot* off the render thread — but it can't be split by point index (scatter + shared RGB444 byte).
A spatial (top/bottom-half) split would need per-frame binning of points into regions; unproven it
pays. The compute is FPU-bound and already dual-cored + hoisted. Realistically, 50 fps at 6000 is
the honest ceiling for this picture on this board.

---

## THE COST MODEL (measured, do not re-derive)

`show()` at 40 MHz ≈ **35 ms** = compute ~21 (field ~18 + flood/project/plot ~3) + swap ~2.4 +
spi ~11.6. At 27 MHz spi was 19.6 (measured split: paint 20977 / swap 2432 / spi 19551 µs).

**MIN_YIELD is the load-bearing tax.** `platform-runtime/src/display.rs:406`:
`thread::sleep(budget.saturating_sub(took).max(MIN_YIELD))`, `MIN_YIELD = 10 ms` (display.rs:93).
Every frame yields ≥1 FreeRTOS tick — **and 1 tick = 10 ms is the floor** because sub-tick sleeps
busy-wait (no scheduler yield) at HZ=100; a shorter yield can't feed IDLE/TWDT. So real cadence ≈
`render_thread_period + 10 ms`. Do NOT lower MIN_YIELD or raise FREERTOS_HZ (shared platform,
watchdog risk, memory flags HZ=100 as load-bearing).

Arithmetic (P = render-thread work per frame; cadence = P + MIN_YIELD, or with double-buffer the spi
overlaps: cadence = max(P, spi) + MIN_YIELD):

| combo | P (thread) | spi | cadence | fps |
|---|---|---|---|---|
| now: SPI40, single-core, single-buf | 35 | (in P) | 35+? ≈ 40 meas | **25** |
| + dual-core, single-buf | ~26 (compute halved) | in P | ~36 | ~27–28 |
| + double-buffer (spi overlaps), single-core | 23.4 (compute+swap) | 11.6 | max(23.4,11.6)+10 = 33.4 | ~30 |
| **+ dual-core + double-buffer** | 14.9 | 11.6 | max(14.9,11.6)+10 = 24.9 | **~40** |

**Conclusion: neither lever alone comfortably clears 30 (both land ~28–30 borderline). BOTH
together → ~40 fps.** This matches the user's stated plan. Build both.

---

## NEXT: two levers, design already scoped

### Lever A — Dual-core compute pipeline (SAFE Rust, no unsafe). Task #10.
Halves the ~18 ms field compute across both cores. Also capital the 4 color sketches reuse.
- **Facts verified:** `esp_idf_hal::task::ThreadSpawnConfiguration { pin_to_core: Option<Core> }`
  with `.set()` (sets affinity for the NEXT spawn) + `cpu::Core::Core0/Core1` exist.
  `SinTable {samples: Vec<f32>}` and `PlumeField {points: Vec<Precomputed>}` are trivially
  `Send+Sync` → share via `Arc`. Precomputed is Copy-of-f32/bool.
- **Design (clean hexagonal, measured cheapest):**
  - `plume-core`: extract the per-point body of `PlumeField::frame()` into a private
    `fn point_from(&Precomputed, t, table) -> FieldPoint`; add
    `pub fn compute_range(&self, start: usize, t, table, out: &mut [FieldPoint])` that fills `out`
    with points `[start, start+out.len())`. Keep `frame()` (reference for tests). Add a test that
    `compute_range` == `frame()` (same fn → trivially).
  - `art-display`: add `plume::plot(frame, points: &[FieldPoint], canvas)` (pure, host-testable —
    the projection+plot loop currently inside `plume::render`). Keep `plume::render` = compute
    serial (via `field.frame`) + plot, for host/default path.
  - **Port**: inject the parallel compute behind a trait so the domain/host stay thread-free.
    Suggested: `trait FrondCompute: Send { fn compute(&self, t: f32, out: &mut [FieldPoint]); }`
    (captures Arc<PlumeField>+Arc<SinTable>). Default serial impl. Firmware provides a dual-core
    impl. Gallery holds one (default serial; `Gallery::with_frond` injects the fast one) + a scratch
    `Vec<FieldPoint>`; plume path = `frond.compute(t, &mut scratch); plume::plot(...)`.
  - **Worker (firmware):** persistent thread pinned to `Core::Core1`, rendezvous per frame via a
    pair of `mpsc` channels **ping-ponging an owned `Vec<FieldPoint>`** (allocation-free, SAFE — no
    aliasing). Main thread computes the near half into its buffer, sends `(t, far_buf)` to worker,
    worker fills far half + sends buffer back (the barrier), main plots both halves.
    Per-frame `std::thread::spawn`/`scope` is too costly (FreeRTOS task create/delete each frame) —
    use a persistent worker. Do NOT plot from 2 threads (shared `&mut Frame` aliasing); compute
    parallel, plot serial (plot is cheap).
  - Memory: `Vec<FieldPoint>` × ~5000 × 12 B ≈ 60 KB. Fits alongside Frame (65 KB) + DMA buf(s).

### Lever B — Non-blocking DMA double-buffer (UNSAFE, quarantined). Task #16.
Hides the ~11.6 ms transfer under the next frame's compute + the 10 ms yield.
- **Facts verified:** `SpiDeviceDriver` (our `BusDevice`, built by `new_single`) EXPOSES
  `pub fn device(&self) -> spi_device_handle_t` (esp-idf-hal 0.46.2 spi.rs:1029). So the raw handle
  is reachable from the `BusDevice` FastPanel already reclaims → non-blocking transfers are feasible.
- **Design:** quarantine crate owns `spi_device_queue_trans` + `spi_device_get_trans_result`
  (raw esp-idf-sys, unsafe). Fold into `dma-mem` (rename to a cohesive "raw DMA" crate: memory +
  transactions) OR a new sibling crate — keep unsafe in ONE place. Two DMA buffers (each via
  `dma_mem::dma_buffer(FRAME_BYTES)`, 65 KB); `FastPanel::blit` ping-pongs: swap Frame→back buffer,
  `queue_trans(back)` (non-blocking), reap the prior transfer with `get_trans_result`. `queue_size`
  ≥ 2 on the device config. RAMWR (0x2C) still issued per frame; window armed once at bring-up.
  Frame N's transfer overlaps frame N+1's compute+yield. Watch: the current blit uses blocking
  `spi.write()` (spi_device_polling_transmit); queue_trans needs `SpiConfig.polling(false)` /
  `queue_size` — verify the esp-idf-hal device was built to allow interrupt (non-polling) transfers,
  or drive queue_trans directly on the raw handle in the quarantine crate.

**Order suggestion:** dual-core first (safe, bigger architectural piece, reusable; if it happens to
overperform and hit ~30 alone, the unsafe double-buffer could be deferred — the cleanest outcome per
"no unsafe unless necessary"). Then double-buffer for the comfortable 40. Measure each on glass —
the estimator has been wrong before; instrument the split (paint/swap/spi) when in doubt.

---

## COMMANDS
- Build (Xtensa release): `just build-generative-art`
- Build + flash + monitor: `just run-generative-art` (runs `espflash flash --monitor
  --non-interactive --baud 115200`, streams serial; run in background, grep the output file for
  `platform-display:.*fps` and crash signatures `assert failed|panic|Guru|spi_bus_add_device`, then
  TaskStop it — it never exits on its own).
- Host tests: `cargo test -p plume-core -p art-display` (add `--lib` to skip goldens while red).
- Regenerate stills (no bless): `cargo run -p art-display --example gallery-screenshots` →
  `target/screens/`. Bless goldens: `just screens-bless`.
- Full gate: `just ci`. Formatting LAST, once: `just fmt` (pre-commit hook rejects unformatted).
- Restore a different app to the board afterwards: `just run` (plant-monitor).

## CONVENTIONS / GUARDRAILS
- Scoped commits (`scope: description`, NOT Conventional Commits). Commit ONLY when the user asks.
- Hexagonal/ECB, deps inward, single responsibility per file, explicit type annotations on ALL
  vars + lambda params. NO unsafe outside the one quarantine crate. Gherkin/property/unit tests,
  test cyclomatic complexity 1, zero/one/many.
- Verify on the glass, not just host-green — a green host gate ≠ device works. Distrust convenient
  signals; a false "verified" is the worst outcome.
- `just fmt` exactly once at the very end, directly (not a sub-agent), never format-then-edit.

## OPEN ITEMS
- [x] Build Lever A (dual-core) — DONE, host-proven bit-identical, feature-gated `dual-core`. Blocked
      on device by the memory wall (see SESSION 2). Enable + measure once the buffers are unified.
- [x] **Keystone: unify the two full-screen buffers** (Frame + DMA → one wire-order DMA buffer).
      DONE (SESSION 4). Freed ~40 K + removed the swap; unblocked both levers.
- [x] Flip on `dual-core`, flash, measure — DONE (SESSION 5): **33.3 fps / 27 ms, 30+ goal cleared.**
      Now default.
- [x] **12-bit RGB444** (Task #19, SESSION 6, 43069f2). Frees SPI + heap; invisible on the white
      plume. fps unchanged (tick bucket) but the enabler for the double buffer's second frame.
- [x] **Field shrink** (Task #21, SESSION 6, b26fdca). `wide` bool → bitset, −19 KiB. Unblocked the
      second buffer's heap.
- [x] **Non-blocking double buffer** (Task #16, SESSION 6, 2ef93dd). New `spi-dma` quarantine crate.
      **paint 14 ms → 50.0 fps.** The ceiling for this picture.
- [ ] User: bless goldens (the 3 continuous-phase-shift plume goldens 02/03/04 from Session 1) +
      confirm 50 fps glass is clean/smooth/no-tearing. (The double buffer's contents are host-proven
      byte-identical; tearing/sync is the one glass-only question.)
- [ ] Branch is `generative-art`. Remaining: docs + on-device fps note + merge to main (Task #11);
      the four placeholder sketches (Tasks #6–9); Task #20 compute micro-opts (only if chasing >50).
- [x] All SESSION 3–6 work COMMITTED (dd4d56d..2ef93dd). Tree clean but for `.claude/`.
