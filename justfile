# stick-c-plus — task automation.
#
# Two build worlds: the host domain (`led-core`, plant-core, esphome-api) compiles
# on stable rustc with no device, and the firmware is a detached Xtensa workspace
# on the std/ESP-IDF stack (esp toolchain via firmware/rust-toolchain.toml).
# esp-idf-sys self-provisions clang + xtensa-gcc + a python 3.12 venv under
# firmware/.embuild, so no ~/export-esp.sh is needed — but a FRESH build's ESP-IDF
# bootstrap needs python <= 3.12 and this host's default python3 is 3.14, so the
# firmware recipes prepend firmware/tools/pyshim (python3 -> python3.12).
# Device recipes go through `/usr/bin/sg` — on this host `sg` is shadowed by an
# ast-grep alias, and the espflash user lacks the dialout group in its login set.
# See kb/guides/flashing-and-serial-access.md and kb/guides/esp-rust-toolchain.md.

set shell := ["bash", "-uc"]

port := "/dev/ttyUSB0"
chip := "esp32"
baud := "115200"   # the board's FT232 is unreliable above this.
elf  := "firmware/target/xtensa-esp32-espidf/release/plant-monitor"
# `sg dialout -c` grants the group espflash needs; absolute path dodges the
# ast-grep `sg` alias.
sg := "/usr/bin/sg dialout -c"
# Firmware toolchain PATH: python 3.12 (ESP-IDF bootstrap), cargo/rustup shims,
# and ninja (via linuxbrew). Prepended inside `sg`, which may reset the env.
pyshim  := justfile_directory() / "firmware/tools/pyshim"
fw_path := pyshim + ":$HOME/.cargo/bin:/home/linuxbrew/.linuxbrew/bin"
# The aioesphomeapi conformance-oracle venv (git-ignored, survives `cargo clean`).
oracle_venv := justfile_directory() / ".oracle-venv"

# List recipes.
default:
    @just --list

# ---- Host domain (led-core) — stable rustc, no device ----

# Run the host domain suite (every host crate): unit + property + cucumber.
test:
    cargo test --workspace

# Render every screen the TFT can show to target/screens/*.png — the four Observation
# states, and the RGB colour-check bands. The pixels come from `plant_display::render`,
# the SAME function the ST7789 adapter calls on the board, drawn into a host
# framebuffer instead of down an SPI bus. So this is the layout, not a picture of it.
#
# What it does NOT show: anything below the DrawTarget — the panel's colour order,
# CGRAM offset, inversion, or backlight. A framebuffer paints red as red however the
# glass is wired. For THAT question: `just run-bin display-colour-check`, and look.
screens:
    cargo run --quiet -p plant-display --example screenshots
    cargo run --quiet -p pomodoro-display --example screenshots
    cargo run --quiet -p host-display --example screenshots

# Regenerate platform-display/src/sprite/generated.rs from the vendored ClaudePix frames
# (babashka; no JS, no network). The generator re-hashes every preset and refuses to emit
# unless all 13 match the digests captured from the live site in a real browser — so a
# corrupted or silently-edited copy fails here, not on the glass. See
# kb/sources/claudepix.md.
sprites:
    bb platform/platform-display/gen/generate.clj

# Fail if generated.rs has drifted from gen/frames.json. Part of `just ci`.
sprites-check:
    bb platform/platform-display/gen/generate.clj --check

# Render every vendored sprite to target/screens/sprites.png — six frames sampled across
# each loop. The sprite unit tests cannot see: a transposed decode still paints a
# creature-shaped blob. This is how a human checks the creature is a creature.
sprite-screens:
    cargo run --quiet -p platform-display --example sprites

