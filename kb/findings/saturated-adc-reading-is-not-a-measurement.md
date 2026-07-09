---
id: saturated-adc-reading-is-not-a-measurement
title: "raw 4095 (or 0) from the Earth Unit is a diagnostic, not a reading — reject the rails at the adapter"
confidence: high
scope: project:stick-c-plus
derived-from: [2026-07-08-probe-rail-gating]
supersedes: []
reviewed: 2026-07-08
check: grep -q 'pub fn unsaturated' firmware-core/src/saturation.rs && grep -q 'ReadError::Saturated' firmware/adapters/src/adc.rs
---

**Claim:** An ADC count sitting on either rail carries **no information** — every
input beyond the rail maps to the same number — yet the calibration curve maps it to
a confident `0 %` or `100 %`. On the Earth Unit the two rails have *distinct
physical causes*, and a third fault mode looks like neither. All three must be
distinguished at the ADC boundary, because none of them is a read *error*: the
conversion succeeds every time.

The probe's wiring (schematic, [m5-earth-unit](../sources/m5-earth-unit.md)):
Grove 5 V is regulated locally to 3.3 V by an HT7533, and the analog output is a
divider `+3.3V → R1 (10 kΩ) → Ain → [soil electrodes] → GND`. **The soil is the
lower leg.** So:

| observed | cause | what the divider is doing |
|---|---|---|
| `raw = 4095` (→ 0 % dry) | electrodes **open**: corroded through, or soil past the readable range | lower leg gone; R1 pulls `Ain` to the probe's 3.3 V rail, above the ADC's 12 dB ceiling |
| `raw = 0` (→ 100 % wet) | the probe **rail is down** — it never energized | no pull-up; `Ain` at ground |
| a plausible mid-scale value | the Grove cable is **unplugged** | R1 is gone with the probe; GPIO33 simply floats |

The third row is the nastiest and is *not* caught by rejecting the rails.

**Evidence:** [2026-07-08-probe-rail-gating](../experiments/2026-07-08-probe-rail-gating/README.md)
measured all three. A probe whose electrodes had corroded open read exactly `4095`
on every one of 64 conversions, from cold boot, for two days — and the monitor
served a serene `0 %` to Home Assistant the whole time. Cutting the rail (EXTEN)
drove the same node to exactly `0`. And qhw.32 records, verified on-board
2026-07-04, that an unplugged probe floats to a *readable* voltage which the
pipeline published as a plausible-but-false `100 %`.

The saturation is unambiguous, not marginal: `oversampled_mean` averages 64
conversions with integer division, so a mean of 4095 requires **every** conversion
in the burst to have pinned.

**Holds when:** any divider-fed resistive sensor read through an ADC whose usable
range is narrower than the sensor's swing. With R1 = 10 kΩ against the ESP32's 12 dB
ceiling, the node crosses out of range somewhere above ~30 kΩ of soil resistance —
so a **healthy** probe in merely dry soil saturates well before the soil is bone
dry, and the provisional `dry_raw = 2600` (≈ 15 kΩ) sits *below* that cliff.

**Breaks when:** you want to alert on a dry plant. Rejecting the ceiling means a dry
pot now reads *unavailable* rather than `0 %` — truthful, since the ADC genuinely
cannot see it, but the watering alert must treat unavailable as actionable. Reading
the dry end properly needs the real calibration (qhw.29) and probably a pull-up
smaller than the probe's fixed 10 kΩ.

Also: rejecting the rails is **not** the whole of qhw.32. A floating (unplugged)
probe lands mid-scale and sails straight through this check. Catching it needs an
*excitation delta* — sample with the rail off, then on; a connected probe's reading
moves, a floating pin's does not — which [axp192-exten-gates-grove-5v](axp192-exten-gates-grove-5v.md)
now makes free.

**How to apply:** `firmware_core::saturation::unsaturated(raw, full_scale)` is the
pure rule; `adapters::adc::EarthUnit` applies it *outside* `gated_read` (whose
contract is corrosion safety, and whose read closure must share the power
mechanism's error type) and surfaces `ReadError::Saturated`. Nothing downstream
changed: `plant_shell`'s sampler already publishes nothing on a failed read, so the
cache ages out and both the TFT and the native-API sensor report unavailable.

Note the native API cannot express "probe fault" for a sensor entity —
`SensorStateResponse` carries only `state` and `missing_state` — so collapsing a
fault into *unavailable* is the protocol's only option today. Distinguishing them in
HA needs a second (binary_sensor) entity.
