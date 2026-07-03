# Plan: soil-moisture plant monitor → Home Assistant, on a std/ESP-IDF Rust foundation

## Context

An M5Stack **Earth Unit** (capacitive soil probe) is plugged into the M5StickC Plus Grove
port and sits in a plant. The goal: read soil moisture, graph it over time, and alert when to
water — with the readings flowing into **Home Assistant** via the **ESPHome native API**, so HA
does the storing, graphing and alerting.

This is the first of **three projects** the user will build on this board/ecosystem, which
reshaped the plan from "a feature" into "a shared foundation":

1. **This plant monitor** — soil moisture → HA dashboard + watering alerts. *Immediate.*
2. **NightDriverStrip-style WS2812 LED driver** for the workstation — the repo's original
   purpose; the existing `led-core` effects domain must survive.
3. **Robotics** — a controllable rover, "eventually flying, ArduPilot-style." *Future; diverges
   in hardware.*

**Decisions locked** (with the user):
- **Foundation pivot:** move the *firmware* from `no_std` esp-hal → **`std` on ESP-IDF**
  (`esp-idf-svc`/`esp-idf-hal`/`esp-idf-sys`). The device (ESP32-PICO-D4, 4 MB flash, 520 KB
  SRAM, **no PSRAM**) handles it comfortably. This is what makes the native API a ~week job
  instead of weeks: `std::net` TCP, `prost` protobuf, ESP-IDF WiFi/mDNS/OTA, `snow` for Noise.
  It **reverses** the documented no_std OTA epic (`stick-c-plus-qqh`) — ESP-IDF now supplies
  that stack natively.
- **Domain stays no_std.** `led-core` and the new `plant-core` remain pure, framework-free, and
  compile unchanged into a `std` firmware. Only firmware adapters + composition root change.
- **HA integration:** implement the ESPHome native API generically (see below). **Plaintext
  first**, Noise encryption as a fast-follow.
- **Sensor pin:** Earth **analog on G33 (ADC1_CH5)** — must be **ADC1** so it coexists with
  WiFi. Moisture % derived in software from raw counts; digital G32 line optional.
- **Display:** moisture status on the onboard **ST7789 TFT** (backlight gated by **AXP192** —
  power the PMIC *before* the display). Optional for the MVP.
- **Home Assistant is not yet running** — a step-zero stands it up.
- **Architecture:** hexagonal/ECB **+ Gary Bernhardt's *Boundaries* (Functional Core / Imperative
  Shell)**, **mechanically gated by your own `~/code/tools` contract suite** (`hex-lint` +
  `effect-audit`) — see the next two sections.

## Architecture: Functional Core / Imperative Shell (Boundaries)

Layer *Boundaries* on top of the hexagon — they compose. The hexagon says *where* dependencies
point (inward); Boundaries says *how* the core is shaped (pure values, no I/O). This also
operationalizes the CLAUDE.md rules: the core carries the branching and is property-tested
zero/one/many with **no mocks**; the shell is near-branchless.

- **Functional core** = the domain crates (`led-core`, `plant-core`) **and the protocol logic in
  `esphome-api`**. Pure, deterministic, no I/O, no time/network/hardware, no `&mut self`
  side-effects through ports. **Values in → values (or decision/command values) out.** *Many
  paths, few dependencies* → exhaustively **property-tested with no mocks**.
- **Imperative shell** = the composition root + firmware adapters + the accept/sample/flush loops.
  Performs all I/O (ADC, SPI/display, WiFi, TCP, mDNS, clock), pumps values through the core, and
  executes the commands the core returns. *Few paths, many dependencies*, kept at **cyclomatic
  complexity ~1** → covered by a few integration tests, not unit tests.

**The one real shape change from classic hexagonal:** the core does **not** call output ports
mid-computation — it **returns the effect as a value** and the shell performs it.
- `led-core`: `Animator` stops being generic over `LedOutput` / calling `out.write(…)`; `tick(t,
  buf)` **returns the rendered frame**; the shell's WS2812 adapter writes it. (Small refactor to
  existing code — folds into project #2's bead L.)
- `plant-core`: the sampler is a **pure step** — `step(prev, raw_readings, cal) -> (SamplerState,
  Option<Moisture>)` (new value + whether it changed); the caching / `Arc<Mutex>` / sleep live in
  the shell.
