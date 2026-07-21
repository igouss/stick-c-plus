# Handoff — see where the time goes, and who took it

**Epic:** `stick-c-plus-9q1`
**Beads:** `stick-c-plus-grp` (FreeRTOS run-time stats, P1) · `stick-c-plus-dx9` (rust `tracing`, P2)
**Prior art:** `kb/experiments/2026-07-21-paint-cost-by-rotation/` — read it before writing code;
it is the measurement this epic starts from and the method it should imitate.

> [!IMPORTANT]
> **Superseded in its central claim, 2026-07-21.** This document's framing — that the paint is
> *blocked* for ~39 ms and the job is to name the thread that took the core — is wrong, and the
> live question it set is answered.
>
> Nothing took the core. `Panel::set_rotation` early-returns on an unchanged rotation, but on a
> real change it writes MADCTL **and clears the whole screen**, inside the same `Screen::show`
> the render loop times. A paint carrying a turn costs **59.56 ms** against **21.50 ms** settled
> — 19 of 19 real turns over budget, 0 of 180 settled paints over — which is the missing 38 ms.
> Pressing start did not cause it: an idle screen is held to a 1 s budget and a running one to
> 50 ms, so the press *lowered the budget* that reveals a cost that was always there. Confirmed
> on unmodified production firmware.
>
> Read [`kb/experiments/2026-07-21-what-blocks-the-pomodoro-paint/`](../../kb/experiments/2026-07-21-what-blocks-the-pomodoro-paint/README.md)
> instead of Part 1's "How you will know it worked", and `br show stick-c-plus-grp` before
> touching that bead — it cannot be built without `unsafe` and its sdkconfig flags cannot be
> scoped, both verified.
>
> **What still stands and is worth reading:** the two rules an instrument must obey (Part 2),
> the flash-cost baseline table, the house rules, the working relationship, and `stick-c-plus-dx9`
> (`tracing`), which is unaffected and is now the epic's remaining work.

Last updated 2026-07-21, at the end of the session that closed the screen-rotation epic;
banner added by the session that answered the live question.

---

# Part 1 — The idea

This part is the user's. It is the thing being built; everything in Part 2 is implementation
serving it. Where they were quoted, the quotes are verbatim.

## What they want

The idea arrived as a question about a specific line of serial output:

