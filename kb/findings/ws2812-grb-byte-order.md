---
id: ws2812-grb-byte-order
title: "WS2812 latches G-R-B on the wire; the boundary adapter does the swap — pass RGB, don't double-swap"
confidence: high
scope: project:stick-c-plus
derived-from: []
supersedes: []
reviewed: 2026-07-04
check: grep -Eq 'pub r: u8' apps/led-driver/led-core/src/color.rs   # domain stays RGB; the single wire swap lands in the WS2812 adapter under qqh.1
---

**Claim:** A WS2812 latches its 24 bits as **green, then red, then blue** — not
RGB. Exactly one place in the stack performs that reorder: the boundary adapter.
The domain's `Rgb { r, g, b }` stays honest RGB and the firmware must **not**
pre-swap — a second swap produces red↔green inverted output, the classic "why is
my red green" bug.

**Evidence:** `domain/src/color.rs` defines `Rgb { r, g, b }` (plain RGB, no
pre-swap) — the domain never encodes wire bytes. The single G-R-B reorder belongs
in the WS2812 output adapter, which emits each pixel as `[g, r, b]` RMT pulses.
The latch order itself is the datasheet fact (see
[m5stickc-plus-datasheets](../sources/m5stickc-plus-datasheets.md) — the ESP32 TRM
+ WS2812 timing).

> **Note (std/ESP-IDF pivot):** the WS2812 adapter re-homes to **`esp-idf-hal`
> RMT** (`TxRmtDriver` + `FixedLengthSignal`, same WS2812 timings) and lands under
> **qqh.1** at `firmware/adapters/src/ws2812.rs` — it is not built yet. Earlier
> notes had `esp-hal-smartled` (then, briefly, an in-tree `esp-hal` RMT encoder)
> do the swap; the encoder implementation moved, the **invariant did not** —
> whoever owns the PHY does the one swap, the domain stays RGB.

**Holds when:** the LED output goes through the WS2812 adapter (project #2's
intended path, qqh.1) — it emits G,R,B, so the domain stays RGB.

**Breaks when:** a refactor re-introduces a swap in the domain or a wrapping layer
— then the two swaps cancel wrong and colours permute. Also false for non-WS2812
strips (APA102/SK9822 are RGB/BGR with a clock).

**How to apply:** Keep colour order out of the domain — `Rgb` is RGB, full stop.
The single wire-order swap belongs in the WS2812 adapter and nowhere else. If
output colours look permuted, suspect a double-swap at the boundary, not the
domain.
