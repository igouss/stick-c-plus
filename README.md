# stick-c-plus — a multi-app board platform for the M5StickC Plus, in Rust

Rust firmware for the **M5StickC Plus** (ESP32-PICO-D4, Xtensa LX6, 4 MB flash,
520 KB SRAM, no PSRAM), built `std` on **ESP-IDF**. Several small **apps** share one
reusable **platform** foundation — the screen, the sprite creature, the buttons, the
buzzer, the clock, and the change-suppressing render loop are written once and reused,
so a new experiment is a new directory under `apps/`, not a new firmware:

1. **pomodoro** — a standalone, offline focus timer: the TFT shows `MM:SS` and a Claude
   creature that **codes** through a focus and **dances** through a break; the two
   buttons drive it and the buzzer sounds each transition. *Screen + buttons + buzzer,
   no network.*
2. **plant-monitor** — an M5 **Earth Unit** soil probe → moisture dashboard, surfaced to
   **Home Assistant** through a home-grown **ESPHome native-API** crate so HA does the
   storing, graphing, and alerting.
3. **host-monitor** — a desk display of every **homelab host's CPU and memory** as live
   scrolling **sparklines**, one row per host: the board fetches a single bearer-gated
   **hostpulse** endpoint over WiFi, which returns a ready-to-plot per-host series for all
   hosts at once (the PromQL `rate()` is done server-side). *Screen + WiFi, a pure metrics
   client.*
4. **orientation** — a live readout of **which way the board is pointing**: the MPU6886
   IMU's gravity vector as three signed X/Y/Z bars, the pitch and roll in degrees, and the
   face it is resting on — laid out for whichever way up the board is being held. *Screen +
   IMU, no network, no buttons — turn it and watch.*
5. **plume** — an ambient, clock-driven **feathered frond** that breathes on the panel: a
   five-thousand-point parametric field (a faithful port of a *Dwitter*) animated on the phase
   clock alone. Its point is the platform's math and rendering under load — a startup-built
   **sine table** replaces every per-point transcendental, and each frame is plotted into a
   one-bit offscreen frame and blitted in a **single** SPI window, not five thousand pokes.
   *Screen only, no sensor, no network — just watch it move.*
6. **led-driver** — a NightDriverStrip-style **WS2812** animation driver (the repo's
   original purpose; the `led-core` effects domain lives on). *Future.*
7. **rover** — a controllable robot. *Future; diverges in hardware.*

## Architecture — hexagonal / ECB, on a shared platform

The domain is framework-free and dependencies point inward. The heart of each app (the
pomodoro FSM, the moisture curve, the LED effects) never mentions hardware, so it is
verified on the host — Gherkin, property, and unit tests — with no device attached. The
firmware is the thin **imperative shell**: adapters that implement the domain's ports
against real ESP-IDF peripherals, plus a composition root per app.

What makes it a *platform* is that the board-generic machinery is carved out of any one
app and shared. The pomodoro timer and the plant monitor drive the **same** generic render
loop, over the same `Screen`/`Animated`/`Clock` ports, painting the same ClaudePix creature
through the same ST7789 panel adapter — each app supplies only its own *picture* and its own
*state*.

```
stick-c-plus/
├─ platform/          # the reusable, app-agnostic foundation (context = "shared")
│  ├─ platform-core/       #   domain          — Tick, the Clock/Screen/Backlight/Tone/AudioIn
│  │                       #                      ports, the Animated contract, the rotation policy
│  ├─ platform-input/      #   domain           — the board's three buttons as one event source:
│  │                       #                      the levelled + latched ports, and the pure
│  │                       #                      click / double-click / long-hold recognizer
│  ├─ platform-audio/      #   domain           — the acoustic level (DC-removed RMS) + sound-present
│  │                       #                      verdict (the chime self-test's ears, host-tested)
│  ├─ platform-display/    #   port-and-adapter — the ClaudePix sprite library, the fixed-width
│  │                       #                      text primitives, the sparkline, the signed axis
│  │                       #                      bar, the colour self-test
│  ├─ platform-runtime/    #   driving-adapter  — the Monotonic clock, the generic change-
│  │                       #                      suppressing render loop (over any Animated state),
│  │                       #                      and the backlight switch it skips a dark paint on
│  ├─ firmware-core/       #   domain           — pure shared kernel (ADC oversampling, gating)
│  ├─ esphome-api/         #   domain           — ESPHome native-API framework (prost + std::net)
│  └─ esphome-server/      #   driving-adapter  — the native-API server host (accept loop → FSM)
├─ apps/              # one bounded context per app, built on the platform
│  ├─ pomodoro/            #   pomodoro-core (FSM) · pomodoro-display (screen) · pomodoro-shell
│  ├─ plant-monitor/       #   plant-core (moisture) · plant-display · plant-shell
│  ├─ host-monitor/        #   host-core (Pulse frame + clamp/gap transform) · host-wire (JSON codec) · host-display · host-shell
│  ├─ orientation/         #   orientation-core (tilt + resting face) · orientation-display · orientation-shell
│  ├─ plume/               #   plume-core (parametric field + sine table + phase) · plume-display
│  └─ led-driver/          #   led-core (WS2812 effects)
├─ firmware/          # the Xtensa boundary — a detached std/ESP-IDF workspace
│  ├─ platform/            #   board-support (BSP: AXP192, MPU6886, I2C) · adapters (ST7789 panel +
│  │                       #     generic PanelScreen, G37/G39 buttons + the AXP192 PEK power button,
│  │                       #     the LDO2 backlight, G2 LEDC buzzer, G0/G34 PDM mic, MPU6886 IMU) ·
│  │                       #     net (shared WiFi STA + DNS resolve)
│  └─ apps/                #   plant-monitor/{adapters, firmware-infra, bin} · host-monitor/{adapters, bin}
│                          #     · pomodoro/bin (+ the chime-selftest bench tool) · orientation/bin
└─ kb/                # Knowledge base — board facts, sources, findings (kbe-style)
```

