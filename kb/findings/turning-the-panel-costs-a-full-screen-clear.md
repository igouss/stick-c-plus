---
id: turning-the-panel-costs-a-full-screen-clear
title: "Turning the panel costs a full-screen clear inside the timed paint — ~38 ms, once per turn"
confidence: high
scope: board:m5stickc-plus
derived-from: [2026-07-21-what-blocks-the-pomodoro-paint]
supersedes: []
reviewed: 2026-07-21
check: grep -q 'clear(Rgb565::BLACK)' firmware/platform/adapters/src/panel.rs && grep -q 'over the .* tick budget' platform/platform-runtime/src/display.rs
---

**Claim:** A paint that carries a **change** of rotation costs about **38 ms more** than a paint
at a settled rotation, because `Panel::set_rotation` writes MADCTL and then clears the whole
screen — and that clear happens **inside** the `Screen::show` call the render loop is timing.

Measured (`blocker-probe`, 200 paints, the timer's own picture, nothing else running):

| | median | over a 50 ms budget |
|---|---|---|
| paints carrying a turn | **59.56 ms** | 19 of 20 |
| paints at a settled rotation | **21.50 ms** | 0 of 180 |

The one turning paint that did not breach asked for the rotation already showing, where
`set_rotation` early-returns — 19 real turns, 19 breaches.

**Why it hides.** `set_rotation` early-returns on an unchanged rotation, so the cost is invisible
on every frame but the one that turns, and a bench with the board flat on a desk never pays it at
all. The spread is tiny (0.10 ms across 19 samples) because a full-screen clear is a fixed byte
count down a fixed-rate bus, which makes it look like a deterministic *blocker* rather than
deterministic *work*.

**What it is not.** It is not contention and not a scheduling problem. Nothing takes the core:
the time is spent inside the display thread's own `show`. A per-task CPU-time profiler pointed at
this would correctly report that no other task ran, and would not locate it.

**When it becomes visible.** Only while the drawn state is *animated*. The render loop holds a
still state to `RENDER_PERIOD` (1 s) and an animated one to `ANIMATION_PERIOD` (50 ms), so the
same 60 ms turning paint is silent on an idle screen and a budget breach on a running one. On the
pomodoro timer this reads as "overruns started when I pressed start" — the press does not cause
the cost, it lowers the budget that reveals it.

**Consequence for a design.** The budget alarm treats a once-per-turn cost as a per-frame
failure. An app whose picture turns should expect one breach per quarter-turn and decide
deliberately whether that is worth reporting; see also
[[mipidsi-rectangle-fill-costs-an-address-window]] for the other way this panel surprises a
frame-time budget.