> "can you figure out how to profile issues like `W (21521) platform_runtime::display:
> platform-display: paint took 60.334ms, over the 50ms tick budget` on the controller"

Then, having seen the options:

> "maybe we can add rust tracing support?"

And, on being told the two instruments answer different questions and being offered both:

> "Yes do both, love your initiative and test regime."

## What that means, stated plainly

**The board should be able to answer "why was that slow?" and not only "that was slow."**

Today the render loop times a whole paint and warns past a threshold. That is a smoke alarm:
it tells you there is a fire and nothing about where. The user wants the next question
answerable *on the device*, because every attempt to answer it on the host has been useless —
a host framebuffer's pixel write is a `Vec` index, and says nothing about an SPI transaction.

## Why it is two instruments and not one

This distinction is the heart of the idea and the reason the epic has two beads. Do not
collapse them.

- **`tracing` answers "where in my code did the time go."** Nested spans over the render path;
  a paint decomposes into text fields and sprite fill.
- **FreeRTOS run-time stats answer "who took the core."** Per-task CPU time.

**No amount of instrumenting your own code can reveal time you were not scheduled for.** That
is why `tracing` alone would not close the question that prompted all of this, and why the
run-time stats bead is P1 while the `tracing` bead — the one the user actually named — is P2.
That ordering was explained to them and is what they said yes to.

## How you will know it worked

The epic has a live question attached, and closing it is the acceptance test:

> *The pomodoro timer's display thread is blocked for ~39 ms in about 0.8% of frames. Name
> what blocks it.*

An honest "measured, and here is what it was not" is a real answer. A guess is not.

---

# Part 2 — Your instructions

## Start here

```sh
br show stick-c-plus-9q1                                  # the epic
br show stick-c-plus-grp                                  # take this one first
cat kb/experiments/2026-07-21-paint-cost-by-rotation/README.md
just run-bin-pomodoro paint-profile                       # the instrument that already exists
```

Take **`stick-c-plus-grp`** first. It is P1 because it is the one that can answer the live
question; `stick-c-plus-dx9` is the durable instrument and is independent of it.

## What is already true

Verified on hardware, not assumed. Do not re-derive any of it.

- **The pomodoro timer's screen paints in 21.3 ms**, and **rotation costs nothing**: all four
  rotations agree to within 0.07 ms across 240 samples. The budget is 50 ms, so a normal paint
  uses 43% of it. Measured by `paint-profile`; the numbers are in the experiment README.
- **So the 60 ms production paints are not slow paints — they are blocked paints**, by roughly
  39 ms, in about 0.8% of frames. Everything in this epic follows from that reframing.
- **The blocker is deterministic, not jitter.** Ten production samples spanned 60.31–60.94 ms.
  A 0.63 ms spread on a 60 ms measurement is about one percent. Work that varies costs varying
  amounts; work that is deterministic costs the same every time.
- **The overruns are bursty** — eight in eight seconds, then 22 seconds of silence — and
  **uncorrelated with the main loop's 5 s heartbeat** (deltas scattered 230–4990 ms). They
  clustered in the seconds after the start button was pressed.
- **`paint-profile` exists** at `firmware/apps/pomodoro/bin/src/paint_profile.rs`, run by
  `just run-bin-pomodoro paint-profile`. It times 60 paints at each of four rotations for two
  pictures. It has no other threads running, which is exactly why it reads clean — and is also
  the suspect list.
- **Cost on this panel tracks address windows, not pixels.** Confirmed twice now: the
  2026-07-09 sprite study (400 windows = 85 ms, one window = well under budget), and again at
  2026-07-21 (the rotation-check frame paints far fewer pixels than the creature and takes
  three times as long, 64–67 ms).
- **`Panel::set_rotation` early-returns when the rotation is unchanged**, so `Turning` costs
  nothing per frame. This was checked because it was a suspect, and it was innocent.

## The leading hypothesis, and that it is only a hypothesis

The overruns clustered right after the start button was pressed. `spawn_input` sounds a jingle
on a state change, through the buzzer's one owner thread. **The input/buzzer path is the
suspect.** It has not been tested, and it should not be written down anywhere as though it had
been.

The cheapest discriminator: sound a jingle deliberately, on a cadence you control, and watch
whether paints overrun in step with it. That experiment needs no new instrument and could be
done before either bead — consider doing it first, because a confirmed cause would sharpen what
the run-time stats need to show.

## ce2.1 / `stick-c-plus-grp` — per-task CPU time

ESP-IDF has this built in: `CONFIG_FREERTOS_USE_TRACE_FACILITY` and
`CONFIG_FREERTOS_GENERATE_RUN_TIME_STATS` in `sdkconfig.defaults`, then `vTaskGetRunTimeStats`
or `uxTaskGetSystemState`.

Two things to get right:

1. **They are `unsafe extern "C"` entry points, and this project forbids `unsafe`.** The
   crossing belongs in a safe wrapper in an adapter — `firmware/platform/adapters` — not as raw
   calls scattered through a bench binary. That is a hexagonal boundary as much as a safety
   one: the composition root and the bench tools should ask a port for task statistics, not
   know about FreeRTOS.
2. **Turning the config on is not free**, and the epic's inherited requirement is that a binary
   which did not ask for an instrument pays nothing for it. Check whether the sdkconfig flags
   can be scoped, and if they cannot, say so plainly in the bead rather than quietly widening
   the cost.

## ce2.2 / `stick-c-plus-dx9` — `tracing`, feature-gated

The three properties are in the bead. The short version:

- **Feature-gated, non-default** — `tracing` plus a subscriber is tens of KB.
- **A ring-buffer subscriber, not `tracing-subscriber`'s `fmt` layer** — see the observer-effect
  rule below.
- **Spans at the paint level, not the pixel level** — a span is ~100 ns–1 µs, noise against a
  21 ms paint and ruinous inside a fill loop.

**Prove it works by making it rediscover something already known.** `paint-profile` measured
the rotation-check frame at 64–67 ms against the timer screen's 21.3 ms, and the cause is
address-window count. Spans over both should surface that split without anyone reasoning it
out. An instrument that only agrees with you when you already know the answer has not been
tested.

## The two rules an instrument has to obey

Both were paid for. Neither is negotiable.

1. **An instrument must not perturb what it measures.** A `warn!` at 115200 baud is milliseconds
   of blocking UART. The production render loop reports *by logging*, which is fine for a
   threshold alarm and disqualifying for a measurement. Buffer samples into a fixed array and
   dump outside the timed region — `paint-profile` does this and says why in its module docs.
2. **An instrument must not cost anything to the code that did not ask for it.** `size` on the
   release elf is the signal; `nm` is **not sufficient** — an inlined call cost `host-monitor`
   744 bytes while leaving no symbol at all, and a second regression this session cost it 44
   bytes the same way. `host-monitor` is the canary: it opts into nothing, and it must stay
   byte-identical.

```sh
just build
for b in host-monitor pomodoro plant-monitor orientation; do
  printf '%-14s %s\n' "$b" "$(size firmware/target/xtensa-esp32-espidf/release/$b | tail -1)"
