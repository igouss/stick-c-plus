---
id: 2026-07-09-panel-colour-order
title: "Does the M5StickC Plus ST7789V2 want RGB or BGR colour order under mipidsi?"
date: 2026-07-09
domain: [esp32, display, st7789, colour]
status: confirmed
hardware: "M5StickC Plus @ /dev/ttyUSB0, FT232, ESP32 rev v1.1, MAC DE:AD:BE:EF:00:01; onboard 1.14in ST7789V2 TFT"
artifacts: ./run.sh
findings: [st7789-wants-rgb-colour-order]
source: [m5stack-m5stickc-plus, embedded-driver-crates]
---

## Question

The firmware's new `FAULT` line — the first non-greyscale pixel this project has ever
drawn — renders **bluish instead of red**. `adapters::st7789` passes
`ColorOrder::Bgr`. Is that wrong?

The question is sharper than it looks. Every pixel the display had drawn until this
point was white text on a black background, and **white and black are symmetric in
red and blue**: `0xFFFF` and `0x0000` are unchanged by swapping those two channels.
A wrong colour order was therefore *unobservable* for the display's entire life. It
did not regress; it was never once tested.

## Hypothesis

Written before measuring: **`Bgr` is wrong and the panel wants `Rgb`.**

Reasoning: red rendering as blue is the exact signature of an R↔B swap, and the
alternative explanation — a wrong `ColorInversion` — is already excluded, because
inversion would render white text as black, and the text has always been white.

Prediction if right: painting bands labelled RED / GREEN / BLUE shows them as
**blue / green / red**, with green untouched (green is invariant under an R↔B swap).

**The source archaeology said the opposite, and was wrong.** The pinned factory
library resolves `TFT_MAD_COLOR_ORDER` to `TFT_MAD_BGR` — its `TFT_RGB_ORDER` is
undefined and `CGRAM_OFFSET` *is* defined, which selects the BGR branch in
`src/utility/ST7789_Defines.h`. And `mipidsi` 0.10's `ColorOrder::Bgr` sets exactly
the same MADCTL bit (`set_address_mode.rs`: `Bgr => result.0 |= 0b0000_1000`). On
paper our init already matched the factory's.

## Method (reproducible)

```sh
kb/experiments/2026-07-09-panel-colour-order/run.sh   # == just run-bin display-colour-check
just run                                              # restore the monitor afterwards
```

`St7789Display::colour_check` paints three full-width bands — red, green, blue, top
to bottom — each labelled in white with the colour it is *meant* to be. It runs
through the **production** `St7789Display::new`, not a replica, so it measures the
init the monitor actually ships. Then a human looks at the glass.

Discriminating power: green is invariant under an R↔B swap but becomes magenta under
a wrong inversion, and the white labels stay white only if the inversion is right.
One glance separates the two failure modes.

## Raw results

With `ColorOrder::Bgr` (the shipped value):

| band label | rendered |
|---|---|
| RED | **blue** |
| GREEN | green |
| BLUE | **red** |

White labels remained white throughout.

With `ColorOrder::Rgb`:

| band label | rendered |
|---|---|
| RED | red |
| GREEN | green |
| BLUE | blue |

Both observations were read off the panel by a human, twice, on the same board.

## Verdict

**The panel wants `ColorOrder::Rgb` under `mipidsi`. Confirmed**, and the fix is
landed in `adapters::st7789::St7789Display::new`.

Green unchanged in both runs, white labels white in both runs: the inversion was never
the problem, exactly as predicted, and the failure is a pure red/blue swap.

The wider lesson is the one worth keeping: **a MADCTL bit is not portable across
driver stacks.** TFT_eSPI and `mipidsi` do not hand the controller identical bytes, so
"the factory sets `TFT_MAD_BGR`" is not evidence about what `mipidsi` should set. The
adapter's doc comment previously asserted BGR was *"verified against the factory
firmware"* — it was verified against the factory's *source*, which is a different and
much weaker claim, and it was never once checked against a coloured pixel. Promoted to
[st7789-wants-rgb-colour-order](../../findings/st7789-wants-rgb-colour-order.md).

## Threats to validity

- **Human colour judgement.** The bands are fully saturated primaries and were read
  under normal room light; a subtle tint would not survive this method, but a channel
  swap cannot hide in it. Anyone re-running this should confirm the white labels look
  white before trusting the band colours.
- **One board, one panel revision.** The claim is scoped to the M5StickC Plus's
  ST7789V2 driven through `mipidsi` 0.10 + `SpiInterface`. A different mipidsi version
  could in principle change how pixels are packed, which is precisely the coupling that
  caused this bug — so re-run the check after any `mipidsi` bump.
- **The mechanism is not established.** We know *what* the panel needs, not *why* the
  two stacks disagree despite writing the same MADCTL bit. No claim is made about the
  cause; the finding rests on observation alone, which is enough to configure the
  adapter correctly and not enough to predict the next driver's behaviour.