The generic render loop is the keystone of the reuse. Its `Animated` contract carries a
coarse **`anchor`** (a plant `Observation`; a pomodoro `(phase, status)`) that resets the
creature's animation clock only on a *real* transition — so a pomodoro's `mm:ss` can tick
every second while its creature keeps animating on the phase's clock, and a healthy plant
reading stays a motionless creature the loop never repaints. Motion is spent only where it
buys the operator information.

Every crate in both workspaces carries one `[package.metadata.hex-arch] role` tag (and, in
the host workspace, a bounded `context` — `shared` for the platform, one per app otherwise).
`hex-lint` enforces the role matrix **and** context isolation on each commit via
`just precommit`; `effect-audit` holds the functional cores pure. Neither is advisory.

`kb/` is a `~/kbe`-style knowledge base for everything we learn
about this board — cited sources, on-device experiments, and the findings distilled from
them. It never compiles into the firmware. Its headline source is M5Stack's shipped
**FactoryTest** app ([`m5stack/M5StickC-Plus`](https://github.com/m5stack/M5StickC-Plus), a
pinned submodule) — the AXP192 / ST7789 bring-up we port into `firmware/`. Start at
[`kb/INDEX.md`](kb/INDEX.md). Fresh checkouts: `git submodule update --init`.

The two worlds build under **different toolchains**, on purpose: the host workspace on
**stable** rustc (`cargo test`), the firmware on the **`esp`** fork for
`xtensa-esp32-espidf`. `firmware/` is its own workspace (`[workspace]` + root `exclude`) so
its Xtensa target never touches `cargo test`; firmware crates reach the host crates by
**path** across the boundary.

### Looking at the screen without a board

`just screens` renders every state each app's TFT can show — the pomodoro's ready / focus /
break / paused / finished screens, the plant monitor's four `Observation` states, and the
orientation readout's poses in both the landscape and the portrait layout — to
`target/screens/*.png`. The pixels come from each app's `render`, the *same* function the
ST7789 adapter calls on the board, drawn into a host framebuffer instead of down an SPI bus.
It is the layout, not a picture of it — proven against real pixels by a shared test
framebuffer.

The right-hand region holds a **creature that is the status**. On the pomodoro it heads
down to code through a focus, bounces through a break, and winks when a phase completes; on
the plant monitor it breathes while healthy, is startled when the probe lies, and sleeps
when the sampler stops. The artwork is vendored from [ClaudePix](kb/sources/claudepix.md),
whose licence is **unresolved**; `just sprites` regenerates it with babashka,
`just sprite-screens` draws all 13 presets.

A host render proves the wording, the alignment, the colour each state is drawn in, and that
a short value erases the longer one it replaces. It proves **nothing** below the
`DrawTarget`: the panel's colour order, CGRAM offset, inversion and backlight are the
adapter's business, and a framebuffer paints red as red however the glass is wired. For the
red/blue order, `just run-bin display-colour-check` and look at the board.

## Prerequisites

- The `esp` rustc fork + Xtensa toolchain, via [`espup`](https://github.com/esp-rs/espup):
  `cargo install espup && espup install --targets esp32`. **No `~/export-esp.sh` to
  source** — `esp-idf-sys` self-provisions clang, xtensa-gcc, and a Python venv under
  `firmware/.embuild`. A *fresh* ESP-IDF bootstrap needs Python ≤ 3.12 plus `ninja` and
  `ldproxy` on `PATH` (the justfile handles the Python shim).
- [`espflash`](https://github.com/esp-rs/espflash) for flashing.

## Build & test

```sh
just test          # host — every app's domain + shell, stable rustc, no device
just build         # firmware — Xtensa std/ESP-IDF (both bins, release)
just screens       # render every app's screens to target/screens/*.png
just ci            # fmt + hex-lint + sprites + clippy (both worlds) + test + build
```

## Flash & run

Connect the board (appears as `/dev/ttyUSB0`), then flash the app you want:

```sh
just run-pomodoro       # the standalone pomodoro timer (screen + buttons + buzzer, offline)
just run-chime-selftest # play every jingle through the buzzer, hear it back on the PDM mic
just run                # the plant monitor  (a.k.a. `just flash`)
just run-host-monitor   # the homelab CPU/memory monitor (WiFi → hostpulse → per-host sparklines)
just run-orientation    # the IMU orientation readout (X/Y/Z bars + pitch/roll + resting face)
just run-plume          # the ambient feathered frond that breathes on the panel (screen only)
just monitor            # serial monitor only — pty-free (espflash --non-interactive)
```

The **host-monitor** needs the **hostpulse** endpoint reachable on the LAN (the control node,
`:9099` by default): a single read-only, bearer-gated `GET /pulse` that returns a ready-to-plot
per-host CPU/memory series for every host, having already done the PromQL `rate()` server-side.
Its `endpoint` (`host:port`) and `token` (a 64-hex bearer) go in `firmware/secrets.toml`'s
`[host_monitor]` table (git-ignored, alongside the WiFi credentials — see
`firmware/secrets.toml.example`; the token is never committed or logged). The board fetches the
frame every ~20 s and simply replaces its buffers — no on-device parsing or rate math — and
draws one row of CPU/memory sparklines per host, keeping the last good frame if the endpoint
faults or goes stale.

The pomodoro controls: **front button (G37) click** = start / pause / resume, **front
double-click** = restart the whole session, **front long hold** = reset the current phase, **side
button (G39) click** = skip to the next phase, **power button click** = light the glass or darken
it (a dark screen is not painted at all, so it costs no SPI traffic). Only the front button
reports double-clicks, and it is the only one that pays for them: telling a double-click from a
single one means waiting out a 300 ms window, so the side button's skip and the power button's
toggle stay immediate. Durations are the classic 25 / 5 / 15 min (long
break every 4th focus) — one constant in `pomodoro-core` to change, or shrink for a bench
test. The transition jingles are melodies in intent, but the tiny passive buzzer is not a
speaker: measured on-device it is loud across ~2–9 kHz yet radiates almost none of its energy at
the pitch it is driven, so it renders them as loud beeps told apart by rhythm and note count, not
tune — the notes are simply kept in that loud band. `just run-chime-selftest` proves the
audibility on-device — it plays every note through the buzzer while listening on the PDM mic and
logs each note's acoustic level against the silent floor, so audibility is falsifiable instead of
taken on faith. Serial traps (dialout group, the FT232 baud ceiling)
are in [`kb/guides/flashing-and-serial-access.md`](kb/guides/flashing-and-serial-access.md).

## Hardware wiring

Pin-exact in the KB ([`kb/guides/m5stickc-plus-board-reference.md`](kb/guides/m5stickc-plus-board-reference.md)):
the ST7789 TFT on SPI (SCLK 13 / MOSI 15 / CS 5 / DC 23 / RST 18), the front / side buttons
on **G37 / G39** (input-only, active-low), the passive buzzer on **G2** (LEDC PWM). The plant
probe is the M5 Earth Unit on **G33 (ADC1_CH5)** — ADC1 so it coexists with WiFi — and the
LED strip (project #3) is WS2812 on **G32**.

## Roadmap

Tracked in beads — `just ready` for unblocked work, `just triage` for graph-ranked
recommendations. Done: the platform carve-out and the standalone pomodoro timer (host-tested
FSM + on-device screen / buttons / buzzer); the plant monitor's WiFi, mDNS, ADC sampler, and
the Sensor entity served over the native-API host (verified host-first against the real HA
client; the on-device adoption pass awaits the board); the **host monitor** — WiFi promoted to
a shared `net` crate, the hostpulse `Pulse` frame + clamp/gap transform + JSON codec + a
board-generic sparkline, one row per host, all host-tested, cross-compiled to a linked Xtensa
image (the on-device run awaits a reachable hostpulse endpoint). Next up: the plant monitor's
Noise encryption + OTA, then the WS2812 driver and the rover on the same platform.

## License

The code here is dual-licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([`LICENSE-MIT`](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option. Each crate's `license` field is the source of truth — every first-party crate
carries this pair, with one deliberate exception: `platform/esphome-api` is **MIT-only**, because
its message types are vendored from [aioesphomeapi](https://github.com/esphome/aioesphomeapi)
(MIT); see its [`PROVENANCE.md`](platform/esphome-api/PROVENANCE.md).

Two things the dual licence does **not** cover:

- **The ClaudePix creature art** in `platform/platform-display`, vendored from
  [ClaudePix](kb/sources/claudepix.md): its licence is **unresolved**, so it is *not* offered
  under the terms above. `just sprites` regenerates it — resolve or replace it before any
  redistribution that needs clean provenance.
- **The `kb/sources/` submodules** (M5Stack's FactoryTest, aioesphomeapi, UbiHome's
  esphome-native-api): each keeps its own upstream licence.

Unless you state otherwise, any contribution you intentionally submit for inclusion in the work,
as defined in the Apache-2.0 licence, shall be dual licensed as above, without additional terms.
