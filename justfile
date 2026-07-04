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
# Runner is `espflash flash --monitor --baud 115200`; ESPFLASH_PORT names the
# port so espflash never prompts (which fails without a tty). Ctrl-C exits.
run:
    cd firmware && {{sg}} 'export PATH="{{fw_path}}:$PATH" ESPFLASH_PORT="{{port}}"; cargo run --release -p plant-monitor'

# `just flash` == `just run` (build + flash + monitor).
alias flash := run

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
ci: (fmt "check") hex-lint lint lint-fw test build

# Remove build artifacts (host + firmware).
clean:
    cargo clean
    cd firmware && cargo clean

# ---- Beads (issue tracking) ----
#
# Two tools over one dependency graph in .beads/. `br` (beads_rust) is the CRUD +
# git-facing store: a SQLite db mirrored to the committed .beads/issues.jsonl.
# `bv` reads that graph and ranks it (PageRank / betweenness) into robot-JSON
# triage. Division of labour: br writes & lists (human-readable), bv analyses &
# recommends (agent JSON — NEVER run bare `bv`, it opens a blocking TUI).
# br is non-invasive: it never runs git, so flushing the db to JSONL and
# committing it is our job — `just bead-sync`, then a `beads:`-scoped commit.
# The robot-JSON conforms to the schemas `br schema <target>` emits (and the
# upstream agent_baseline/schemas), so agents parse it deterministically.
# Full workflow: kb/guides/beads-triage.md.

# Ready work: open, unblocked, not deferred (br, human list).
ready:
    br ready

# Blocked work and what each waits on (br, human list).
blocked:
    br blocked

# Project counts by status / type / priority (br).
stats:
    br stats

# Top pick + its `br update … --status=in_progress` claim command (bv, robot JSON).
next:
    bv --robot-next

# Full triage — recommendations + blockers + graph health (bv, the mega-command).
triage:
    bv --robot-triage

# Parallel execution tracks: what can run concurrently right now (bv, robot JSON).
plan:
    bv --robot-plan

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
