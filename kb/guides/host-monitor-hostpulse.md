---
id: host-monitor-hostpulse
title: "host-monitor: the hostpulse endpoint and what the glass shows"
kind: guide
scope: project:stick-c-plus
reviewed: 2026-07-16
supersedes: host-monitor-node-exporter
---

The **host-monitor** app turns the M5StickC Plus into a desk display of the whole
homelab — one row per host (`fedora`, `oracle-arm`, `oracle-amd`), each with live
CPU and memory percentages and two scrolling sparklines. The board is a pure
*client* of one endpoint, **hostpulse**, which returns a ready-to-plot CPU/memory
series for every host in a single bearer-gated call.

## Why hostpulse (and not node_exporter)

Per-host node_exporter is **not exposed** on this LAN — metrics go to a sealed
Prometheus the M5 can't reach. hostpulse is a thin, read-only, bearer-gated endpoint
bridged onto the LAN that queries that Prometheus and hands back a small JSON frame.
Crucially it has **already done the PromQL `rate()`**, so the device does no
Prometheus-text parsing and no counter/rate arithmetic — it holds N hosts × two
`%`-series and draws them. (The retired scrape path is
[host-monitor-node-exporter](host-monitor-node-exporter.md), kept as history.)

## The contract

```
GET  http://<control-node>:9099/pulse
Header:  Authorization: Bearer <HOSTPULSE_TOKEN>
```

`<control-node>` is the LAN host bridging to Prometheus (currently `10.0.0.10`,
`fedora.local` via mDNS also works); port `9099`. `<HOSTPULSE_TOKEN>` is a 64-hex
bearer in Bitwarden (`observability-secrets / HOSTPULSE_TOKEN`) — **never committed**.

`200 application/json`:

```json
{
  "step_s": 30,
  "window_s": 900,
  "hosts": [
    { "name": "fedora",     "cpu": [11,13,9,12,null,10], "mem": [41,42,42,43,43,44] },
    { "name": "oracle-arm", "cpu": [3,4,3,5,4,4],        "mem": [58,58,59,59,60,60] },
    { "name": "oracle-amd", "cpu": [1,2,1,1,2,1],        "mem": [22,22,23,23,23,24] }
  ]
}
```

Semantics the device relies on:

- **`hosts` is always every host, in order.** A down host appears with all-`null`
  arrays and is rendered as "no data" — never dropped, so the rows don't reshuffle.
- **`cpu`/`mem` are integer percents `0..=100`, oldest→newest**, both on the same
  grid (length ≈ `window_s/step_s + 1`). Out-of-range values are clamped on-device.
- **A `null` element is a gap** — a missing scrape, *not* `0`. The device keeps it as
  a gap and draws it as a dim baseline tick, distinct from a `0%` bar.
- **`step_s`/`window_s` are read from the payload**, never hard-coded: `step_s` paces
  the poll cadence (the device polls at the server's own step, clamped 15–45 s), and
  `window_s` is shown as the corner span label (`900` → `15m`).
- **A Prometheus-side error is `502 {"error":"prometheus_unavailable"}`** — treated as
  *unreachable*: the last good frame is kept and a `DOWN` token shown.
- `GET /healthz` is the liveness probe. (The firmware never calls it; it only fetches
  `/pulse`.)

## Board config

The endpoint and token are baked in at build time from the git-ignored
`firmware/secrets.toml` (the bin's `build.rs` reads `[host_monitor]`):

```toml
[host_monitor]
endpoint = "10.0.0.10:9099"   # host:port, site-specific — not a secret
token    = "…64-hex…"          # SECRET — from Bitwarden, never commit
```

The template with placeholders is `firmware/secrets.toml.example`. The token rides the
same road as the WiFi credentials: git-ignored, baked into the image, sent only in the
`Authorization` header, and never logged.

## Verify before flashing

From any LAN machine that has the token:

```sh
# 200 + the JSON frame:
curl -sS -H "Authorization: Bearer $HOSTPULSE_TOKEN" http://10.0.0.10:9099/pulse | jq
# 403 without the token:
curl -s -o /dev/null -w '%{http_code}\n' http://10.0.0.10:9099/pulse   # -> 403
```

Then flash: `just run-host-monitor` (builds the lean mdns-free IDF, flashes, monitors).
`just run` puts the plant monitor back.

## What the glass shows

Three stacked rows, one per host: `name   CPU <spark> NN%   MEM <spark> NN%`, CPU cyan,
memory yellow, a percentage red once pegged (≥ 85 %). Details:

- **Gaps** in a series draw as dim baseline ticks — "no data here", not a `0%` floor.
- **A down host** (all-`null`) keeps its row and shows "no data" with no bars.
- **The top-right corner** shows the window span (`15m`) when fresh, escalating to a
  health token when not: `DOWN` (unreachable / 502), `BAD` (malformed body), `OLD`
  (poller stale). The host names tint (white → red/dim) to match.
- **The frame outlives the reading**: when the endpoint goes dark the last good window
  stays on the glass (a record of what the hosts were doing is useful), with the
  tint + token saying the numbers are no longer live.

Every state is rendered without a board by `just screens` → `target/screens/*.png`, and
locked against drift by committed golden PNGs (`host-display/goldens/`, checked by the
`goldens` test in `just test`; re-accept an intended change with `just screens-bless`).