# Run the aioesphomeapi conformance oracles — the #[ignore]d tests that drive the
# REAL Home Assistant client (aioesphomeapi) against our device: the connection FSM
# (esphome-api) and the qhw.9 Soil Moisture `SensorDevice` served over the full
# accept loop (esphome-server). First run provisions a local venv at
# {{oracle_venv}} (needs network + a python with aioesphomeapi wheels); it survives
# `cargo clean`. NOT part of `just ci`: aioesphomeapi is not a Cargo dep, so a plain
# build never fakes it — the tests show *ignored*, never a false green — and this
# recipe is the one place that pins ESPHOME_ORACLE_PYTHON and un-ignores them.
oracle:
    #!/usr/bin/env bash
    set -euo pipefail
    venv="{{oracle_venv}}"
    py="$venv/bin/python"
    if ! "$py" -c "import aioesphomeapi" 2>/dev/null; then
      base="$(command -v python3.12 || command -v python3)"
      echo "▶ provisioning oracle venv at $venv (from $base)"
      "$base" -m venv "$venv"
      "$py" -m pip install --quiet --upgrade pip
      "$py" -m pip install --quiet aioesphomeapi
    fi
    echo "▶ oracle: esphome-api connection FSM"
    ESPHOME_ORACLE_PYTHON="$py" cargo test -p esphome-api --test aioesphomeapi_oracle -- --ignored --nocapture
    echo "▶ oracle: esphome-server SensorDevice over the server host (qhw.9)"
    ESPHOME_ORACLE_PYTHON="$py" cargo test -p esphome-server --test aioesphomeapi_oracle -- --ignored --nocapture

# Lint the host domain (every host crate), warnings as errors.
lint:
    cargo clippy --workspace -- -D warnings

# Enforce the functional-core / imperative-shell split on every crate marked
# `[package.metadata.hex-arch] role = "domain"` (led-core, plant-core, esphome-api).
# Fails if a concrete effect — socket, file, thread, clock — leaks into a domain
# core. (effect-audit's own CI wiring is qhw.25.)
audit:
    effect-audit --strict --require-domain .

# Enforce hexagonal role + bounded-context boundaries (hex-lint, ~/code/tools) on
# BOTH workspaces: a cross-role dependency edge (e.g. the ESPHome transport or the
# ADC HAL sneaking into a role=domain crate) fails the build. Suite exit contract
# is 0 clean · 1 policy violation · 2 tool error; ANY non-zero is fatal here — a
# tool error must never read as clean (that would be a false green). NB: hex-lint
# 0.2 reports tool errors (e.g. `cargo metadata failed`) as exit 1, so we fail red
# on 1 and 2 alike. The tree is clean, so this is blocking from day one (no
# advisory grace period needed); grandfather any future debt in
# hex-lint-exceptions.toml.
hex-lint:
    #!/usr/bin/env bash
    set -uo pipefail
    fail=0
    for ws in Cargo.toml firmware/Cargo.toml; do
      echo "── hex-lint $ws"
      if hex-lint --manifest-path "$ws"; then :; else
        rc=$?
        case $rc in
          1) echo "❌ hex-lint: policy violation or tool error in $ws [exit 1]" ;;
          2) echo "❌ hex-lint: TOOL ERROR in $ws [exit 2] — not clean" ;;
          *) echo "❌ hex-lint: unexpected exit $rc in $ws" ;;
        esac
        fail=1
      fi
    done
    [ "$fail" -eq 0 ] && echo "✅ hex-lint: both workspaces clean (roles + contexts)"
    exit "$fail"

# ---- Firmware (Xtensa, std/ESP-IDF) ----

# Build the firmware (release).
build:
    cd firmware && PATH="{{pyshim}}:$PATH" cargo build --release

# Build just the standalone pomodoro timer (Xtensa release). It links the shared ESP-IDF the
# workspace builds (root crate = plant-monitor); the pomodoro ELF drops the unused mdns
# symbols, so the flashed image is offline and clean. Set ESP_IDF_SYS_ROOT_CRATE=pomodoro to
# build a lean mdns-free IDF instead (a separate, slower first build).
build-pomodoro:
    cd firmware && PATH="{{pyshim}}:$PATH" cargo build --release -p pomodoro

# Build just the host monitor (Xtensa release), with a lean mdns-free ESP-IDF. Unlike
# `just build` (root crate = plant-monitor, which pulls the espressif/mdns managed
# component), setting ESP_IDF_SYS_ROOT_CRATE=host-monitor builds an IDF from host-monitor's
# own deps — no mdns, which this app never uses. A separate (slower first) IDF build.
build-host-monitor:
    cd firmware && PATH="{{pyshim}}:$PATH" ESP_IDF_SYS_ROOT_CRATE=host-monitor cargo build --release -p host-monitor

# Type-check the firmware without linking (fast).
check:
    cd firmware && PATH="{{pyshim}}:$PATH" cargo check --release

# Lint the firmware, warnings as errors.
lint-fw:
    cd firmware && PATH="{{pyshim}}:$PATH" cargo clippy --release -- -D warnings

