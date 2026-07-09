#!/usr/bin/env bash
# Re-run the probe-rail-gating measurement (see README.md).
#
# Flashes `probe-rail-check` — a bench bin in the plant-monitor package — and
# streams its serial output. The tool toggles the AXP192's EXTEN bit and samples
# GPIO33 (the Earth Unit's analog node) across the fall and the rise, so the ADC
# itself reports whether the probe's rail actually died. No multimeter needed.
#
# Rig: M5StickC Plus on /dev/ttyUSB0 (FT232, 115200 — see
# ../../guides/flashing-and-serial-access.md), Earth Unit in the Grove port.
#
# Reading it: `rail=off … raw=0` means EXTEN gates the Grove 5 V rail. The rise
# sweep's first plateaued offset is the settle delay `ProbePower::energize` owes
# its caller — but see README "Threats to validity": measured against an
# OPEN-electrode probe that unloads the boost, so it is an upper bound, not the
# number to hardcode.
#
# Afterwards, restore the monitor:  just run
set -euo pipefail
cd "$(dirname "$0")/../../.."

exec just run-bin probe-rail-check
