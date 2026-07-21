---
id: mipidsi-derives-the-cgram-window-per-orientation
title: "mipidsi 0.10 derives the CGRAM window and the canvas size for every orientation — do not tabulate per-rotation offsets"
confidence: high
scope: board:m5stickc-plus
derived-from: [screen-rotation-platform-ce1.4]
supersedes: []
reviewed: 2026-07-21
check: grep -q 'MemoryMapping::from(self.options.orientation)' ~/.cargo/registry/src/*/mipidsi-0.10.0/src/lib.rs && grep -q 'display_offset(OFFSET_X, OFFSET_Y)' firmware/platform/adapters/src/panel.rs
---

**Claim:** `mipidsi` 0.10 already handles rotation-dependent CGRAM offsets. A driver that
sets `display_offset` to the panel's **native-portrait** window and then calls
`set_orientation` at runtime gets the correct address window at every rotation, computed
for it. There is no need to measure, tabulate, or apply per-orientation offsets by hand,
and doing so double-counts.

The M5StickC Plus panel is a 135×240 visible window on a 240×320 ST7789 framebuffer,
sitting at column 52 / row 40 in native portrait. Those two numbers are the *only* offsets
this board needs.

## Where it happens

`Display::set_address_window` (mipidsi-0.10.0/src/lib.rs, ~line 301) recomputes the offset
on every windowed write, from three inputs — the configured `display_offset`, the
configured `display_size`, and the model's `FRAMEBUFFER_SIZE` (240×320 for ST7789):

```rust
let mut offset = self.options.display_offset;
let mapping = MemoryMapping::from(self.options.orientation);
if mapping.reverse_columns {
    offset.0 = M::FRAMEBUFFER_SIZE.0 - (self.options.display_size.0 + offset.0);
}
if mapping.reverse_rows {
    offset.1 = M::FRAMEBUFFER_SIZE.1 - (self.options.display_size.1 + offset.1);
}
if mapping.swap_rows_and_columns {
    offset = (offset.1, offset.0);
}
```

For this panel that yields a column offset of 52 unmirrored and `240 - (135 + 52) = 53`
mirrored — asymmetric, because the visible window is not centred, which is exactly the
kind of off-by-one a hand-written table gets wrong.

`Options::display_size()` (src/options.rs:56) swaps the canvas dimensions on the same
signal, so the `DrawTarget`'s `bounding_box()` reports 240×135 in landscape and 135×240 in
portrait without the caller doing anything.

## The consequence for callers

**Keep `display_offset` at the native-portrait pair.** It is the *input* to the derivation,
not the current orientation's answer. Baking a rotated offset into the builder makes
`set_address_window` compound it, and the fault shows as a picture shifted by tens of pixels
at three of the four rotations while looking perfect at the fourth.

## What this does not settle

That mipidsi's `MemoryMapping` matches *this* panel's wiring. It is a model of how the
controller scans memory, and this board has already produced one case where such a model
was right on paper and wrong on the glass — see
[st7789-wants-rgb-colour-order](st7789-wants-rgb-colour-order.md), where the factory driver
and mipidsi set the same MADCTL bit and red still rendered blue. Rotation bits live in that
same register.

So it was measured: `just run-bin display-rotation-check` paints a border on the outermost
pixel ring at each of the four rotations, because an offset error is a translation and a
translation is only visible against a boundary. Confirmed on the board 2026-07-21 — the
picture is upright at every stop, and the derivation is correct for this panel.

## What still has to be measured

The **phase** between an app's `ScreenRotation` and mipidsi's `Rotation`. They share a step
but not an origin — `ScreenRotation::Deg0` is the native *landscape*, which is
`Rotation::Deg90` on the controller — and, worse, they may run in opposite directions,
because mipidsi rotates the memory scan while `ScreenRotation` rotates the image, and on a
panel mounted turned those are not the same sense. That cannot be reasoned out; it was
measured on the glass. For this board the mapping is a **+90° shift** (`panel_rotation` in
`firmware/platform/adapters/src/panel.rs`), confirmed 2026-07-21.

## Related

- [mipidsi-rectangle-fill-costs-an-address-window](mipidsi-rectangle-fill-costs-an-address-window.md)
  — why rotation is done in hardware here rather than with a rotating `DrawTarget`: a
  transform maps rows onto columns and destroys the `fill_contiguous` fast path.
- [st7789-wants-rgb-colour-order](st7789-wants-rgb-colour-order.md) — the precedent for
  distrusting a MADCTL bit that looks right on paper.
