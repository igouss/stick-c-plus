# Battery-Life Refactor — MQTT Deep-Sleep Duty Cycle

## Context

The M5StickC Plus plant monitor has a **120 mAh** LiPo. Today the firmware runs
**flat-out 24/7** — no sleep of any kind, TFT backlight always lit, WiFi always
associated, four schedulers waking ~4×/s, the AXP192 touched once at boot then
dropped (no rail-off, no battery telemetry). On 120 mAh that is **~1–1.5 h** of
runtime (field reports: "8 % after 20 min at full brightness").

The device is a **plant monitor**: soil moisture changes over *hours*, not
seconds — the textbook case for **deep-sleep duty cycling**. This refactor takes
the firmware from ~1.5 h to a **days**-scale battery target.

**Decisions locked with the user (do not revisit):**

1. **Deep-sleep duty cycle** — wake on RTC timer (~15 min, configurable), sample,
   report, sleep. Also wake on **Button A (GPIO37, ext0)** to briefly light the
   display. Target ~2–3 days on 120 mAh.
2. **Display dark by default** — LDO2/LDO3 rails cut; on button wake, power rails,
   render latest ~5 s, cut rails, sleep. Timer wakes are **headless**.
3. **MQTT push, not native-API** — the device connects *out* to an MQTT broker,
   publishes, sleeps (no listener, shortest wake). `esp-idf-svc`'s `EspMqttClient`
   is a **safe** wrapper, so this is *more* rule-compliant, not less.
4. **Isolate all `unsafe`** — every sleep/PM FFI + RTC-memory access lives in **one**
   audited module; the rest of the tree stays `#![forbid(unsafe_code)]`.
5. **Per-power-model roadmap split** — the **battery** plant monitor uses MQTT; the
   **mains-powered** siblings (WS2812 LED driver `fmz`, rover `z6p`) keep the
   home-grown native-API crate. `esphome-api`/`esphome-server` are **not** deleted —
   the plant monitor just selects a different outbound adapter.

---

## The device power model (the research — capture in `kb/`)

The primary ask was to *understand* the ESP32/AXP192 power features. Capture this
as `kb/guides/m5stickc-plus-power-model.md` + a falsifiable finding for the AXP192
sleep floor (kb two-voice convention).

**ESP32 sleep tiers**

| Mode | SoC current | Wake | Notes |
|---|---|---|---|
| Active (240 MHz + WiFi) | 100–260 mA peaks | — | TX peaks dominate |
| Modem-sleep (`esp_wifi_set_ps`) | 20–68 mA | — | stays associated, RF gated per DTIM |
| Light-sleep (`esp_light_sleep_start`) | ~0.8 mA | timer/GPIO, <1 ms | RAM retained, resumes in place |
| Deep-sleep (`esp_deep_sleep_start`) | **~10 µA (SoC only)** | timer/ext0/ext1, **reboots** | only `.rtc.data` (~8 KB) survives |

