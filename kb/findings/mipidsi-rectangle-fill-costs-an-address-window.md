---
id: mipidsi-rectangle-fill-costs-an-address-window
title: "Under mipidsi, each Rectangle fill costs an address-window setup — 400 of them per frame is 85 ms, not 6 ms"
confidence: high
scope: board:m5stickc-plus
derived-from: [2026-07-09-sprite-fill-throughput]
supersedes: []
reviewed: 2026-07-09
check: grep -q '\.fill_contiguous(&area' plant-display/src/sprite/render.rs && grep -q 'over the .* tick budget' plant-shell/src/display.rs
---

**Claim:** Painting a region of the M5StickC Plus ST7789V2 as **many small
`Rectangle` fills** is bounded by the *number of fills*, not by the pixel count.
`mipidsi` brackets every `fill_solid` with a `CASET` / `RASET` / `RAMWR` sequence — DC
toggles and short SPI transactions whose driver overhead dwarfs their payload. A 100×100
box drawn as 400 cell-sized rectangles takes **≈85 ms**. The same 10 000 pixels streamed
through a single `DrawTarget::fill_contiguous` takes **well under 50 ms**, comfortably
inside the animation tick.

Payload arithmetic predicts the wrong answer and predicts it confidently:
10 000 px × 16 bit ÷ 27 MHz ≈ **5.9 ms**. That number is correct about the bytes and
silent about the windows, which is where ~93% of the time went.

**Evidence:** [2026-07-09-sprite-fill-throughput](../experiments/2026-07-09-sprite-fill-throughput/README.md).
Two flashes of the same firmware differing only in the fill strategy: 155 over-budget
paints in 40 s (~85 ms each) versus 0 in 50 s across *more* animated time. Measured by
the render loop against its own tick budget, on the board.

**Holds when:** driving this panel with `mipidsi` (0.10) over `SpiInterface` at 27 MHz.
The mechanism is the per-window command overhead, so it should generalize to any
`mipidsi` model and any SPI rate — the faster the bus, the *worse* the ratio, since the
fixed per-window cost stays put while the payload shrinks.

**Does not hold for:** a fill that must skip pixels. `fill_contiguous` writes every
pixel of its area, so a **transparent** sprite composited over an existing background
cannot use it. `plant_display::sprite::draw` still fills per cell for exactly that
reason; it runs once over a static background, never on an animation tick.

**Why this hid:** it is invisible to every test this project has. The host renders into
a `Vec`-backed framebuffer where a pixel write is an index, so the unit tests, the
property tests, and the PNG screenshots were all green and all silent. And it is
invisible on the glass: a creature that drops every second frame still looks like a
creature. The failure had **no observer** until the render loop was made to time itself
and warn when a paint outran its tick. That warning is the instrument; keep it.

**Guard:** `plant-shell::display` warns on any paint that exceeds its tick budget, and
floors its sleep at `MIN_YIELD` so an overrunning paint still yields the core. A
`plant-display` unit test renders the per-cell and contiguous paths and compares pixels,
so the two implementations of one picture cannot drift; a transposed contiguous stream
fails it.
