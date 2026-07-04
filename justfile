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

# List recipes.
default:
    @just --list

# ---- Host domain (led-core) — stable rustc, no device ----

# Run the host domain suite (every host crate): unit + property + cucumber.
test:
    cargo test --workspace

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

# Install the git hooks (points core.hooksPath at .githooks). Run once per clone.
# The pre-commit hook runs `just hex-lint` (architecture gate) before a commit.
setup-hooks:
    git config core.hooksPath .githooks
    @echo "hooks installed: .githooks/pre-commit → just hex-lint  (bypass: git commit -n)"

# Show ready-to-start beads (unblocked work).
ready:
    br ready

# Everything CI checks: format, architecture (hex-lint), lint both worlds, test,
# build. hex-lint runs early — an architecture breach fails fast, before builds.
ci: (fmt "check") hex-lint lint lint-fw test build

# Remove build artifacts (host + firmware).
clean:
    cargo clean
    cd firmware && cargo clean