**The catch — the AXP192 sets the real floor, not the ESP32.** On this board
deep-sleep floors at **~2 mA** (AXP192 quiescent), and the **MPU6886 IMU adds
~2.5 mA** unless slept. The headline target is only reachable if we also cut
LDO2/LDO3 (reg `0x12`) and **sleep the MPU6886** (its own I²C reg `0x6B`; the IMU is
on DCDC1, shared with the ESP32 core, so its *rail* can't be cut).

**AXP192 rails (I²C `0x34`, bus G21/G22):** LDO2 = TFT backlight, LDO3 = TFT panel
(reg `0x28` voltage/brightness, reg `0x12` enable), **DCDC1 = ESP32 core + 3.3 V
MPU6886 (never cut)**. Battery telemetry via ADC regs (voltage `0x78/0x79`) — a free
"battery %" to publish to HA. The soil probe is on **ADC1/GPIO33, not** the I²C bus,
so battery/rail/IMU I²C traffic and the ADC burst are independent.

**Battery math (120 mAh):** naive always-on lit ≈ **1–1.5 h**; duty cycle @ 15 min,
~5 s burst @ ~100 mA + sleep @ ~2 mA ≈ **~1.9 days**; drop the floor toward ~0.5 mA
(IMU slept, rails cut) ≈ **~4 days**; longer `sleep_duration` scales linearly.

**Why MQTT over native-API for a sleeper:** native-API has HA *connect to* the
device — a sleeper holds no listener, forcing a ~15–30 s reconnect-per-wake window
(most of the energy budget) plus a `has_deep_sleep` flag + graceful-disconnect
handshake just to stop HA flapping to *unavailable*. MQTT inverts it: connect out,
publish, sleep — **shortest possible wake**. HA ingests via its MQTT integration +
MQTT Discovery.

---

## Target architecture — the wake pass

`run_cycle` (host, in `plant-shell`) returns normally; the divergent
`esp_deep_sleep_start()` is the **last line of `main()`**, so the shell stays
host-testable and unsafe-free.

```
reset → wake = PowerController::wake_reason()            (safe: esp_idf_hal reset cause)
      → load Calibration from NVS   (qhw.29 — hard predecessor; safe default fallback)
      → retain AXP192 over the internal I²C RefCell (screen stays OFF on timer wake)
      → [if wake == Button] screen_on → render latest ~5 s → screen_off
      → probe burst: gated_read(energize → 64× oversample → deenergize)   (qhw.31)
      → battery% = AXP192.battery_voltage_mv() → plant_core LiPo curve
      → WiFi assoc → EspMqttClient::new(mqtt://…)
      → publish RETAINED discovery(soil,battery) + RETAINED state(soil,battery)  QoS1
      → AWAIT PUBACKs (bounded)     ← the critical flush; sleeping early loses states
      → drop client (clean DISCONNECT)
      ── run_cycle returns ──
      → PowerController::deep_sleep(plan{timer, button_wake})  -> !   // main()'s last line
```

Timer wake ≈ WiFi(2–4 s) + publish(1–2 s) ≈ **~5 s**, headless. Button wake is
**display-only by default** (sample → render ~5 s → sleep); publishing on button
wake is a config option.

---

## Hex-arch seams (MQTT = an outbound-adapter choice over the same domain)

- **Domain (`plant-core`, pure/no_std, host-tested):** add a `BatteryPct` newtype
  (0..=100, same construction discipline as `Moisture::new`), a `Telemetry { soil:
  Option<Measurement>, battery: Option<BatteryPct> }` value, and a pure LiPo
  `voltage_mv → BatteryPct` curve. Keeps the battery curve in the domain, host-tested.
- **Outbound port (`plant-shell`):** `TelemetrySink { fn publish(&mut self, t:
  &Telemetry) -> Result<(), Self::Error> }` — the network twin of
  `plant_core::MoistureDisplay` (ports.rs:52). Plus `BatterySensor` (twin of
  `SoilSensor`). Both host-tested against fakes.
- **Payload rendering (pure, host-tested):** a function `Telemetry → [(topic, json,
  retain, qos)]` producing the HA MQTT-Discovery config + state payloads — golden-
  tested (zero/one/many sensors), mirroring how `esphome-api` keeps tested-protocol
  out of firmware-transport. Lives in `plant-shell` (or a small `telemetry` module).
- **Driven adapter (`firmware/adapters/src/mqtt.rs`, new — sibling of `st7789.rs`):**
  `HassMqtt` wraps `EspMqttClient`, implements `TelemetrySink`, transports the
  rendered payloads (QoS1, retain, **awaits PUBACK**, clean disconnect). Render JSON
  with `heapless::String`/`write!` to stay off-heap on the no-PSRAM chip (the st7789
  adapter already uses `heapless`). No `unsafe`.
- **`BatterySensor` adapter (`firmware/adapters/src/battery.rs`, new):** AXP192-backed,
  reads `battery_voltage_mv()` over the shared I²C RefCell.

`plant-core`/`firmware-core` don't change shape; the powered siblings keep rendering
`Measurement` through `SensorDevice`, the plant monitor renders through `HassMqtt`.

---

## HA integration — MQTT Discovery specifics

- **Discovery (retained):** `homeassistant/sensor/plantmon/soil_moisture/config` and
  `.../battery/config`. `plantmon` reuses `DEVICE_NAME` (main.rs:82); `soil_moisture`
  reuses the existing id (main.rs:243). Payload fields map 1:1 from the current
  `SensorConfig` (main.rs:242) + `DeviceInfoResponse` (main.rs:234): `name`,
  `unique_id`, `state_topic`, `unit_of_measurement`, `device_class`
  (`moisture`/`battery`), `state_class`, `expire_after`, shared `device` block (both
  entities under one HA device).
- **State (retained, QoS1):** `plantmon/soil_moisture/state` (bare percent) and
  `plantmon/battery/state`.
- **Availability = `expire_after`, NOT LWT.** The device disconnects *cleanly* every
  cycle, so an LWT never fires. Set `expire_after` ≈ **2–2.5× `sleep_duration`** in one
  place (tolerates one missed wake before HA shows *unavailable*).
- **Republish discovery every wake.** The broker retains, so "once" suffices — but a
  Mosquitto restart without persistence drops it and the entity vanishes silently.
  Re-publishing the tiny retained config each wake self-heals a broker restart.
- **Credentials** (`MQTT_URL`/`MQTT_USER`/`MQTT_PASS`) baked from `secrets.toml` via
  the existing `firmware-infra/build.rs` seam (same as `WIFI_SSID`). Plaintext
  `mqtt://` on-LAN — no TLS/certs, matching the project's plaintext-first posture.

---

## Work breakdown (phased)

### Phase 0 — infra (blocks end-to-end validation; parallel with firmware)
Reverse `qhw.11`'s recorded "**No MQTT or Mosquitto**" decision: add **Mosquitto** (HA
add-on or a sibling Podman-Quadlet) + the HA **MQTT integration**, and re-express
`qhw.22`'s acceptance (adopted / graphs / alert / 24 h soak) against MQTT-Discovery
entities. Ops work, not firmware.