# Report the firmware binary's section sizes (text/data/bss).
size: build
    size {{elf}}

# ---- Device (needs the board on {{port}}) ----

# Build, flash, and monitor the firmware (the qhw.1 board session, automated).
# Runner is `espflash flash --monitor --non-interactive --baud 115200`;
# ESPFLASH_PORT names the port so espflash never prompts, and --non-interactive
# streams serial without a controlling TTY, so this works from a pipe/CI/agent
# (no crossterm input reader to fail). Ctrl-C exits.
run:
    cd firmware && {{sg}} 'export PATH="{{fw_path}}:$PATH" ESPFLASH_PORT="{{port}}"; cargo run --release -p plant-monitor'

# `just flash` == `just run` (build + flash + monitor).
alias flash := run

# Flash and monitor one named bin of the plant-monitor package — the bench tools
# that live beside the monitor itself. `just run-bin probe-rail-check` measures
# whether the AXP192's EXTEN bit gates the Grove 5 V probe rail (qhw.31); see
# kb/experiments/2026-07-08-probe-rail-gating/. Leaves that bin on the board —
# `just run` puts the monitor back.
run-bin bin:
    cd firmware && {{sg}} 'export PATH="{{fw_path}}:$PATH" ESPFLASH_PORT="{{port}}"; cargo run --release -p plant-monitor --bin {{bin}}'

# Build, flash, and monitor the standalone pomodoro timer. `just run` puts the plant monitor
# back. Front tap = start/pause, front double-tap = restart session, front hold = reset, side
# tap = skip.
run-pomodoro:
    cd firmware && {{sg}} 'export PATH="{{fw_path}}:$PATH" ESPFLASH_PORT="{{port}}"; cargo run --release -p pomodoro'

# Build, flash, and monitor the Fedora host monitor. Scrapes the node_exporter at the
# [host_monitor] address in firmware/secrets.toml and draws two live CPU/memory sparklines.
# ESP_IDF_SYS_ROOT_CRATE=host-monitor builds the lean mdns-free IDF (see build-host-monitor).
# `just run` puts the plant monitor back.
run-host-monitor:
    cd firmware && {{sg}} 'export PATH="{{fw_path}}:$PATH" ESPFLASH_PORT="{{port}}" ESP_IDF_SYS_ROOT_CRATE=host-monitor; cargo run --release -p host-monitor'

# Flash and monitor the chime self-test: it plays every jingle note on the buzzer while
# listening on the PDM mic, and logs each note's acoustic level vs. the silent floor + PASS/FAIL
# — so "the chime is audible" is checked on-device, not by ear. (The tiny buzzer is a resonant
# transducer, so the test measures loudness above the floor, not pitch.) Read the floor vs. note
# levels and, if needed, calibrate MARGIN/MIN_LEVEL in src/chime_selftest.rs. `just run-pomodoro`
# puts the timer back.
run-chime-selftest:
    cd firmware && {{sg}} 'export PATH="{{fw_path}}:$PATH" ESPFLASH_PORT="{{port}}"; cargo run --release -p pomodoro --bin chime-selftest'

# Attach a serial monitor only, no flash. Ctrl-C to exit. `--non-interactive`
# skips espflash's crossterm input reader, so no controlling TTY (and no
# `script` pty shim) is needed — it streams serial straight to stdout, which is
# what a piped / CI / agent invocation wants. Reset-on-connect still yields a
# fresh boot; you forgo only the interactive Ctrl-R chip reset.
monitor:
    {{sg}} 'espflash monitor -p {{port}} -c {{chip}} --non-interactive'

# Print board / flash info over serial.
board-info:
    {{sg}} 'espflash board-info -p {{port}} -c {{chip}}'

# ---- Project meta ----

# Format all code (host + firmware); `just fmt check` to only verify.
fmt mode="write":
    cargo fmt --all {{ if mode == "check" { "--check" } else { "" } }}
    cd firmware && cargo fmt {{ if mode == "check" { "--check" } else { "" } }}

# Re-check the pinned esp-rs stack against crates.io latest.
versions:
    #!/usr/bin/env bash
    set -euo pipefail
    ua="stick-c-plus just versions (i.gouss@gmail.com)"
    for c in esp-idf-svc esp-idf-hal esp-idf-sys embuild ldproxy; do
      curl -s -H "User-Agent: $ua" "https://crates.io/api/v1/crates/$c" \
        | jq -r --arg n "$c" '.crate | "\($n): \(.max_stable_version)  (newest \(.newest_version))"'
    done

