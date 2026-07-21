---
id: axp192-exten-gates-grove-5v
title: "AXP192 EXTEN (reg 0x12 bit 6) gates the Grove 5 V rail — so probe power-gating is firmware-only"
confidence: high
scope: board:m5stickc-plus
derived-from: [2026-07-08-probe-rail-gating]
supersedes: []
reviewed: 2026-07-08
check: grep -q 'const EXTEN: u8 = 0x40' firmware/platform/board-support/src/axp192.rs && grep -Fq 'rail=off t=+ 500ms raw=0' kb/experiments/2026-07-08-probe-rail-gating/rail-sweep.log
---

**Claim:** Clearing **bit 6 (EXTEN)** of AXP192 register `0x12` removes power from
the M5StickC Plus **Grove port's 5 V pin**, and setting it restores power. Nothing
else moves: DCDC1/DCDC3 and the display LDOs are untouched, and the board keeps
running. Gating the Earth Unit's power therefore needs **no external switch** —
qhw.31 is a firmware change inside the existing `ProbePower` seam.

**Evidence:** [2026-07-08-probe-rail-gating](../experiments/2026-07-08-probe-rail-gating/README.md).
With EXTEN cleared, the Earth Unit's analog node on GPIO33 falls from a saturated
`4095` to a hard `0` within 500 ms and stays there; setting EXTEN returns it to the
rail. Three cycles, `reg_0x12` read back `0x5F ↔ 0x1F` each time (bit 6 alone).
The probe's electrodes had corroded open, so no current flowed through its 10 kΩ
pull-up (R1) and the node reported the probe's own 3.3 V rail directly — the ADC
acted as a voltmeter on the rail. No multimeter was used, or needed.

**Holds when:** driving the Grove port's 5 V pin on the M5StickC Plus. The register
semantics are the AXP192's, so any board wiring EXTEN to its external boost behaves
the same.

**Breaks when:** you need the *settle delay*. The rise is slow (1–3 s) and drifts
between cycles when the probe is open-circuit and the boost runs unloaded; that
number is a rig artifact, **not** a board constant, and must be re-measured with a
conducting probe (`just run-bin probe-rail-check`) before it is hardcoded. Also
untested on battery — EXTEN gated fine on USB, which is the harder case if Grove 5 V
were VBUS-fed, but the null result was never exercised.

**How to apply:** `board_support::Axp192::set_exten` is the mechanism (a
read-modify-write, so it can never disturb the rail the ESP32 runs on). Wrap it in a
`GroveRail` implementing `firmware_core::ProbePower` and hand that to
`adapters::adc::EarthUnit` in place of `AlwaysOn`.

Put `GroveRail` in **`adapters`**, not `board-support`, despite what qhw.31
suggests: `hex-lint explain infra` says an infra crate may depend only on infra, and
`ProbePower` lives in `firmware-core` (`role=domain`). A `driven-adapter` may depend
on `infra`, so the AXP192 register knowledge stays in `board-support` and the port
implementation sits beside `AlwaysOn`.

Two traps worth naming:

- The bias comes from the probe's own pull-up, **not** from `read_raw`. The
  electrodes corrode whenever the rail is up, whether or not the firmware ever
  samples. Sampling less often achieves nothing; only cutting the rail does.
- The AXP192 **holds its rail state through ESP32 deep sleep**. A duty-cycled or
  deep-sleeping monitor must `deenergize` *before* it sleeps, or it corrodes the
  probe at full speed while the CPU is idle.

A settle delay long enough to be safe (order of a second) exceeds the current 2 s
sample period, so gating forces the sample period up — which the deep-sleep
duty-cycle refactor does anyway.

**Corrects:** [axp192-powers-lcd-backlight](axp192-powers-lcd-backlight.md), whose
"Breaks when" claimed the Grove 5 V pin does not need the AXP192. It does: `power_on`
sets EXTEN, and without EXTEN the pin is dead.
