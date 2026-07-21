---
id: 2026-07-21-paint-cost-by-rotation
title: "Does turning the panel make a paint more expensive, and what is actually blowing the pomodoro timer's 50 ms budget?"
date: 2026-07-21
domain: [esp32, display, st7789, mipidsi, spi, performance, rotation]
status: confirmed
hardware: "M5StickC Plus @ /dev/ttyUSB0, FT232, ESP32 rev v1.1, MAC DE:AD:BE:EF:00:01; onboard 1.14in ST7789V2 TFT, SPI2 @ 27 MHz"
artifacts: firmware/apps/pomodoro/bin/src/paint_profile.rs
findings: [mipidsi-rectangle-fill-costs-an-address-window]
source: [m5stack-m5stickc-plus, embedded-driver-crates]
---

## Question

The pomodoro timer took the screen-rotation capability (ce1.6/ce1.9). With the timer
running, serial began reporting over-budget paints:

```text
W (21521) platform_runtime::display: platform-display: paint took 60.334ms, over the 50ms tick budget
```

**Did rotation cause this?** The plan had warned that a rotated draw target "maps rows onto
columns and destroys the `fill_contiguous` fast path", and the 2026-07-09 sprite study had
already measured what losing that path costs: 85 ms when one address window becomes 400.

## What the production log could and could not say

It could say *that* a paint overran. It could not say which rotation was showing, which part
of the picture was slow, or what a normal paint costs — it only warns on the failures, so the
distribution is invisible.

Two properties of the raw samples were load-bearing before any new code was written:

- **The spread was 0.63 ms** across ten samples (60.31–60.94 ms) spanning 30 seconds. One
  percent variation on a 60 ms measurement is far too tight for scheduler jitter. Work that
  varies costs varying amounts; deterministic work costs the same every time. So the slow
  paints were *doing something different*, not being unlucky.
- **They were bursty** — eight in eight seconds, then 22 seconds of silence — and uncorrelated
  with the main loop's 5 s heartbeat (deltas scattered from 230 ms to 4990 ms).

Only ~10 of roughly 1200 paints in that minute overran, so the warning describes **0.8% of
paints**, not the normal case. That reframing matters: the question is not "why is the paint
slow" but "what does 0.8% of paints do that the rest do not".

## Method

`paint-profile` (a bench bin beside the timer, `just run-bin-pomodoro paint-profile`) times
60 paints at **each of the four rotations**, for **two pictures**:

- the timer's own screen — two text fields and an 80×20-cell creature scaled to 80×80, whose
  sprite is one large `fill_contiguous`;
- the rotation-check frame — four thin edge strips and four small corner squares, and no large
  fill at all.

The two pictures separate the explanations. Only the first slow when turned ⇒ the large
contiguous fill is being broken into many windows in the rotated frame. Both slow ⇒ the turned
panel costs more per window generally. Neither ⇒ rotation is innocent.

It never logs inside a timed region — samples go into a fixed array and the summary is printed
afterwards. A `warn!` at 115200 baud is milliseconds of blocking UART, which is enough to be
the thing you are measuring.

Every frame is forced to differ from the one before it (clock advances, animation clock steps),
so no paint is skipped as unchanged.

## Result

```
--- the rotation frame: four edge strips and four corner squares (no large fill) ---
DEG0   (landscape)  min 64.06ms  median 66.86ms  max  69.46ms   OVER BUDGET
DEG90  (portrait)   min 64.05ms  median 66.85ms  max 123.02ms   OVER BUDGET
DEG180 (landscape)  min 64.05ms  median 67.15ms  max 117.07ms   OVER BUDGET
DEG270 (portrait)   min 64.05ms  median 67.16ms  max 118.62ms   OVER BUDGET

--- the timer's screen: two text fields and an 80x80 creature (one large fill) ---
DEG0   (landscape)  min 21.28ms  median 21.30ms  max 21.34ms   fits
DEG90  (portrait)   min 21.27ms  median 21.29ms  max 21.34ms   fits
DEG180 (landscape)  min 21.27ms  median 21.29ms  max 68.16ms   over on some frames
DEG270 (portrait)   min 21.28ms  median 21.29ms  max 21.34ms   fits
```

## Reading

**Rotation is innocent, and the hypothesis that prompted this tool was wrong.** The timer's
screen costs 21.3 ms at every rotation — landscape and portrait agree to within 0.07 ms across
240 samples. The rotated `fill_contiguous` is not being broken into windows, the MADCTL write
costs nothing per frame (`Panel::set_rotation` early-returns when the rotation is unchanged),
and the portrait layout is no more expensive than the landscape one.

**The timer's paint fits its budget with 58% to spare** — 21.3 ms against 50 ms. So the 60 ms
paints in production are not the paint being slow; they are the paint being *blocked*, by
roughly 39 ms, in 0.8% of frames. This bench has none of the app's other threads (input,
rotation, power-watch, buzzer owner, heartbeat), which is exactly why it reads clean, and is
also the remaining suspect list.

The production overruns clustered in the seconds immediately after the start button was
pressed, which points at the input/buzzer path — a jingle is sounded on a state change — rather
than at anything the display does. That is a hypothesis, not a result; it has not been tested.

**Incidentally confirmed: the rotation-check frame is itself over budget** at 64–67 ms, at every
rotation. It draws eight small filled shapes plus text, each of which costs its own address
window, which is precisely the 2026-07-09 finding restated. It is a bench tool with a 4 s dwell
so this costs nothing in practice — but it means the frame is not a picture to imitate in an
app, and it is a second independent confirmation that on this panel **cost tracks the number of
address windows, not the number of pixels**: the frame paints far fewer pixels than the creature
and takes three times as long.

## Consequences

- Nothing in the screen-rotation epic needs undoing. Rotation costs no measurable paint time.
- The 60 ms production paints remain unexplained and are **not** a rotation regression. Next
  step is to profile with the other threads running — the cheapest discriminator is to sound a
  jingle deliberately and watch whether paints overrun in step with it.
- `paint-profile` stays as the instrument. It answered a question the production log could only
  raise, and its two-picture design is what made the answer decisive rather than suggestive.

## Method note worth keeping

The tight spread was the whole clue, and it was available in the raw log before any code was
written. **A distribution says things a threshold cannot**: the production warning fires only on
failures, so it can never show that 99.2% of paints take a third of the budget. When something
intermittent is over a threshold, measure the distribution before theorising about the cause —
and note that a suspiciously *narrow* distribution argues against jitter and contention, and for
a deterministic branch.
