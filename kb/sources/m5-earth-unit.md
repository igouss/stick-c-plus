---
id: m5-earth-unit
title: "M5 Earth Unit (U019) — resistive soil-moisture sensor"
type: datasheet
author: M5Stack
publisher: M5Stack (docs.m5stack.com)
url: https://docs.m5stack.com/en/unit/earth
retrieved: 2026-07-04
license: "© M5Stack; local reference copy of the schematic, not redistributed"
material: ./m5-earth-unit/    # schematic PDF gitignored; fetch.sh reproduces it
seeds: []
---

## Citation

M5Stack. *Unit Earth (U019) — soil-moisture sensor.* M5Stack Docs,
<https://docs.m5stack.com/en/unit/earth>. Schematic `U019_UNIT_EARTH_SCHE.pdf`.
Retrieved 2026-07-04.

## What it is

The soil-moisture probe of the plant monitor. A **resistive** sensor: two
exposed traces whose resistance falls as the soil around them gets wetter, read
as an **analog** voltage. Grove connector; on this board it lands on the Grove
port (**G33 = ADC1 channel 5**, G32 = the digital line).

Two consequences the domain already encodes:

- **Resistive, not capacitive** — the traces corrode over time, so the firmware
  power-gates the probe (energise only while sampling; qhw.31) and the calibration
  records two raw endpoints without assuming which reads higher (see
  [`plant-core/src/moisture.rs`] and qhw.29).
- **ADC1 for WiFi coexistence** — the probe must sit on **ADC1** (G33); ADC2 is
  unusable while WiFi is active on the ESP32. This pins the wiring choice.

The `plant-core` domain turns its raw 12-bit reading (`0..=4095`) into a
calibrated `Moisture` percentage; this note is the hardware record behind that.

## Regenerate (the reproducibility primitive)

```sh
# Idempotent: an existing valid PDF is skipped.
kb/sources/m5-earth-unit/fetch.sh
```

## What to read, and why

- `earth_unit_schematic.pdf` — the divider/output wiring: confirms the analog
  output pin and that there is no on-board ADC (the ESP32 reads it directly),
  the basis for the ADC1/G33 adapter (qhw.5).