### Phase 1 — the enabling seam (no user-visible change yet)
- **`PowerController` port** → new `firmware-core/src/power.rs`, beside `ProbePower`.
  Pure signatures only, so `forbid(unsafe)` holds:
  ```rust
  pub enum WakeReason { ColdBoot, Timer, Button, Other }
  pub struct SleepPlan { pub timer: core::time::Duration, pub button_wake: bool }
  pub trait PowerController {
      type Error;
      fn wake_reason(&self) -> WakeReason;                 // safe read
      fn deep_sleep(self, plan: SleepPlan) -> !;           // arm timer+ext0, then sleep
  }
  ```
- **The one audited `unsafe` home** → downgrade **only** `board-support/src/lib.rs`
  from `#![forbid(unsafe_code)]` to `#![deny(unsafe_code)]`, then put the adapter in a
  new `board-support/src/power.rs` carrying a module-level `#[allow(unsafe_code)]` +
  audit block. `deny` (unlike `forbid`) *is* locally overridable, so every other
  module stays no-unsafe and the audit surface is exactly one file:
  `esp_sleep_enable_timer_wakeup`, `esp_sleep_enable_ext0_wakeup(GPIO_NUM_37, 0)`
  (active-low), `esp_deep_sleep_start` — and later `.rtc.data` statics + `esp_wifi_set_ps`.
  `wake_reason` stays **safe** via `esp_idf_hal::reset::WakeupReason` (verify variant
  names on the pinned hal). *Alternative (tighter blast radius, one extra crate): a
  dedicated `firmware/power` crate as the only non-`forbid` crate — rejected as primary
  to match the "in the BSP adapter" decision and keep the crate graph flat.*
- **Retained AXP192 facade** — lift the `RefCell<I2cDriver>` + `Axp192` out of the
  scoped block (main.rs:157–172) to live through the wake. New **safe I²C** methods on
  `board-support/src/axp192.rs` (crate keeps no-unsafe): `screen_on()`/`screen_off()`
  (RMW reg 0x12 LDO2|LDO3; DCDC1 bit0 untouched), `set_brightness()` (reg 0x28),
  `battery_voltage_mv()` (regs 0x78/0x79). Single-threaded wake ⇒ a plain `RefCell`
  suffices (no `Arc`/`Mutex`); MPU6886 joins the same bus via another `RefCellDevice`.
- Prove this on the **existing always-on path first** — battery% shows up over the
  current native-API/TFT build before any sleep exists.

### Phase 2 — `qhw.29` (calibration → NVS) — **hard predecessor**
Deep sleep = full reboot = RAM wiped (`.rtc.data` survives resets but not battery-dead;
NVS survives power-off). Without persisted `Calibration`, every wake reverts to
`PROVISIONAL_CAL` (main.rs:63) and a button-captured calibration is lost on the next
sleep — the device could never *be* calibrated. Load `Calibration` from NVS (the
`EspDefaultNvsPartition` is already taken, main.rs:115) before DutyCycle is useful.
**BtnA now has two jobs** — ext0 wake *and* the qhw.29 calibration trigger; resolve by
wake-reason + press-duration (short = wake+render; long/second press within the render
window = capture). qhw.29's "torn write never yields a degenerate `dry==wet` pair" AC
matters far more on a device that reboots every cycle.

