---
id: sub-tick-sleeps-busy-wait-on-esp-idf
title: "A sleep shorter than one FreeRTOS tick (10 ms) does not yield on ESP-IDF — it busy-waits and starves the idle task"
confidence: high
scope: platform:esp-idf
derived-from: []
supersedes: []
reviewed: 2026-07-20
check: grep -q 'CONFIG_FREERTOS_HZ=100' firmware/target/xtensa-esp32-espidf/release/build/esp-idf-sys-*/out/sdkconfig && grep -q 'Duration::from_millis(10)' platform/platform-runtime/src/display.rs
---

**Claim:** ESP-IDF here is built with `CONFIG_FREERTOS_HZ = 100`, so one scheduler tick is
**10 ms**. A `std::thread::sleep` shorter than a tick cannot block on the scheduler at all —
it falls through to a busy wait. The thread keeps the core, the FreeRTOS **idle task never
runs**, and the task watchdog fires every 5 s (`CONFIG_ESP_TASK_WDT_TIMEOUT_S = 5`) reporting
`IDLE1 (CPU 1)`. The sleep looks like a pause in the source and behaves like a spin loop on
the metal.

The consequence for this repo: **10 ms is the floor for every periodic thread's period, and
for every "minimum yield" fallback.** A cadence faster than 100 Hz is not available, and
asking for one does not buy speed — it buys a starved scheduler and a *slower* readout.

**Evidence:** the orientation readout's first two flashes (2026-07-20).

- Attempt 1 — render loop at a 20 ms cadence against a paint that the loop's own instrument
  measured at **31 ms**. Every frame overran, so the sleep collapsed to
  `platform_runtime::display::MIN_YIELD`, which was then **1 ms** — sub-tick, therefore a
  spin. `IDLE1` starved from 10 s onward and the watchdog fired continuously; `main`'s 2 s
  heartbeat never printed once in 150 s.
- Attempt 2 — the panel's SPI batch buffer widened 512 → 4096 bytes, which cut the paint to
  roughly 12 ms, and the render cadence relaxed to 40 ms. Paint overruns dropped to 9 in
  110 s. **The watchdog still fired every 5 s**, because the *sampler* thread's period was
  5 ms — also sub-tick, also a spin, and the real remaining culprit.
- Attempt 3 — sampler period raised to 10 ms and `MIN_YIELD` raised to 10 ms. **Zero
  watchdog triggers and zero paint overruns over 70 s**, with the 2 s heartbeat steady
  throughout.

**Holds when:** any periodic `std::thread` on this ESP-IDF build. It is a property of the
tick rate, so it applies to `thread::sleep`, `FreeRtos::delay_ms`, and anything built on
them, in every app in this repo.

**Breaks when:** `CONFIG_FREERTOS_HZ` is raised (1000 Hz would make the floor 1 ms, at the
cost of more scheduler overhead) — nothing here has needed that. It also says nothing about
*sub-millisecond* busy-waiting on purpose, which is legitimate for a hardware timing loop
that genuinely must not yield; the fault is a busy wait that was *meant* to be a yield.

**How to apply:** when adding a periodic thread, treat 10 ms as the minimum period and buy
responsiveness elsewhere — the orientation readout bought it by halving its smoothing weight
rather than by polling faster, which cost nothing in noise because the board's measured rest
jitter is only a few milli-g. And when a cadence must be fast, **check the paint/work cost
first**: a budget the hardware cannot meet is not a fast setting, it is a broken one. The
render loop already logs `paint took Xms, over the Yms tick budget` — that warning is the
signal, and it is worth reading before, not after, the watchdog starts.

**Note:** a host `cargo test` cannot catch any of this. `std::thread::sleep` on Linux yields
at any duration, so the whole sample-smooth-publish cycle passed on the host at a 1 ms period.
This is a device-only failure mode.
