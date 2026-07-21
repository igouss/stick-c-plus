# Handoff — health supervision and an alarm you can hear

Written 2026-07-21. Epic: `stick-c-plus-health-supervision-bcf` (+ 8 child beads).
Nothing has been implemented yet. This document is the brief; the beads are the spec.

---

## Part 1 — The idea, in the owner's words

> So I'd like to have some kind of, uh, monitor, um, that would make sure that the
> application is running correctly, uh, that there is no, uh, stuck errors or, um, the
> application haven't crushed. And if anything like this happened, I would like the device
> to have constantly beeping sounds, uh, so that it would notify me that the error
> happened, and it would, uh, be a way for me to confirm that without treating the logs.

Restated, with the decisions the owner made when asked:

The board should notice when it has stopped working and **tell me out loud**, so that
finding out does not require a serial cable. Four things count as "stopped working": a
thread that is **stuck**, a component that has **given up**, a **crash**, and **memory
running out**. When any of them happens the buzzer sounds and keeps sounding until I press
a button to acknowledge it — and after I have silenced it, the screen still tells me what
broke.

Scope: the **shared platform**, so every app gets it, not one app.

Two follow-up decisions the owner made during the session:

- **2026-07-21 — `unsafe` exception granted** for IDF heap/stack introspection
  specifically, after being shown the trade against the safe-but-weaker alternative. It is
  an exception for that one case, not a general relaxation. See Part 2, item 4.

---

## Part 2 — Instructions for the next agent

### Before you touch anything

1. `br show stick-c-plus-health-supervision-bcf` and read the epic in full, then
   `br ready` and take the top unblocked bead. The beads carry the real detail — this file
   is orientation, not a substitute for them.
2. Read `/home/elendal/CLAUDE.md`. It is not boilerplate here: hexagonal/ECB with
   dependencies pointing inward, Gherkin tests, cyclomatic complexity 1 per test, no loops
   in tests, zero/one/many, explicit type annotations including on lambda parameters.
3. `kb/INDEX.md` — the board findings referenced below are real and were paid for.

### The dependency graph (verified acyclic)

```
panic-behaviour-z1v ──┬─→ heartbeat-registry-qg0 ─┐
                      └─→ boot-verdict-bz1 ───────┤
health-domain-sgu ────┬───────────────────────────┼─→ supervisor-thread-2q7 ─┐
                      └─→ reported-and-starved-jc0 ──────────────────────────┼─→ alarm-on-device-proof-72x
buzzer-priority-wqr ──────────────────────────────┘                         │
```

Ready now: `panic-behaviour-z1v`, `health-domain-sgu`, `buzzer-priority-wqr`.
Do `panic-behaviour-z1v` early regardless of what else you pick up — it is one bench run
and it unblocks two beads whose design changes depending on the answer.

### Five things that are load-bearing, and why

1. **Sound is the primary channel; the glass is secondary.** The supervisor drives the
   buzzer *directly* and never routes an alarm through the render loop, because the thread
   most likely to be wedged **is** the render loop. Accept that the fault banner does not
   appear when the display thread is the dead one. Do not add a second painting path to
   cover that gap — it is a new thing to go wrong on exactly the axis being defended.

2. **The heartbeat registry must be lock-free.** Atomics, not a `Mutex`. If reading the
   heartbeats can block, a thread that wedges *while holding the lock* takes the supervisor
   with it, and the alarm dies at precisely the moment it exists to fire. The supervisor
   must never be blockable by the thing it supervises. This is also why
   `buzzer-priority-wqr` exists: `BuzzerHandle::play` currently blocks its caller for the
   length of a melody, and a supervisor parked inside a pomodoro jingle is not supervising.

3. **A false alarm is worse than a missed one.** It teaches the operator to ignore the
   buzzer, and after that the real fault is silent no matter what the firmware does. This
   is the reason for the off-by-one deadline test, the "beat at the top of the loop,
   unconditionally" rule, the long quiet run in the proof bead, and the instruction to read
   real numbers off a healthy board before setting any floor. When in doubt, do not beep.

4. **`unsafe`: exactly one file, and it is not a precedent.** The tree has zero unsafe
   across 273 `#![forbid(unsafe_code)]` files. The owner granted an exception for the
   STARVED detector only (`esp_get_free_heap_size`, `esp_get_minimum_free_heap_size`,
   `uxTaskGetStackHighWaterMark` — no safe wrapper exists in esp-idf-hal 0.46 /
   esp-idf-svc 0.52). Scoping is acceptance criteria, not style: one driven adapter under
   `firmware/platform/adapters`, reads only, `#![deny(unsafe_code)]` with a targeted
   `#[allow]` and a real SAFETY argument per call site so every crossing is greppable. No
   gate enforces this — it is a per-file convention — so **the count of files that may
   contain `unsafe` is the number to watch, and it is 1.** Everything else in the epic is
   safe: `ResetReason::get()` and `TWDTDriver::watch_current_task()` are safe wrappers,
   confirmed in esp-idf-hal 0.46.2.

