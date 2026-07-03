---
id: ws2812-grb-byte-order
title: "WS2812 latches G-R-B on the wire, but smart-leds does the swap — pass RGB, don't double-swap"
confidence: high
scope: project:stick-c-plus
derived-from: []
supersedes: []
reviewed: 2026-07-03
check: grep -Eq 'pub r: u8' domain/src/color.rs && grep -Eq 'pub g: u8' domain/src/color.rs
---

**Claim:** A WS2812 latches its 24 bits as **green, then red, then blue** — not
RGB. But in our stack the `smart-leds` + `esp-hal-smartled` RMT path performs that
reorder for you: you hand it `smart_leds::RGB8` in **R,G,B** order and it emits
G,R,B on the wire. So the domain's `Rgb { r, g, b }` stays honest RGB, and the
firmware must **not** pre-swap. Swapping in the domain would produce red↔green
inverted output — the classic "why is my red green" bug — via a *double* swap.

**Evidence:** `domain/src/color.rs` defines `Rgb { r, g, b }` (plain RGB, no
pre-swap). `firmware/src/main.rs` drives the strip through
`esp_hal_smartled::SmartLedsAdapter` + `smart_led_buffer!`, whose WS2812 encoder
owns the GRB ordering. The domain never encodes wire bytes, so the concern lives
entirely at the boundary adapter.

**Holds when:** the LED output goes through `smart-leds` / `esp-hal-smartled`
(our current and intended path).

**Breaks when:** someone hand-rolls the RMT bit stream (bypassing smart-leds) — then
*they* must emit G,R,B themselves, and the swap becomes their responsibility. Also
false for non-WS2812 strips (APA102/SK9822 are RGB/BGR with a clock).

**How to apply:** Keep colour order out of the domain — `Rgb` is RGB, full stop.
Do any wire-order concern in the `LedOutput` adapter only, and prefer letting
`smart-leds` handle it. If output colours look permuted, suspect a double-swap at
the boundary, not the domain.
