---
id: 2026-07-09-sprite-fill-throughput
title: "How long does one 100x100 sprite frame take to paint on the ST7789, and does it fit a 50 ms animation tick?"
date: 2026-07-09
domain: [esp32, display, st7789, mipidsi, spi, performance]
status: confirmed
hardware: "M5StickC Plus @ /dev/ttyUSB0, FT232, ESP32 rev v1.1, MAC DE:AD:BE:EF:00:01; onboard 1.14in ST7789V2 TFT, SPI2 @ 27 MHz"
artifacts: ./run.clj
findings: [mipidsi-rectangle-fill-costs-an-address-window]
source: [m5stack-m5stickc-plus, embedded-driver-crates]
---

## Question

The plant monitor's glass gained an animated 20×20 creature, drawn at scale 5 into a
100×100 box, repainted on a 50 ms tick whenever the probe is unhealthy. **Does a frame
fit inside its tick?**

The question could not be answered on the host. Every unit test rendered into a
`Framebuffer` where a pixel write is a `Vec` index; the whole crate's test suite says
nothing about how long a write takes when it is an SPI transaction on a 27 MHz bus. A
frame that overruns its tick does not look broken — a slow creature still looks like a
creature — so the failure mode is a **silent one**, and the only instrument that can
see it is the board.

## Method

The `plant-shell` render loop times every paint that actually touches the panel and
warns when the paint exceeds the tick budget it was given:

```
if painted && took > budget {
    warn!("plant-display: paint took {took:?}, over the {budget:?} tick budget");
}
```

Flash, leave the probe disconnected so the observation stays `Faulted` (which is an
*animated* scene, budget 50 ms), stream serial for ~45 s, and count the warnings. A
healthy probe would be useless here: `Fresh` is a motionless creature with a 1000 ms
budget, and it never repaints at all.

`run.clj` does exactly that, and prints the count.

## Runs

### Run 1 — one `Rectangle` fill per cell

`sprite::draw_onto` painted each of the 400 cells as its own
`Rectangle::into_styled(...).draw(target)`.

```
over-budget paints : 155   (in ~40 s)
paint took 83.635ms, over the 50ms tick budget
paint took 84.707ms, over the 50ms tick budget
paint took 87.334ms, over the 50ms tick budget
paint took 87.105ms, over the 50ms tick budget
```

**~85 ms per frame against a 50 ms budget.** The creature was dropping roughly one
frame in two. Nothing else in the log complained: no render errors, no watchdog, no
reboot. The picture was correct and the timing was not.

### Run 2 — one `fill_contiguous` over the whole box

Same pixels, same colours, same origin and scale. The 100×100 area is filled by a
single `DrawTarget::fill_contiguous`, streaming 10 000 `Rgb565` values row-major.

```
over-budget paints : 0    (in ~50 s)
render failures    : 0
panics / watchdog  : 0
boots              : 1
faulted cycles     : 50   (more animation than run 1's 39)
```

Zero. Across *more* animated time than the failing run.

## Reading

The pixel count did not change; the number of **address-window setups** did. `mipidsi`
brackets each `fill_solid` with a `CASET` / `RASET` / `RAMWR` command sequence — DC line
toggles and short SPI transactions, each with driver overhead that dwarfs its payload.
400 of them per frame cost ~85 ms. One of them, followed by 20 KB of pixel data streamed
at 27 MHz, costs well under the budget.

The naive arithmetic hid this: 10 000 pixels × 16 bits ÷ 27 MHz ≈ 5.9 ms, which "fits
easily" in 50 ms. That estimate is right about the payload and silent about the
per-window cost, which is where all the time went.

## Consequences

- `sprite::draw_onto` (the animating path) uses `fill_contiguous`. See
  [mipidsi-rectangle-fill-costs-an-address-window](../../findings/mipidsi-rectangle-fill-costs-an-address-window.md).
- `sprite::draw` (the compositing path) still fills per cell, because it must skip
  transparent cells and a contiguous fill cannot. It is a one-shot composite over a
  static background, so it never runs on an animation tick.
- The two paths are separate algorithms for one picture, so a `plant-display` unit test
  renders both and compares pixels. Transposing the contiguous stream's row/column makes
  it fail; that was checked by mutation, not assumed.
- The over-budget warning stayed in the loop. It is the instrument that found this, and
  the next sprite, scale, or panel change will need it again.
- An overrunning paint used to sleep for zero and never yield the core. It survived on
  luck. `plant-shell::display::MIN_YIELD` now floors the sleep at 1 ms.

## Rig

M5StickC Plus on `/dev/ttyUSB0` — see
[flashing-and-serial-access](../../guides/flashing-and-serial-access.md). The probe must
be **disconnected or dry** so the observation stays `Faulted` and the creature animates;
with a healthy probe this experiment measures nothing, and reports zero warnings for the
wrong reason.