5. **Verification is acoustic, and on hardware.** The G2 transducer radiates almost none of
   its energy at the pitch it is driven at (kb finding). Asserting the right notes were
   sent to the buzzer proves nothing about anyone hearing anything — judge by DC-removed
   RMS on the PDM mic against the silent floor, via `platform-audio`, the way
   `chime_selftest` already does. Warm the mic up first. Separately: a green host gate is
   not a working device (kb finding, learned expensively here) — flash it and watch.

### The trap specific to this epic

A health monitor that falsely reports healthy is worse than no health monitor, because it
converts an unknown into a wrong belief. Its whole job is to report a state that never
occurs during normal testing, so the code path that matters is the one nothing exercises —
**passing tests are the weakest possible evidence here.** Every bead is done on a
demonstrated *true positive* on real hardware: a thread actually killed, an alarm actually
heard, a leak actually leaked. The composition roots are where the false green will come
from: a thread spawned but never registered is invisible, and invisible in the direction
that says everything is fine. Make it hard to spawn a loop without a pulse rather than
relying on remembering.

If an injection does not raise the alarm, that is the instrument working — write it up as a
finding, do not work around it. An honest "could not verify" is fine; a fabricated
"verified" is the worst available outcome.

### What the ground looks like today (verified 2026-07-21, not assumed)

- **No watchdog, no supervisor, no heartbeat, no `esp_task_wdt`, no `catch_unwind`** in
  either workspace. Grep hits are doc prose only.
- Every loop spawns as an `XTask` and is then **bound to a `_`-prefixed local in `main` and
  never polled again**. `JoinHandle::is_finished()` is called once in the whole tree
  (`platform/esphome-server/src/server.rs:247`), nowhere near an app thread.
- `main`'s "supervisory loop" is `loop { FreeRtos::delay_ms(5_000); info!(..) }`. It
  inspects no task handle and logs the same cheerful snapshot whether four threads are
  running or none.
- Consequence, asymmetric by thread: a dead **sampler/poller** is indirectly visible via
  the per-app freshness inference (`Stale`). A dead **display, input, rotation,
  power-watch or buzzer** thread is invisible in every channel the operator has.
- The eight loops needing a pulse: `spawn_display`, `spawn_power_watch`, `spawn_rotation`,
  `spawn_buzzer` (platform-runtime), `pomodoro_shell::spawn_input`,
  `host_shell::spawn_poller`, `plant_shell::spawn_sampler`,
  `orientation_shell::spawn_sampler`.
- The buzzer arbiter (`platform/platform-runtime/src/buzzer.rs`) is one owner thread with a
  blocking one-slot rendezvous, and `OwnerGone` is the tree's only existing "owner thread
  died" error type — the natural seam.
- Pomodoro and orientation have **no status surface at all**. Host-monitor has a 4-char
  `DOWN`/`OLD` token; plant-monitor picks a scene from `Observation`.

### Open question the first bead exists to close

Does a Rust panic on this board unwind the thread, or reset the chip? No `panic = "abort"`
in `firmware/Cargo.toml`, but nobody has watched it happen. If it unwinds,
`is_finished()` plus a panic hook become a fast second detector with a named thread. If it
resets, both are dead ends and the heartbeat deadline is the only stall detector. **Measure
it in a non-main thread. Do not guess, and do not design around a guess.**

### Constraints inherited from elsewhere in the tree

- An app that does not opt into an instrument **must pay nothing** in flash or RAM.
  host-monitor is the canary and must stay byte-identical. `size` is the signal, `nm` is
  not sufficient. (From the profiling epic; this is what killed `stick-c-plus-grp` — its
  sdkconfig flags could not be scoped to one binary. An adapter taken in a composition root
  scopes naturally, but confirm it rather than assume it.)
- Never log inside a timed region — a `warn!` at 115200 baud is milliseconds of blocking
  UART, and an instrument must not perturb what it measures.
- `CONFIG_FREERTOS_HZ` is 100, so **10 ms is the hard floor** for any thread period or
  deadline (kb finding). Host tests cannot see this.
- Every crate needs a `[package.metadata.hex-arch] role`; `hex-lint` enforces the role
  matrix and context isolation on every commit via `just precommit`. The new
  `platform-health` crate is `role = domain`, `context = shared`.
- Beads: `br` never runs git. After changing beads, `just bead-sync`, then commit
  `.beads/issues.jsonl` under a `beads:`-scoped message. Prose goes in via
  `--description-file` — backticks in `-d "..."` get command-substituted and vanish.
- Commits are scoped, not typed: `platform-health: ...`, not `feat: ...`.

### Definition of done for the epic

Kill any one app thread on a running board, touch nothing else: it beeps within seconds,
keeps beeping until a button is pressed, and still names what broke on the glass
afterwards. No serial cable anywhere in that story. Every detector has a measured
time-to-alarm and an acoustic level clearing the silent floor, written up in `kb/`, behind
a `just` recipe so it is re-runnable — a regression in an alarm is silent by definition.
