---
id: st7789-wants-rgb-colour-order
title: "The M5StickC Plus TFT needs ColorOrder::Rgb under mipidsi — the factory's TFT_MAD_BGR does not transfer"
confidence: high
scope: board:m5stickc-plus
derived-from: [2026-07-09-panel-colour-order]
supersedes: []
reviewed: 2026-07-09
check: grep -q 'ColorOrder::Rgb' firmware/adapters/src/st7789.rs && grep -q 'fn colour_check' firmware/adapters/src/st7789.rs
---

**Claim:** Driving the M5StickC Plus's ST7789V2 through `mipidsi` requires
**`ColorOrder::Rgb`**. With `ColorOrder::Bgr`, red and blue are swapped —
`Rgb565::RED` renders blue — while green and all greyscale are unaffected. The
inversion (`ColorInversion::Inverted`) is correct and is *not* involved.

**Evidence:** [2026-07-09-panel-colour-order](../experiments/2026-07-09-panel-colour-order/README.md).
Three labelled bands painted through the production init: under `Bgr` they render
blue/green/red against labels RED/GREEN/BLUE; under `Rgb` they match. Green is
invariant under a red/blue swap and the white labels stayed white in both runs, which
excludes a wrong inversion. Read off the glass, twice.

**Holds when:** driving this panel with `mipidsi` (0.10) over `SpiInterface`.

**Breaks when:** you bump `mipidsi`, or swap the driver stack. The colour order is a
property of *the pipeline*, not of the panel alone — which is the entire point of this
finding. Re-run `just run-bin display-colour-check` after any such change; it is one
flash and one glance.

**How to apply:** pass `ColorOrder::Rgb` in `St7789Display::new`. Do **not** "correct"
it back by reading the factory library: the pinned `m5stack-m5stickc-plus` source
resolves `TFT_MAD_COLOR_ORDER` to `TFT_MAD_BGR` (`TFT_RGB_ORDER` undefined,
`CGRAM_OFFSET` defined → the BGR branch), and `mipidsi`'s `ColorOrder::Bgr` sets that
same MADCTL bit 3. **On paper the two inits agree; on the panel they do not.** TFT_eSPI
and `mipidsi` do not hand the controller identical bytes, so a MADCTL value lifted from
one stack is not evidence about the other.

Two things worth internalising beyond the pixel:

- **White-on-black cannot test colour.** `0xFFFF` and `0x0000` are symmetric in red and
  blue, so a channel swap is *invisible* to greyscale output. This bug shipped with the
  display and survived every look at the screen until the first coloured pixel — a red
  `FAULT` line — was drawn months later. It never regressed; it was never tested.
  `St7789Display::colour_check` now makes it falsifiable in one flash.
- **"Verified against the factory firmware" was a false green.** The adapter's doc
  comment claimed exactly that. What had actually been verified was the factory's
  *source*, by reading it — never its *output*, by looking. Reading a header is not
  measurement, and a claim that cannot fail is not verified. Any port of a hardware
  magic number across driver stacks should be treated as a hypothesis until a pixel,
  a voltage, or a scope trace says otherwise.