### Phase 3 — the MQTT one-shot (the feature)
- Domain `BatteryPct`/`Telemetry`/curve; `plant-shell` `TelemetrySink`/`BatterySensor`
  ports + `duty_cycle::run_cycle(...) -> Result` (one linear pass, **no threads**,
  reusing the pure halves of `sampler.rs`/`display.rs`, host-tested with fakes).
- `firmware/adapters/src/mqtt.rs` `HassMqtt` + `battery.rs`.
- **`power_profile` as a runtime config value** (baked from `secrets.toml`/env), not a
  Cargo feature: `AlwaysOn` = today's threaded path verbatim (bench/USB dev — deep
  sleep kills `espflash monitor`, so this stays valuable); `DutyCycle` = `run_cycle` +
  `deep_sleep`. One image, both paths type-checked; `main()` shares bring-up and
  branches only at the "spawn threads + loop" vs "run_cycle + sleep" tail. The
  DutyCycle arm **stops constructing `SensorDevice`/`Server`** (main.rs:234–277).
- Flip the default to `DutyCycle` on the battery build; validate against Phase-0 HA.

### Phase 4 — close `qhw.31` for real
Swap `AlwaysOn` (adapters/src/probe_power.rs) for an AXP192-EXTEN `ProbePower`; the
burst energizes only across `gated_read` (adc.rs:91 already routes through it, so this
is a `P`-swap with zero adapter change). Between wakes the board is off ⇒ gating is
nearly free. **Closes qhw.31.**

### Phase 5 — battery-floor reduction (after correctness)
- **Sleep the MPU6886** (I²C reg 0x6B SLEEP bit) — **safe**, new
  `board-support/src/mpu6886.rs`, called once per cold boot. *Load-bearing for the
  ~2–3 day headline* — skipping it leaves ~2.5 mA on DCDC1.
- **WiFi fast reconnect** — static IP (skip DHCP) + pinned BSSID/channel in
  `ClientConfiguration` are **safe** (extend firmware-infra/src/wifi.rs). Persisting
  BSSID/channel across a reboot needs `.rtc.data` (**unsafe**) behind a safe `RtcStore`
  in the audited `power.rs`; it is a **cache only** (vanishes on battery-dead) — every
  load must degrade to a full scan + DHCP. **Calibration must NOT live here** (→ NVS).
- **`esp_wifi_set_ps`** — unsafe FFI → the audited `power.rs`.
- **Configurable/longer `sleep_duration`** — safe, a `Duration` in `SleepPlan`.

### OTA (`qhw.12/23`) — sequence explicitly, scope change
Push-then-sleep leaves **no inbound path** to a sleeping device — the serve-window
that would have been the OTA opportunity **doesn't exist**. `qhw.12` (slot mechanism,
`EspOta`) is unaffected; `qhw.23` (fetch/trigger) must adopt one of: **(a)** each wake,
after publishing, briefly subscribe to an MQTT command/OTA topic for a bounded window
before sleeping (airtime every cycle), or **(b)** a dedicated **maintenance wake reason**
(BtnB / long BtnA hold) that skips `deep_sleep` and holds the device online for OTA.
Decide before DutyCycle becomes the only build.

---

## Beads to create (epic `stick-c-plus-qhw`, "power/battery" theme)

Specify *what*, not *how*. Rough shape (+ deps):

