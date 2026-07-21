---
id: 2026-07-21-what-blocks-the-pomodoro-paint
title: "What blocks the pomodoro timer's paint for 39 ms — and the answer that nothing blocks it at all"
date: 2026-07-21
domain: [esp32, display, st7789, mipidsi, spi, performance, rotation, freertos, scheduling]
status: confirmed
hardware: "M5StickC Plus @ /dev/ttyUSB0, FT232, ESP32 rev v1.1, MAC DE:AD:BE:EF:00:01; onboard 1.14in ST7789V2 TFT, SPI2 @ 27 MHz"
artifacts: firmware/apps/pomodoro/bin/src/blocker_probe.rs, platform/platform-bench/
findings: [turning-the-panel-costs-a-full-screen-clear]
supersedes-reading-of: 2026-07-21-paint-cost-by-rotation
source: [m5stack-m5stickc-plus, embedded-driver-crates]
---

## Question

The previous experiment established that the timer's screen paints in **21.3 ms** with none of
the app's other threads running, while production reports **60.4 ms** in about 0.8% of frames.
It drew the obvious conclusion:

> So the 60 ms paints in production are not the paint being slow; they are the paint being
> *blocked*, by roughly 39 ms, in 0.8% of frames.

and handed on a suspect list of threads — input, rotation, power-watch, buzzer owner, heartbeat
— with the input/buzzer path leading, because the overruns clustered in the seconds after the
start button was pressed.

**Which thread takes the core for 39 ms?**

The answer is that no thread does. The premise was wrong.

## Method

`blocker-probe` (`just run-bin-pomodoro blocker-probe`) times 200 paints per stage and runs a
**subtractive sweep**: it starts as a replica of production and stops one thread per stage, so a
distribution that recovers between two stages names the thread removed between them. Subtractive
rather than additive because the threads own their peripherals outright — the IMU moves into the
rotation thread, the PMIC into the power watch — so they can be stopped and cannot be restarted.

A deliberate jingle sounds on a controlled cadence through the real buzzer owner, standing in for
the button press the production overruns clustered behind, and **every sample is marked with
whether a jingle was sounding while it was taken**. So each stage also splits into two halves and
answers three questions at once: breaches only while a jingle rang (the buzzer blocks), breaches
in both halves (something else blocks), breaches in neither (this stage reproduced nothing).

Two stages are calibration rather than measurement, and the tool reports them as pass/fail
against numbers fixed before the run:

- **stage 1 must break the budget**, or the bench has not reproduced the fault and no thread can
  be cleared by it;
- **stage 5 must land near 21.3 ms**, or this bench and `paint-profile` are measuring different
  things.

Nothing is logged inside a timed region. The pure reading of the samples — the distribution, the
breach count, the split — lives in `platform-bench` and is host-tested.

## Result: the first run found nothing, and said so

| stage | threads running | median | over budget |
|---|---|---|---|
| 1 | jingle + input + power-watch + rotation | 21.55 ms | 0 / 200 |
| 2 | minus input | 21.98 ms | 0 / 200 |
| 3 | minus power-watch | 21.64 ms | 0 / 200 |
| 4 | minus rotation (the jingle, alone) | 21.53 ms | 0 / 200 |
| 5 | minus the jingle (the display, alone) | 21.51 ms | 0 / 200 |

```text
FAIL  stage 1 painted clean (200 paints, max 23.02ms) — the fault was NOT reproduced,
      so no thread is cleared by this run
PASS  stage 5 agrees with paint-profile: median 21.51ms against 21.30ms
```

**A thousand paints with every one of the app's threads running, and not one breach.** All four
threads together cost under a millisecond, and a jingle sounding through the real buzzer owner
costs a concurrent paint about 0.1 ms — not 39 ms.

This is the calibration check earning its place. Without it the sweep would have been written up
as *"all four threads innocent, the jingle cleared"*, which reads identically to a result and is
a false green: nothing can be cleared by a run that never showed the problem.

## The stage that was missing

Stages 1-5 replicate what the timer **runs**. What they cannot replicate is what a person
**does** — and the board sat flat on a desk for all of them, where the rotation never changes.

`Panel::set_rotation` early-returns when the rotation is unchanged, which is why the turn read as
innocent last time. On a real change it writes MADCTL **and clears the whole screen**
(`panel.rs`), and that clear lands inside the same `show` the render loop is timing.

Stage 6 paints the display alone, turning the panel every tenth paint:

```text
--- stage 6: the display alone, turned every 10 paints ---
all samples                  n 200  min 21.47ms  median 21.50ms  max 59.66ms  over budget  19
  paints carrying a turn     n  20  min 21.98ms  median 59.56ms  max 59.66ms  over budget  19
  paints at a settled rotation n 180  min 21.47ms  median 21.50ms  max 21.58ms  over budget   0
  evidence about the turn: IMPLICATED — breaches only while the turn was active
```

**Nineteen of twenty turning paints break the budget; none of the hundred and eighty settled ones
do.** The one turn that did not breach is sample 0, which asks for the rotation already showing
and early-returns — so 19 real turns produced 19 breaches, and the tool's own internal
inconsistency is the check that it measured turns rather than something correlated with them.