# The fast pre-commit gate: formatting + architecture, no compile (so it stays
# quick). This is what .githooks/pre-commit runs; the slow gates (clippy, tests,
# build) stay in `just ci`.
precommit: (fmt "check") hex-lint

# Install the git hooks (points core.hooksPath at .githooks). Run once per clone.
# The pre-commit hook runs `just precommit` (fmt + hex-lint) before a commit.
setup-hooks:
    git config core.hooksPath .githooks
    @echo "hooks installed: .githooks/pre-commit → just precommit  (bypass: git commit -n)"

# Everything CI checks: format, architecture (hex-lint), lint both worlds, test,
# build. hex-lint runs early — an architecture breach fails fast, before builds.
ci: (fmt "check") hex-lint sprites-check lint lint-fw test build

# Remove build artifacts (host + firmware).
clean:
    cargo clean
    cd firmware && cargo clean

# ---- Knowledge base ----

# Verify the KB against itself: dangling cross-links, finding checks, script
# sanity. `just kb` runs the full gate; target one with `just kb links` /
# `checks` / `scripts`. Pure-read, no board. See kb/Justfile and kb/README.md.
kb *args:
    @just --justfile {{justfile_directory()}}/kb/Justfile {{args}}

# ---- Beads (issue tracking) ----
#
# Two tools over one dependency graph in .beads/. `br` (github.com/gastownhall/beads)
# is the CRUD + git-facing store: a SQLite db mirrored to the committed
# .beads/issues.jsonl. `bv` reads that graph and ranks it (PageRank / betweenness)
# into robot-JSON triage. Division of labour: br writes & lists, bv analyses &
# recommends. br is non-invasive: it never runs git, so flushing the db to JSONL and
# committing it is our job — `just bead-sync`, then a `beads:`-scoped commit.
#
# NON-INTERACTIVE CONTRACT (every recipe here is safe to script / run headless):
#   - `br` prints and exits — no pager, no prompt, no TTY needed. `--json` gives a
#     stable machine shape; the human recipes below rely on the plain-text output.
#   - `bv` is a TUI FIRST: bare `bv` opens a full-screen viewer that BLOCKS the
#     terminal (and any agent session). So `bv` is invoked ONLY with a `--robot-*`
#     flag, which makes it emit-and-exit. NEVER script bare `bv`.
#   - `-f json` PINS the output format: `bv`'s robot mode defaults to JSON but honours
#     BV_OUTPUT_FORMAT / TOON_DEFAULT_FORMAT, so pinning it keeps the shape
#     deterministic no matter the environment. Robot-JSON conforms to the schemas
#     `br schema <target>` emits.
# Full workflow: kb/guides/beads-triage.md.

# Ready work: open, unblocked, not deferred (br, human list — prints and exits).
ready:
    br ready

# Blocked work and what each waits on (br, human list — prints and exits).
blocked:
    br blocked

# Project counts by status / type / priority (br — prints and exits).
stats:
    br stats

# Top pick + its `br update … --status=in_progress` claim command (bv robot JSON).
next:
    bv --robot-next -f json

# Full triage — recommendations + blockers + graph health (bv, the mega-command).
triage:
    bv --robot-triage -f json

# Parallel execution tracks: what can run concurrently right now (bv robot JSON).
plan:
    bv --robot-plan -f json

# Flush the db → the committed .beads/issues.jsonl so beads changes can land.
# br is non-invasive (never runs git), so stage + commit the JSONL yourself —
# the recipe prints the exact command.
bead-sync:
    br sync --flush-only
    @echo "flushed → .beads/issues.jsonl  ·  commit: git add .beads/issues.jsonl && git commit -m 'beads: …'"

# Graph-health gate. bv's insight metrics are lazily computed and report
# Cycles:null when uncomputed (a false red), so the cycle count comes from
# `br dep cycles` instead. Runs br diagnostics + asserts zero cycles; else fails.
bead-check:
    #!/usr/bin/env bash
    set -euo pipefail
    br doctor
    if br dep cycles --json | jq -e '.count == 0' >/dev/null; then
      echo "✅ beads: no dependency cycles"
    else
      br dep cycles
      echo "❌ beads: dependency cycle(s) — break one edge with: br dep remove <child> <parent>"
      exit 1
    fi