- `esphome-api`: the native-API connection is a **pure FSM** — `handle(conn, inbound, snapshot) ->
  (conn, Vec<OutboundMsg>)`. Sockets + Noise *transport* live in the shell. **Marquee win: the
  entire protocol is tested with values — no network, no mocks** (Bernhardt's parser-as-core
  example), dovetailing with the `aioesphomeapi` host-oracle testing already planned.

Input ports (`SoilSensor`, `Clock`) stay hexagonal seams the **shell** reads; their values are
passed into the core. This FC/IS discipline is a cross-cutting acceptance criterion on the domain
beads (C, D) and the `led-core` refactor (L): *core pure, shell complexity ~1*.

## Dogfood: your `~/code/tools` contract suite gates the architecture

The CLIs you're building in `~/code/tools` are static-analysis CI gates that enforce **exactly the
two principles above** — so dogfooding is the *mechanical enforcement* of the hexagon + FC/IS, not
a bolt-on. Both primary gates are **crate-granularity**, which is why the multi-crate workspace
matters (a monolith gets nothing; the role-tagged split lights them up). Wire into `just` +
pre-commit/CI, **advisory first → blocking**. Follow the suite's exit-code contract (**0 clean /
1 policy violation / 2 tool error** — a crash must never read as clean).

**Primary fits:**
- **`effect-audit` → enforces the Functional Core.** Tag `led-core`, `plant-core`, and the pure
  `esphome-api` protocol core `role="domain"`; run `effect-audit --require-domain <workspace>`. It
  fails the instant a clock/RNG/ADC handle/socket/`tokio`/mutable-static leaks into the core —
  keeping the sensor math + protocol FSM host-testable without hardware. `--require-domain` is the
  anti-false-green guard (audited-nothing = exit 2). *It's the least-productized of the six (2
  commits, no CI yet) but functionally complete — leaning on it here is a genuine feedback loop
  that hardens it.*
- **`hex-lint` → enforces the hexagon.** Tag every member with `[package.metadata.hex-arch] role=…`
  and gate; it fails any cross-role edge (ESPHome transport or ADC HAL sneaking into `domain`).
  Optional `context=…` adds bounded-context isolation. Roles:

  | Crate(s) | role | context |
  |---|---|---|
  | `led-core`, `plant-core`, `esphome-api` (pure core) | `domain` | led / plant / api |
  | `adapters` (adc, st7789, wifi, ws2812) | `driven-adapter` | — |
  | `firmware-infra` (socket server, mDNS, OTA), `board-support` | `driven-adapter` / `infra` | — |
  | `bins/*` | `composition-root` | — |

- **`cargo-regime-check` → freezes the reusable crates' public API.** Gate `led-core`,
  `plant-core`, `esphome-api` (`--workspace --base <ref>`, `cargo public-api` piped in) so a
  "just-a-refactor" PR is mechanically falsifiable. Overkill for leaf `bins/`.