done
```

Baseline at `163fa8b` (text / data / bss):

| binary | text | data | bss | |
|---|---|---|---|---|
| host-monitor | 1 000 949 | 203 356 | 23 321 | opts out — must never move |
| pomodoro | 395 736 | 105 556 | 6 641 | turning |
| plant-monitor | 985 561 | 190 712 | 25 361 | turning |
| orientation | 396 276 | 97 476 | 6 641 | turning |

## The method to imitate

`kb/experiments/2026-07-21-paint-cost-by-rotation/` is the shape of a good result here, and it
is worth reading for the method rather than the answer:

- **Measure the distribution, not the threshold.** The production warning fires only on
  failures, so it can never show that 99.2% of paints take a third of the budget. That
  reframing — from "the paint is slow" to "0.8% of paints are blocked" — was the whole
  breakthrough, and it came free from looking at the spread.
- **A suspiciously narrow distribution argues against contention and for a deterministic
  branch.** That is what sent this investigation at rotation. Rotation turned out innocent —
  but the reasoning was sound and the tool was built to test it rather than to confirm it.
- **Design the instrument so it can refute you.** `paint-profile` profiles *two* pictures
  precisely so "rotation is slow", "everything is slow", and "rotation is innocent" produce
  three different-looking results. It returned the third, contradicting the hypothesis that
  prompted it. That is the tool working, not the tool failing.
- **Write up null results.** The headline of that experiment is that nothing was wrong with the
  thing being suspected.

## House rules

Same as the rotation epic; they are in `CLAUDE.md` and in
`docs/plans/screen-rotation-handoff.md`, which is worth reading for the working relationship
even though its own work is finished.

- **`just ci` is the gate** — fmt, hex-lint, clippy as errors, host suites, both firmware builds.
- **When a suite passes first try, mutate the code and confirm the right test dies.** Done twice
  this session; both times it either confirmed the test or revealed the test was measuring
  something else.
- **A green host gate does not mean the device works.** Flash it, watch serial for a reboot loop
  and for the paint-budget warning.
- **10 ms is the hard floor** for any periodic thread period — `CONFIG_FREERTOS_HZ=100`, so a
  shorter sleep busy-waits instead of yielding.
- **Explicit type annotations on every binding and lambda parameter.** Read a neighbouring file
  before writing; the voice is consistent across this repo.
- **Scoped commits**, scope first, never a Conventional-Commits type.
- **`br` prose via `--description-file`**, and **`br show` it afterwards and read the field
  back** — two stray characters got into these very beads and were only caught by doing that.
  Note `br close` has no `--reason-file`, and stores newlines in a close reason literally as
  `\n`; keep close reasons short and put the long rationale in the commit message.

## Working with the user on the board

They are quick to respond, generous, and will hold the board for you. Three things learned the
hard way, the third of them twice:

1. **Do the work first, flash it yourself, then ask.** `just run-pomodoro`, `just run` (plant
   monitor), `just run-orientation`, `just run-bin-pomodoro <bin>`, `just run-bin <bin>` (the
   plant-monitor bench tools). Serial needs `timeout N just run-… > file 2>&1`; piping to `tail`
   swallows it. `just monitor` attaches without reflashing, which is often all you need.
2. **Ask for the shape, not the verdict.** "Does it work?" gets "it works", which reads
   identically whether the thing worked or never moved.
3. **Expect the ambiguous answer anyway and spend one more round.** It happened at ce1.4 and
   again at ce1.5 despite being written down. Saying *why* you are re-asking makes the second
   ask read as care rather than doubt.
