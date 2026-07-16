---
id: host-monitor-node-exporter
title: "host-monitor: enabling node_exporter and the scrape it reads"
kind: guide
scope: project:stick-c-plus
reviewed: 2026-07-15
---

The **host-monitor** app turns the M5StickC Plus into a desk display of a Linux
host's CPU and memory, drawn as two scrolling sparklines. The board is a pure
*client*: it scrapes the host's [node_exporter] over HTTP and does the arithmetic
itself. This guide covers the host side (installing the exporter) and the exact
metrics the device reads, so the on-device parser can be checked against a real
capture.

[node_exporter]: https://github.com/prometheus/node_exporter

## Host side — run node_exporter on the Fedora box

node_exporter serves the Prometheus text exposition format on `:9100/metrics`. It
needs no configuration for what host-monitor reads (CPU and memory are default
collectors).

```sh
# Fedora
sudo dnf install golang-github-prometheus-node-exporter
sudo systemctl enable --now node_exporter
# or the upstream binary + a systemd unit; either serves :9100/metrics.

# Confirm it answers, and capture a real scrape for the golden fixture:
curl -s http://localhost:9100/metrics | head
```

Then set the board's target in the git-ignored `firmware/secrets.toml`:

```toml
[host_monitor]
address = "192.168.1.10:9100"   # the host's LAN IP (or name) : port
```

and flash: `just run-host-monitor`.

## What the device reads (and why)

Of the thousands of series in a scrape, host-monitor's streaming parser
(`host_core::prometheus`) keeps four, ignoring everything else:

| Metric | Use |
|---|---|
| `node_cpu_seconds_total{cpu="…",mode="idle"}` | summed → idle CPU-seconds |
| `node_cpu_seconds_total` (every mode) | summed → total CPU-seconds |
| `node_memory_MemTotal_bytes` | memory denominator |
| `node_memory_MemAvailable_bytes` | memory numerator |

- **CPU is a rate.** `node_cpu_seconds_total` is a *cumulative* counter, so one read
  says nothing about current load. The device keeps the previous scrape's counters
  and computes the busy fraction between two reads:
  `cpu% = 100 · (1 − Δidle/Δtotal)`. The first scrape after boot only primes the
  baseline — the display shows *starting* for one poll period (~2 s) until the second
  scrape yields a rate.
- **Memory is a level.** `mem% = 100 · (1 − MemAvailable/MemTotal)`, read straight
  from one scrape. (`MemAvailable` is the kernel's own estimate of allocatable memory,
  which is what "used" should mean — not `MemFree`.)

## Gotchas the parser handles (verify against a real capture)

- **Scientific notation.** node_exporter formats float64 values with Go's shortest
  `'g'` form, so `node_memory_MemTotal_bytes` prints as e.g. `1.66508544e+10`, *not*
  `16650854400`. The parser reads values as `f64` for exactly this reason; hand-rolled
  integer parsing would misread them.
- **Large body.** A full scrape is tens of KB. The device never holds it: the HTTP
  adapter (`host_adapters::http`) reads the socket a chunk at a time and folds each
  line into the parser, so memory stays bounded.
- **Counter resets.** A node_exporter restart resets the counters; the device sees a
  negative `Δtotal`, reports no sample that cycle (rather than a glitch), and re-bases.

**Pin the golden fixture to your box.** The parser's golden test
(`host-core/src/prometheus.rs`) ships a representative body, but CPU line counts (one
per core per mode) and even the presence of `MemAvailable` vary by kernel and
node_exporter version. Capture `curl -s http://<host>:9100/metrics` from the real host
and update the fixture before trusting the on-device numbers.

## What the glass shows

Two stacked sparklines — CPU on top (cyan), memory below (yellow) — each labelled with
its live percentage, red once it hits the pegged threshold (85 %). A Claude creature on
the right is the load: **breathing** when calm, **coding** when busy, a **frantic
dance** when pegged; **startled** when the host stops answering (poller alive, host
down), **asleep** when the poller thread itself has died, **thinking** while warming up.
The graph keeps its trailing window even when the host goes dark — a record of what it
was doing is useful, unlike a frozen scalar. `just screens` renders every state to
`target/screens/host-monitor-*.png` without a board.
