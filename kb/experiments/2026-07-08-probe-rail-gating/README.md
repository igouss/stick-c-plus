---
id: 2026-07-08-probe-rail-gating
title: "Does the AXP192's EXTEN bit gate the Grove 5 V rail that powers the Earth Unit?"
date: 2026-07-08
domain: [esp32, power, adc, soil-probe]
status: confirmed
hardware: "M5StickC Plus @ /dev/ttyUSB0, FT232, ESP32 rev v1.1, MAC DE:AD:BE:EF:00:01; M5 Earth Unit (U019) in the Grove port, electrodes corroded open, left in soil"
artifacts: ./run.sh
findings: [axp192-exten-gates-grove-5v, saturated-adc-reading-is-not-a-measurement]
source: [m5-earth-unit, m5stickc-plus-pinout]
---

## Question

The Earth Unit's electrodes corrode under constant DC bias, so qhw.31 wants the
probe powered only across a sample. That needs a switch. **Does clearing EXTEN (bit
6 of AXP192 reg `0x12`) actually cut the Grove port's 5 V pin?**

The answer decides the shape of qhw.31: if EXTEN gates the rail, the fix is pure
firmware inside the existing `ProbePower` seam. If it does not, the Grove 5 V line
needs an external high-side switch, and qhw.31 becomes a hardware change.

The prior art disagreed with itself. [axp192-powers-lcd-backlight](../../findings/axp192-powers-lcd-backlight.md)
asserted in passing that "an external strip on the Grove 5 V pin does **not** need
the AXP192" — yet `Axp192::power_on` sets EXTEN as part of its factory bring-up
sequence, and EXTEN is conventionally the enable for the board's external 5 V boost.
Both could not be right.

## Hypothesis

Written before measuring: **EXTEN gates it.** Reasoning — the factory `begin()`
sequence sets EXTEN alongside the display LDOs, which would be pointless if the bit
drove nothing on this board; and the AXP192's EXTEN pin exists to enable exactly
such an external boost.

Predicted failure mode if wrong: `raw` stays pinned at 4095 with EXTEN cleared.

## Method (reproducible)

```sh
kb/experiments/2026-07-08-probe-rail-gating/run.sh    # == just run-bin probe-rail-check
just run                                              # restore the monitor afterwards
```

`firmware/bins/plant-monitor/src/probe_rail_check.rs` brings up the PMIC and ADC1
ch5 / GPIO33, then repeats three cycles of: clear EXTEN → sample the node at
`0,10,50,100,250,500,1000,2000` ms; set EXTEN → sample at
`0,1,2,5,10,20,50,100,200,500,750,1000,1500,2000,3000,4000` ms. Each sample is the
mean of 64 conversions — the same `firmware_core::oversampled_mean` burst the
`EarthUnit` adapter uses, so the counts are directly comparable with the monitor's
logged `raw`. Register `0x12` is read back after every write.

**Why the ADC can answer a question about a power rail.** Per the vendored
schematic ([m5-earth-unit](../../sources/m5-earth-unit.md)), the Earth Unit
regulates Grove 5 V down to 3.3 V locally (HT7533) and pulls its analog output up
to *that* rail through R1 = 10 kΩ, with the soil in the **lower** leg of the
divider. This probe's electrodes have corroded open, so the lower leg is gone: no
current flows through R1, no drop across it, and the node sits at the probe's own
3.3 V rail. **The dead probe is a voltmeter on the rail we are trying to switch.**

Toolchain: `esp` rustc fork, `xtensa-esp32-espidf`, ESP-IDF v5.3.3, espflash @
115200. Serial capture: [`rail-sweep.log`](./rail-sweep.log).

## Raw results

Verbatim in [`rail-sweep.log`](./rail-sweep.log). Abridged, cycle 1:

```
baseline: raw=4095 exten=true reg_0x12=0x5F
  exten<-off: read back exten=false reg_0x12=0x1F
  rail=off t=+   0ms raw=4095
  rail=off t=+  10ms raw=4095
  rail=off t=+  50ms raw=2024
  rail=off t=+ 100ms raw=899
  rail=off t=+ 250ms raw=300
  rail=off t=+ 500ms raw=0
  rail=off t=+1000ms raw=0
  rail=off t=+2000ms raw=0
  exten<-on: read back exten=true reg_0x12=0x5F
  rail=on  t=+   0ms raw=3982
  rail=on  t=+   1ms raw=3218
  rail=on  t=+  10ms raw=3252
  rail=on  t=+ 100ms raw=3305
  rail=on  t=+ 500ms raw=3463
  rail=on  t=+1000ms raw=3729
  rail=on  t=+2000ms raw=3969
  rail=on  t=+3000ms raw=4095
  rail=on  t=+4000ms raw=4095
```

Fall: identical in all three cycles — `4095` held for ~10 ms, then a decay reaching
exactly `0` by 500 ms and staying there.

Rise: **not** identical. Time to first reach 4095 was 3000 ms (cycle 1), 1000 ms
(cycle 2), and sooner again in cycle 3. Every cycle showed the same shape: a high
first sample (`3982`/`3869`/`3883`) taken within ~1 ms of the register write, a drop
to ~3200 by t=1 ms, then a slow monotone climb.

`reg_0x12` read back `0x5F` ↔ `0x1F` across every toggle — bit 6 moving alone.
DCDC1 (bit 0), DCDC3 (bit 1) and the display LDOs (bits 2–3) never changed, and the
board did not reset or blank.

## Verdict

**EXTEN gates the Grove 5 V rail. Confirmed.** With bit 6 cleared the probe's node
falls to a hard 0 and stays there; with it set the node returns to its rail. Three
for three, with the register read back each time.

Two consequences, promoted to findings:

- qhw.31 is a **firmware-only** change — [axp192-exten-gates-grove-5v](../../findings/axp192-exten-gates-grove-5v.md).
- `raw = 0` is now a *second* way for the ADC to lie: a probe that failed to
  energize reads the floor, exactly as an open probe reads the ceiling. Both rails
  must be rejected at the adapter boundary —
  [saturated-adc-reading-is-not-a-measurement](../../findings/saturated-adc-reading-is-not-a-measurement.md).

The settle delay is **not** established by this experiment. See below.

## Threats to validity

- **The settle number is an artifact of the dead probe.** With the electrodes open
  the LDO drives almost no load, so the 5 V boost runs in a near-unloaded regime;
  that is the most likely reason the rise took 1–3 s and drifted between cycles
  while the fall was perfectly repeatable. A healthy probe pulls ~0.3 mA through R1
  and should settle sooner and more deterministically. **Do not hardcode 3 s.**
  Re-run this with the replacement probe fitted — a few seconds of bias is harmless
  — and read the plateau off that. This tool is the calibration step for a new probe.
- **The high first rise sample is unexplained.** `raw ≈ 3900` within ~1 ms of
  enabling, then a drop to ~3200, then the slow climb. Plausibly a soft-start
  overshoot on an unloaded boost, but not chased down. It does not affect the
  verdict (the fall is what proves gating), and any settle delay long enough to be
  useful is well past it.
- **Powered over USB throughout.** If Grove 5 V were fed from VBUS rather than the
  boost, EXTEN could have failed to gate while plugged in. It gated anyway, so this
  threat is closed for the *positive* result — but a null result here would have
  needed a battery re-run before concluding anything.
- **One board, one probe.** The register semantics are AXP192-generic; the settle
  behaviour is not claimed beyond this rig.
