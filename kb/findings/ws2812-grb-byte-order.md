---
id: ws2812-grb-byte-order
title: "WS2812 latches G-R-B on the wire; our RMT adapter does the swap — pass RGB, don't double-swap"
confidence: high
scope: project:stick-c-plus
derived-from: []
supersedes: []
reviewed: 2026-07-03
check: grep -Eq 'pub r: u8' domain/src/color.rs && grep -Eq 'px.g, px.r, px.b' firmware/src/adapters/ws2812.rs
---

**Claim:** A WS2812 latches its 24 bits as **green, then red, then blue** — not
RGB. Exactly one place in the stack performs that reorder: the boundary adapter.
The domain's `Rgb { r, g, b }` stays honest RGB and the firmware must **not**
pre-swap — a second swap produces red↔green inverted output, the classic "why is
my red green" bug.

**Evidence:** `domain/src/color.rs` defines `Rgb { r, g, b }` (plain RGB, no
pre-swap). `firmware/src/adapters/ws2812.rs` (`Ws2812Rmt::write`) encodes each
pixel to RMT pulses in the byte order `[px.g, px.r, px.b]` — the swap lives there,
once, at the boundary. The domain never encodes wire bytes.

> **Changed 2026-07-03:** we now own the RMT encoder in-tree and emit G,R,B
> ourselves. Previously `esp-hal-smartled` did the swap (you handed it `RGB8` and
> it reordered). We dropped smartled to reach `esp-hal 1.1` — see
> [esp-rs-ota-version-matrix](esp-rs-ota-version-matrix.md). The invariant is
> unchanged; only *who* swaps moved from the crate into our adapter.

**Holds when:** the LED output goes through `Ws2812Rmt` (our current and intended
path) — it emits G,R,B, so the domain stays RGB.

**Breaks when:** a refactor re-introduces a swap in the domain or a wrapping layer
— then the two swaps cancel wrong and colours permute. Also false for non-WS2812
strips (APA102/SK9822 are RGB/BGR with a clock).

**How to apply:** Keep colour order out of the domain — `Rgb` is RGB, full stop.
The single wire-order swap belongs in `Ws2812Rmt` and nowhere else. If output
colours look permuted, suspect a double-swap at the boundary, not the domain.