## Reading

**Nothing took the core. The display thread was never blocked** — it was doing a full-screen
clear it was never credited with, inside a `show` the loop attributes entirely to painting.

The arithmetic closes:

| | bench, stage 6 | production |
|---|---|---|
| turning paint | 59.56 ms | 60.4 ms |
| settled paint | 21.50 ms | 21.3 ms |
| difference | **38.1 ms** | **39.1 ms** |
| spread across samples | 0.10 ms | 0.63 ms |

The tight spread that started this whole investigation is explained rather than explained away: a
full-screen clear is a fixed number of bytes down a fixed-rate bus, so it costs the same every
time. *Work that is deterministic costs the same every time* — the reasoning was right, and it
was pointed at the wrong deterministic thing.

**Why the overruns clustered after the start button was pressed**, which was the last
unexplained fact and the reason the buzzer was suspected: the press does not cause the cost, it
*reveals* it. An `Idle` timer is not animated, so the render loop holds it to `RENDER_PERIOD` =
**1 second**; a `Running` timer animates and is held to `ANIMATION_PERIOD` = **50 ms**. A 60 ms
turning paint is silent against a 1 s budget and a breach against a 50 ms one. Pressing start is
what lowers the budget. Confirmed on production firmware: 160 s of turning the board while
`Idle` produced **zero** warnings; the first warning arrived within a second of the press.

**Why both earlier measurements missed it, by construction rather than by accident:**

- `paint-profile` deliberately excluded the turning paint as an untimed warm-up, on the stated
  grounds that a once-per-turn cost should not be averaged into every frame. That is right for
  the question it was asking and it is exactly what hid this one.
- Stages 1-5 here ran with the board flat on a desk. Every replica of production so far
  reproduced what the firmware runs and not what a hand does.

## Confirmation on production firmware

The unmodified timer, monitored while the board was handled:

```text
60.666  60.471  60.886  60.373  60.403  60.43  60.959  60.42  60.551  60.43  60.336  ...
```

Twenty-five warnings, every one between 60.33 and 60.99 ms, arriving in bursts while the board
was being handled and stopping completely when it was set down. The exact turn-to-warning
correspondence was **not** established by hand (the turns were not counted); it was established
on the bench, where the turn count is commanded rather than estimated, at 19 of 19.

**The decisive half is the silence.** The last warning arrived at `t=265131`; the timer went on
running, animating and heartbeating until `t=590961` with the board flat on the desk. That is
**326 seconds — about 6,500 animated paints at the 50 ms cadence — with every thread alive, the
timer `Running`, and not one breach.**

This is what falsifies the thread hypothesis on production firmware rather than only on the
bench. A blocker that lives in the input, buzzer, rotation, power-watch or heartbeat threads is
present in that window exactly as it was in the bursts: the threads all run, the timer animates,
the budget is 50 ms. At the originally-observed rate of 0.8% of frames it predicts roughly
**52 breaches** across those 6,500 paints. Observed: **zero**. The one variable that changed is
whether a hand was touching the board.

## Consequences

- **`stick-c-plus-grp` (FreeRTOS run-time stats) would not have answered this question.** Its
  premise — "~39 ms of not-being-scheduled" — is refuted. Per-task CPU time answers *who took the
  core*, and the answer here is *nobody*: the time was spent inside the display thread's own
  `show`. The bead and the epic both state the blocked-thread framing as established fact and
  need correcting before either is worked.
- **The 50 ms alarm is arguably miscalibrated rather than the code being wrong.** A turn is a
  rare, user-initiated event, and one dropped animation frame while the board is in a hand is
  invisible. The budget check treats a once-per-turn cost as a per-frame failure. Whether to
  suppress the warning on a paint that carried a turn, make the clear cheaper, or leave it alone
  is a design decision, not a bug fix.
- `blocker-probe` and `platform-bench` stay. The sweep found nothing on its first run and said so
  precisely enough that the missing stage was obvious.

## Method notes worth keeping

**Build the calibration check that can fail, and make it pass/fail rather than eyeballed.** The
whole value of the first run is the line `FAIL stage 1 ... the fault was NOT reproduced`. An
instrument that cannot report "I failed to reproduce it" will instead report agreement with
whoever built it.

**A replica of a system reproduces what the system runs, not what its user does.** Five stages
and a thousand paints of faithful thread-level replication found nothing, and the answer was a
person picking the board up. When a bench that mirrors production finds nothing, the next
question is not "which thread did I miss" but "what is not in the replica at all".

**A null result narrowed the search rather than ending it.** The first run bounded every thread's
contribution to under a millisecond, which is what made "it is not a thread" credible enough to
go looking somewhere else.

**Beware the conclusion drawn one step past the measurement.** `paint-profile` measured 21.3 ms
correctly. "So the paint is blocked" was an inference, not a measurement, and it was wrong — the
paint was doing more work, not waiting. The inference travelled into an epic, two beads and a
handoff as though it were the finding.