- **kb** — capture the M5StickC Plus power model (research write-up).
- **infra** — Mosquitto + HA MQTT integration; re-express qhw.22 AC vs MQTT-Discovery
  (reverses qhw.11's "no MQTT").
- **power port + audited-unsafe adapter** — `PowerController` in `firmware-core`;
  `board-support` forbid→deny + `power.rs` (deep sleep, timer+ext0 wake, safe WakeReason).
- **retained AXP192 facade + battery telemetry** — `screen_on/off`, brightness,
  `battery_voltage_mv` (safe I²C) + LiPo curve in `plant-core`.
- **MQTT telemetry** — `Telemetry`/`BatteryPct` domain values, `TelemetrySink` port,
  host-tested payload renderer, `firmware/adapters/src/mqtt.rs` `HassMqtt`.
- **duty-cycle profile** — `run_cycle` + `power_profile` runtime branch. *deps:* power
  port, AXP192 facade, MQTT telemetry, **qhw.29**.
- **real ProbePower gating** — converges/**closes qhw.31**.
- **floor reduction** — MPU6886 sleep, WiFi fast reconnect (+`.rtc.data`), `set_ps`,
  configurable period. *deps:* power port, duty-cycle profile.
- **OTA-over-sleep decision** — scope change on **qhw.23** (option a or b).
- **HA/Mosquitto validation** — 24 h soak + measured drain. *deps:* duty-cycle; needs
  Phase 0 + qhw.11/22.

**qhw.29 is a hard predecessor; qhw.31 converges here; qhw.23 gains scope.** The
native-API `has_deep_sleep`/serve-window work stays with the powered siblings (#2/#3).

---

## Verification

- **Host tests** (`cargo test`): the LiPo curve + `BatteryPct` (zero/one/many), the
  payload renderer (golden JSON), `duty_cycle::run_cycle` with fake
  `SoilSensor`/`BatterySensor`/`TelemetrySink`/display, `PowerController` against a
  fake. Keep every test cyclomatic-1, matching `firmware-core/src/probe_power.rs`.
- **On-metal, first wake:** flash the DutyCycle build; over serial (`--non-interactive`)
  confirm the single pass runs *before* it sleeps. Confirm publish:
  `mosquitto_sub -v -t 'homeassistant/#' -t 'plantmon/#'`.
- **HA:** entity auto-discovers + graphs; leave a cycle, confirm it keeps its value
  (not *unavailable*) until `expire_after`.
- **Sleep proof:** current collapses on sleep — external USB power meter and/or the new
  `battery_voltage_mv()` trend; measure the floor (~2 mA pre-IMU-sleep, dropping after
  Phase 5).
- **Wake sources:** RTC-timer wake (periodic publish) and Button A wake (screen ~5 s,
  no publish by default).
- **Soak:** 24 h+; extrapolate to the days target from measured floor + burst.

---

## Top risks

1. **Publish-before-sleep race** — `EspMqttClient` is callback-driven; `publish()`
   returning is *not* delivery. Must QoS1 + await PUBACK (bounded) before
   `deep_sleep()`, or readings are silently lost (the MQTT "sleeps too early" bug).
2. **The ~2 mA floor needs the IMU slept** — skipping the MPU6886 `0x6B` write leaves
   ~2.5 mA on DCDC1 and misses the target. Required, not optional.
3. **Unsafe boundary integrity** — exactly one crate goes `deny`, one module holds
   `#[allow]` for *both* sleep FFI and `.rtc.data`. Keep `wake_reason`, MPU6886-sleep,
   and MQTT on safe APIs; let effect-audit/hex-lint guard it.
4. **`expire_after` vs `sleep_duration` mismatch** flaps availability — tie them in one
   place (~2×).
5. **Retained-config loss on broker restart** — mitigate by republishing retained
   discovery every wake.
6. **Torn NVS calibration** (qhw.29 AC) — far higher exposure under reboot-per-wake.
7. **RTC-memory fragility** — a cache that vanishes on battery removal; never trust it
   for correctness; the full-scan/DHCP fallback must be real.
8. **BtnA double-duty** — ext0 wake vs qhw.29 capture; disambiguate by wake-reason +
   press-duration.
9. **OTA reachability** — no inbound path under push; decide option (a)/(b) before
   DutyCycle is the only build.

---

## Critical files

- `firmware-core/src/power.rs` (new) + `lib.rs` — safe `PowerController` + `WakeReason`.
- `firmware/board-support/src/lib.rs` (`forbid`→`deny`) + `power.rs` (new, the one
  audited-unsafe module) + `axp192.rs` (retained facade: rail toggles, brightness,
  `battery_voltage_mv`) + `mpu6886.rs` (new, Phase 5, safe IMU sleep).
- `plant-core/src/` — `BatteryPct`/`Telemetry` values + LiPo curve.
- `plant-shell/src/duty_cycle.rs` (new) — one-shot `run_cycle`; `TelemetrySink`/
  `BatterySensor` ports + host-tested payload renderer.
- `firmware/adapters/src/mqtt.rs` (new, `HassMqtt`) + `battery.rs` (new) +
  `probe_power.rs` (real `ProbePower`, closes qhw.31).
- `firmware/bins/plant-monitor/src/main.rs` — `power_profile` runtime branch, retained
  AXP192 lifetime lift, DutyCycle arm drops server spawn, `deep_sleep()` as the last line.
- `firmware-infra/build.rs` — bake `MQTT_URL`/`MQTT_USER`/`MQTT_PASS` (reuse WiFi seam).
- `kb/guides/m5stickc-plus-power-model.md` (new) — the research capture.
