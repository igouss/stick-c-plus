---
id: __ID__            # YYYY-MM-DD-<slug>
title: __TITLE__
date: __DATE__
domain: []            # e.g. [esp32, firmware, ble, power]
status: wip           # wip | confirmed | refuted | inconclusive
hardware: __RIG__     # board + how it's attached (e.g. M5StickC Plus @ /dev/ttyUSB0, FT232)
artifacts:            # path to dumps/scripts, e.g. ./read-flash.sh
findings: []          # finding ids this experiment fed
source: []            # source ids this experiment probed
---

## Question
<!-- One sentence: what are we trying to learn about the board? -->

## Hypothesis
<!-- What we expected, and why. Write it down BEFORE you measure. -->

## Method (reproducible)
<!-- Exact commands + rig. You in six months must be able to re-run this blind.
     Note port, baud, chip rev, toolchain versions. -->

## Raw results
<!-- RAW VOICE ONLY. Verbatim output / measurements. No interpretation here. -->

## Verdict
<!-- DERIVED VOICE. What it means. Promote anything durable to findings/. -->

## Threats to validity
<!-- What could make this a lie? Wrong partition, a cached dump, an ambiguous match. -->