**House style** (`~/code/tools/specs/CONVENTIONS.md`) — adopt for ecosystem consistency: stable
Rust, `#![forbid(unsafe_code)]` on our crates (deps keep their own unsafe; matches your "NO unsafe"
rule — the domain is trivially clean, the shell uses esp-idf's *safe* wrappers), clippy-deny-all,
Gherkin/cucumber specs, deterministic byte-stable output.

**Stretch / honest non-fits:**
- The three **JSON-wire-contract** tools (`contract-manifest`, `schema-regime-check`,
  `tolerant-reader-audit`, run via `contract-suite/suite-check.sh`) target **JSON Schema**; the
  native API is **protobuf** → **no fit on the device path.** They fit *iff* a JSON boundary
  appears (a host companion, or the alternate MQTT path: HA MQTT-discovery JSON + JSON config) —
  then mark reader structs `#[contract_manifest::reader]` so a firmware update never rejects a
  field a newer HA sends.
- **Gap in your toolset:** no `.proto`/protobuf regime-check (the JSON suite can't gate `api.proto`
  evolution) and no host-side timeseries/serial/dashboard tool (Home Assistant fills the dashboard
  role). A "regime-check for protobuf" is the natural missing sibling — out of scope now, worth a
  future-tool bead.

## The leverage: build the ESPHome native API *once, generically*

Rather than a one-off "soil sensor over the wire," the reusable asset is a **generic Rust
ESPHome-native-API entity framework** in a pure-`std`, host-testable crate. HA then becomes the
shared control/telemetry plane for all three projects:
- Plant → a `sensor` entity (+ HA automation for watering alerts).
- LED driver → a `light` entity (on/off, brightness, effect-select).
- Robot → `switch`/`number`/telemetry entities (non-realtime control/status).

Because the firmware is `std`, the **entire protocol** (framing, protobuf, connection state
machine, entity registry, even Noise) compiles and runs on the x86 host — validated against the
real `aioesphomeapi` client with **zero hardware**. Only WiFi/mDNS/OTA stay ESP-IDF-bound.

Entity model (in the new `esphome-api` crate):
```rust
pub trait ApiEntity: Send {
    fn key(&self) -> u32;                      // stable id (hash of object_id)
    fn object_id(&self) -> &str;
    fn list_message(&self) -> ListEntity;      // enum: Sensor|BinarySensor|Switch|Light|Number|Select|Cover
    fn state_message(&self) -> EntityState;    // enum: Sensor|Light|Switch|Number|Select
    fn handle_command(&mut self, cmd: &Command) {}   // no-op for read-only entities
}
pub struct Registry { entities: Vec<Box<dyn ApiEntity>> }
```
Implement `SensorEntity` now; a new entity type = new enum variant + prost message + `match` arm,
**no rewrite**. State flows via an `Arc<Mutex<Option<Moisture>>>` shared between a sampler thread
(writer) and the API server (reader) — the native-API adapter is a *driving/inbound* boundary.

### Don't write the protocol from scratch — vendor `UbiHome/esphome-native-api` (MIT)

Prior art exists and is permissively licensed. **`UbiHome/esphome-native-api`** (crates.io, MIT,
active) already implements the native-API **server** role, vendors `api.proto` with **prost**
types (version-gated features), and does Noise via **`noise-protocol` + `noise-rust-crypto`** —
pure-Rust, runtime-agnostic, **no_std/alloc-capable** (crucially *not* `snow`, so the handshake
runs in blocking code with no tokio). Its high-level server is WIP, and it's tokio-based, so we
**vendor/fork it** into our local `esphome-api` crate rather than depend on it, and take **Path B**:

- **Lift verbatim** (MIT): the vendored `api.proto` (or take it from `aioesphomeapi`, also MIT —
  **never** from `esphome/esphome`, which is **GPLv3**), the generated **prost** message types +
  the message-type-id registry, and the **Noise handshake** (`noise-protocol`/`noise-rust-crypto`
  need no runtime).
- **Rewrite thin** (~50 lines): replace their `tokio-util` frame codec with a blocking framed
  reader/writer over `std::io::{Read,Write}`.
- **Write ourselves:** the blocking `TcpListener` accept loop + connection state machine, the
  generic entity registry wired to our domain, and mDNS advertisement. Use `aioesphomeapi`'s
  `connection.py` as the handshake oracle and `yinzara/esphome-linux` as an entity-dispatch shape.

This keeps **tokio off the device** (heavy for 520 KB, no PSRAM), fits the hexagon (proto/Noise/
domain stay framework-free; the blocking net server is a boundary adapter), and still gives us
the proto + Noise for free — the two hardest pieces.

**Wire format** (to verify any implementation against): frame = **1-byte preamble + 2-byte
big-endian length + payload**; preamble `0x00` = plaintext (payload = varint type + varint len +
protobuf), `0x01` = encrypted (Noise blob whose *inner* frame = 2-byte type + 2-byte len +
protobuf). Cipher `Noise_NNpsk0_25519_ChaChaPoly_SHA256`, PSK = base64 32-byte HA key, **device =
responder**. TXT advertises `api_encryption=Noise_NNpsk0_25519_ChaChaPoly_SHA256` when on.

## Target workspace structure

Two workspaces kept (host crates build on stable rustc for `cargo test`; firmware builds on the
`esp` fork for `xtensa-esp32-espidf`). Cross-workspace **path deps**, exactly as `firmware`
already depends on `domain` today.

```
stick-c-plus/
  Cargo.toml                 # HOST workspace — members: domain, plant-core, esphome-api
  domain/       led-core     # (UNCHANGED) LED effects — project #2 domain
  plant-core/                # NEW  no_std, zero-dep — soil moisture domain
  esphome-api/               # NEW  std, host-testable — generic native-API framework (prost+snow+std::net)

  firmware/                  # FIRMWARE workspace (detached, esp toolchain)
    rust-toolchain.toml      #   channel = "esp"
    sdkconfig.defaults       # NEW
    board-support/           # NEW  BSP: AXP192 power-on, pin map, peripheral bring-up (all 3 projects)
    firmware-infra/          # NEW  WiFi STA + EspMdns + on-device native-API server host + OTA (all 3)
    adapters/                # NEW  domain-port adapters: adc.rs, st7789.rs, wifi.rs, ws2812.rs
    bins/plant-monitor/      # bin #1 — FIRST deliverable
    # bins/led-driver/       # bin #2 — led-core + ws2812 + Light entity  (later)
    # bins/rover/            # bin #3 — see divergence note              (later)
```

`led-core` is untouched; the existing `Ws2812Rmt` adapter re-homes from esp-hal RMT to
`esp-idf-hal` RMT (`TxRmtDriver` + `FixedLengthSignal`, same WS2812 timings) — that's project #2's
first bead, and the seam exists now. Each crate carries a `[package.metadata.hex-arch] role=…` tag
so `hex-lint` + `effect-audit` gate the layering (see **Dogfood**).

## Verified crate set (crates.io, 2026-07-03)

| Crate | Ver | Role | | Crate | Ver | Role |
|---|---|---|---|---|---|---|
| `esp-idf-svc` | 0.52 | WiFi, mDNS, log, nvs | | `esphome-native-api` | 2.1 | **vendor/fork (MIT)** — proto+prost+Noise |
| `esp-idf-hal` | 0.46 | ADC/SPI/I2C/RMT/GPIO | | `prost` | 0.13 | protobuf runtime (match UbiHome types) |
| `esp-idf-sys` | 0.37 | bindings + IDF build | | `noise-protocol` | 0.2 | Noise NNpsk0 (no_std-capable, **not** snow) |
| `embuild` | 0.33 | build.rs glue | | `noise-rust-crypto` | 0.6 | RustCrypto cipher backend for Noise |
| `ldproxy` | 0.3 | linker shim (binary) | | `mipidsi` / `embedded-graphics` | 0.10 / 0.8 | ST7789 / drawing |
| | | | | `axp192` | 0.2 | PMIC (eh 1.0) |

**ESP-IDF ≥ 5.3.0 required** (esp-idf-sys 0.37 dropped < 5.3). Target `xtensa-esp32-espidf`.
`UbiHome/esphome-native-api` is **vendored/forked** (MIT), not a crates.io dependency — we trim
tokio and swap its codec (see Path B above). `api.proto` is taken from `aioesphomeapi` (MIT).

## Work breakdown (beads — *what*, not *how*; the implementing agent designs the *how*)

New epic **"M5StickC Plus — std/ESP-IDF foundation + ESPHome-native plant monitor."**
`C` and `D` are **pure host work** — start immediately, no board time.

| # | Bead | Depends on |
|---|---|---|
| A | Migrate firmware to std ESP-IDF (target, esp-idf-svc/hal/sys, ldproxy, `sdkconfig.defaults`; LED still runs) | — |
| B | Restructure firmware into workspace (board-support + firmware-infra + adapters + bins) | A |
| C | `plant-core`: soil port + `Moisture` + calibration + sampler (host proptest + cucumber) | — |
| D | `esphome-api`: **vendor `UbiHome/esphome-native-api` (MIT)** — lift its api.proto+prost types+Noise, rewrite a blocking `std::net` codec+server, add generic entity registry (host-tested vs `aioesphomeapi`) | — |
| E | ADC adapter — `SoilSensor` on ADC1/G33, oversampling | A, C |
| F | AXP192 power-on + ST7789 display adapter (board-support) | A, B |
| G | WiFi STA bring-up (git-ignored creds) | A |
| H | mDNS `_esphomelib._tcp` advertiser | A, G |
| **I** | **On-device native-API server (plaintext), one Sensor entity, wired to sampler — end-to-end in HA** | D, E, G, H |
| J | Noise encryption follow-on (`snow` responder) | I |
| K | Host: stand up Home Assistant + adopt device | — |
| L | WS2812 adapter re-home to esp-idf RMT + `led-driver` bin exposing a **Light** entity (project #2 seed) | B, D |
| M | ESP-IDF-native OTA (esp-idf-svc OTA + HTTP; supersedes qqh no_std OTA) | G |
| N | KB: pin docs + write findings (below) | I |
| O | Dogfood gates: tag crates `role=…`; wire `hex-lint` + `effect-audit --require-domain` (+ `cargo-regime-check` on libs) into justfile + pre-commit/CI (advisory→blocking) | B |

Bead **I is the MVP milestone** — soil moisture graphing in HA over plaintext.

**Reconcile the old epic:** mark `stick-c-plus-qqh` and its no_std OTA children `qqh.2–qqh.6`
**superseded** by the new epic (keep for provenance, don't delete). **Keep `qqh.1`** (own the
WS2812 encoder — intent survives, substrate changes) and re-file under project #2. **Keep `5ww`**
(serial-under-conserver, still relevant until OTA lands). Add a *superseded* banner to
`kb/findings/esp-rs-ota-version-matrix.md`.

## Domain additions — `plant-core` (mirrors `led-core`'s shape & test discipline)

Reuse the existing patterns directly: port shape from `domain/src/ports.rs` (associated `Error`),
Control shape from `domain/src/animator.rs`, and the `Recorder`/`FixedClock` fake-adapter test
template from `animator.rs`'s inline tests.

- **`plant-core/src/ports.rs`** — driven port:
  `trait SoilSensor { type Error; fn read_raw(&mut self) -> Result<u16, Self::Error>; }` (0..=4095).
- **`plant-core/src/moisture.rs`** — entity `Moisture(u8)` (invariant 0..=100) + `Calibration
  { dry_raw, wet_raw }` + `to_percent(raw, cal)` using **integer-only** i32 math (no float/libm),
  clamped, branch-safe for either calibration order.
- **`plant-core/src/sampler.rs`** — Control as a **pure step** (FC/IS): `step(prev: SamplerState,
  raw: &[u16], cal: &Calibration) -> (SamplerState, Option<Moisture>)` — averages, calibrates,
  reports the new value + whether it changed. No sensor handle, no caching, no I/O; the shell
  reads the ADC and owns the `Arc<Mutex>` / interval.
- **Tests** (mirror `led-core`): inline `proptest` — result always 0..=100; `dry_raw`→0,
  `wet_raw`→100; monotonic; out-of-range clamps. `sampler.rs` unit tests with a `ScriptedSensor`
  fake. `plant-core/tests/features/moisture.feature` + `cucumber.rs` (`[[test]] harness=false`).
- Add `plant-core`, `esphome-api` to the root `Cargo.toml` `members`.

## Firmware pivot specifics (the load-bearing file changes)

- **`firmware/.cargo/config.toml`**: `target = "xtensa-esp32-espidf"`; `linker = "ldproxy"`;
  `build-std = ["std","panic_abort"]`; `[env]` `MCU=esp32`, `ESP_IDF_VERSION=v5.3.x` (verify exact
  tag), `ESP_LOG=info`. Runner stays `espflash flash --monitor`.
- **`firmware/*/build.rs`**: `embuild::espidf::sysenv::output();` (replaces the `-Tlinkall.x` line).
- **`firmware/sdkconfig.defaults`** (new): larger main-task + pthread stacks, `LWIP_MAX_SOCKETS`,
  `MDNS_MAX_SERVICES`, size-opt. Prefer per-thread `thread::Builder::stack_size(...)` for the API
  thread (~10–12 KB).
- **Cargo.toml**: add esp-idf-svc/hal/sys; drop esp-hal/esp-backtrace/esp-println.
- **Composition root** `bins/plant-monitor/src/main.rs`: standard `std` `fn main()` →
  `link_patches()`+logger → peripherals → **I2C up → AXP192 power-on (LDO2/LDO3)** → ADC1/G33 →
  (optional display) → WiFi connect → shared `Arc<Mutex<Option<Moisture>>>` → **sampler thread**
  → `Registry` with a `SensorEntity{name:"Soil Moisture", unit:"%", device_class:"moisture",
  state_class:"measurement"}` → EspMdns advertise → run native-API server.

**Adapters** (`firmware/adapters/`, each one file, re-exported from `mod.rs`):
- `adc.rs` — `SoilSensor` over `AdcDriver<ADC1>` + `AdcChannelDriver` on `gpio33`, attenuation
  `DB_12`, oversampled. `type Error = EspError`.
- `st7789.rs` + `board-support` AXP192 — power PMIC first (LDO2/LDO3), then ST7789 via `mipidsi`
  over `display-interface-spi` (MOSI G15, SCLK G13, DC G23, RST G18, CS G5); apply the
  M5StickC-Plus column/row **offset** + `INVON`. Share the one internal I2C bus via
  `embedded-hal-bus`.
- `wifi.rs` (in `firmware-infra`) — `EspWifi`/`BlockingWifi` client; creds from a **git-ignored**
  `firmware/secrets.toml` read in `build.rs` → `env!("WIFI_SSID"/"WIFI_PASS")`. Add
  `secrets.toml` + the Noise PSK to `.gitignore`. (SSID "REDACTED-WIFI-SSID" / pass "REDACTED-WIFI-PW"
  never enter committed source.)
- native-API — split **core vs shell** (FC/IS + `effect-audit`): the **pure core** in `esphome-api`
  (`role="domain"`, no sockets, tested with values) reuses the **vendored `UbiHome/esphome-native-api`
  (MIT)** `api.proto`+prost types + message-type-id registry, and holds the connection **FSM**
  (`handle(conn,msg,snapshot)->(conn,out)`) + framing/Noise *transforms*. The **shell** in
  `firmware-infra` runs the blocking `TcpListener` on `0.0.0.0:6053`, pumps bytes through the core
  (our ~50-line `std::io` frame codec, replacing their tokio-util one), and does mDNS — cap
  concurrency 1–2 conns (no PSRAM). Noise (bead J): reuse UbiHome's **`noise-protocol` +
  `noise-rust-crypto`** responder (no_std-capable, no tokio), `Noise_NNpsk0_25519_ChaChaPoly_SHA256`,
  PSK = base64 HA key, **device = responder**; feature-gate so plaintext builds omit it.

## Host side — stand up Home Assistant (step-zero, bead K)

No MQTT/Mosquitto needed (native-API path). Recommend **HAOS** on a mini-PC/Pi/VM; **HA
Container** (Docker) is fine too but must use **host networking**/macvlan so mDNS discovery works
on the same L2. Adoption: HA raises a discovery notification for `_esphomelib._tcp`, or add
manually via *Settings → Devices & Services → Add → ESPHome →* IP:6053 (plaintext: no key;
Noise: paste the same base64 32-byte key baked into firmware). Recorder stores + graphs the
sensor natively; watering alert = an automation on the moisture entity crossing a threshold.

## Save reference docs to the KB (user's explicit request — runs on execution)

Pin to `kb/sources/` (pinned commit/tag) and write `kb/findings/`:
- **Sources:** **`UbiHome/esphome-native-api`** (MIT — the crate we vendor) + its encryption doc
  (`ubihome.github.io/esphome-native-api/native_api/encryption/`); **`aioesphomeapi`** (MIT — pin
  its `api.proto` as our proto source **and** `connection.py` as the Noise/inner-frame oracle);
  `esphome/esphome` `components/api/` (**GPLv3 — behavior reference only, never copy**); ESPHome
  native-API protocol details (developers.esphome.io); HA ESPHome integration + Security/Noise
  pages; Rust-on-ESP (std) book + `esp-idf-template` + esp-idf-sys `BUILD-OPTIONS.md`;
  esp-idf-svc (wifi/mdns) + esp-idf-hal (adc/rmt/spi/i2c) docs; `prost`, `noise-protocol` +
  `noise-rust-crypto` + Noise NNpsk0 spec; `jasta/esp32-tokio-demo` (tokio-on-esp-idf caveat, if
  Path A ever needed); `mipidsi`/`embedded-graphics` + ST7789 datasheet; `axp192` + AXP192
  datasheet; M5StickC Plus schematic (have) + **Earth Unit** datasheet/wiki.
- **Findings:** std/ESP-IDF pivot supersedes the no_std OTA stack (cross-link
  `esp-rs-ota-version-matrix`); ESPHome native API framing + message-type-ID table + connection
  flow (plaintext vs Noise); ESPHome Noise params; Earth Unit = capacitive probe on ADC1_CH5/G33,
  ADC1-for-WiFi-coexistence + dry/wet calibration; M5StickC Plus std-ESP-IDF pin map.

## Verification (end-to-end)

1. **Host `cargo test`** (extend `just test` to `-p led-core -p plant-core -p esphome-api`):
   domain proptest + cucumber, **plus** `esphome-api` integration tests that bind the server on
   `127.0.0.1:0`, connect an in-process client, and assert the full flow + every entity's
   List/State frames. The whole protocol is validated on the dev host **before flashing**.
2. **Oracle — `aioesphomeapi`** (the exact client HA uses): `pip install aioesphomeapi`; connect,
   `device_info()`, `list_entities_services()`, `subscribe_states()` against (a) the host server
   and (b) the device — identical results prove conformance. Cross-check against a minimal
   reference **ESPHome-YAML** `sensor:` node (`esphome compile`) captured with the same client.
   Neither oracle needs HA running.
3. **On-device flash** via the existing justfile pattern (`sg dialout -c 'cargo run --release'`,
   `/dev/ttyUSB0`); `espflash flash --monitor` works for esp-idf ELFs — update the ELF path/vars
   to `target/xtensa-esp32-espidf/release/plant-monitor`.
4. **In HA:** the "Soil Moisture" entity appears, updates as the probe wets/dries, and graphs in
   History.
5. **Architecture gates (CI, dogfood):** `hex-lint` (role boundaries) and `effect-audit
   --require-domain` (core purity) pass clean; `cargo-regime-check --workspace` shows no undeclared
   public-API drift on the library crates. A red gate (exit 1) blocks the merge.

## Open items to confirm at implementation time (flagged, not blocking)

1. `noise-protocol` + `noise-rust-crypto` cross-compile on `xtensa-esp32-espidf` (pure-Rust, no
   ring — should be fine) — mitigated by plaintext-first. Confirm `blckngm/noise-rust`'s license
   is MIT-compatible before vendoring.
2. UbiHome server maturity: its high-level `EspHomeServer` accept-loop is WIP — confirm the
   generated prost `.rs` is committed (no build-time `protoc`) and budget to write the accept
   loop/state machine ourselves. Enable exactly **one** version feature to keep code size sane.
3. Exact mDNS managed-component wiring (`espressif/mdns`) for esp-idf-svc 0.52.
3. Exact `ESP_IDF_VERSION` v5.3.x tag esp-idf-sys 0.37 targets.
4. ST7789 M5StickC-Plus **offsets + inversion** in `mipidsi` 0.10 (the "renders shifted" trap).
5. Whether esp-idf-hal 0.46 `AdcChannelConfig` exposes ADC calibration on ESP32 (raw counts
   suffice regardless).
6. Noise inner-frame layout (type-id + length inside the encrypted payload) vs `aioesphomeapi`.
7. Current HA still accepts a **plaintext** native-API device (2026.x trends toward encryption) —
   confirm at HA stand-up.
8. Re-derive `api.proto` message IDs from the pinned commit at implementation time.

## Robotics divergence (flagged, not designed)

The shared foundation does **not** preclude a rover phase — a robot can expose
`cover`/`switch`/`number`/telemetry through the same registry, and drive-command ports fit the
hexagon. But the M5StickC Plus is **not a flight controller** (PICO-D4 + single MPU6886, no
mag/baro; HA is the wrong control path for flight). Keep telemetry/config on the native-API path;
keep any real-time control loop **local** to firmware; expect **different hardware** for the flight
phase. Don't wire the foundation to require HA in the control path.

## Critical files
- `firmware/Cargo.toml` (+ per-crate `.cargo/config.toml`, `build.rs`, new `sdkconfig.defaults`) — the std/ESP-IDF pivot.
- `esphome-api/` (new: `build.rs` protox subset, `codec.rs`, `connection.rs`, `entity.rs`, `server.rs`) — shared native-API framework.
- `plant-core/src/{ports.rs,moisture.rs,sampler.rs}` (new; mirrors `domain/src/ports.rs` + `animator.rs`) — soil domain.
- `firmware/adapters/{adc.rs,st7789.rs,wifi.rs,ws2812.rs}` + `firmware/board-support/` (AXP192) — esp-idf adapters.
- `firmware/bins/plant-monitor/src/main.rs` — composition root.
