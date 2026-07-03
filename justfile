# stick-c-plus — task automation.
#
# Two build worlds: the host domain (`led-core`) compiles on stable rustc with no
# device, and the firmware is a detached Xtensa workspace that needs the `esp`
# toolchain (sourced from ~/export-esp.sh) plus dialout access to /dev/ttyUSB0.
# Device recipes go through `/usr/bin/sg` — on this host `sg` is shadowed by an
# ast-grep alias, and the espflash user lacks the dialout group in its login set.
# See kb/guides/flashing-and-serial-access.md and kb/guides/esp-rust-toolchain.md.

set shell := ["bash", "-uc"]

port := "/dev/ttyUSB0"
chip := "esp32"
elf  := "firmware/target/xtensa-esp32-none-elf/release/stick-led-firmware"
# `sg dialout -c` grants the group espflash needs; absolute path dodges the
# ast-grep `sg` alias. Baud stays default 115200 — this FT232 fails higher.
sg := "/usr/bin/sg dialout -c"

# List recipes.
default:
    @just --list

# ---- Host domain (led-core) — stable rustc, no device ----

# Run the domain suite: unit + property + cucumber.
test:
    cargo test -p led-core

# Lint the domain, warnings as errors.
lint:
    cargo clippy -p led-core -- -D warnings

# ---- Firmware (Xtensa, esp toolchain) ----

# Build the firmware (release).
build:
    source ~/export-esp.sh && cd firmware && cargo build --release

# Type-check the firmware without linking (fast).
check:
    source ~/export-esp.sh && cd firmware && cargo check --release

# Lint the firmware, warnings as errors.
lint-fw:
    source ~/export-esp.sh && cd firmware && cargo clippy --release -- -D warnings

# Report the firmware binary's section sizes (text/data/bss).
size: build
    source ~/export-esp.sh && xtensa-esp32-elf-size {{elf}}

# ---- Device (needs the board on {{port}}) ----

# Flash + monitor the firmware (espflash, via the cargo runner).
run:
    source ~/export-esp.sh && cd firmware && {{sg}} 'cargo run --release'

# Attach a serial monitor only, no flash. Ctrl-C to exit.
monitor:
    {{sg}} "script -qec 'espflash monitor -p {{port}} -c {{chip}}' /dev/null"

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
    for c in esp-hal esp-radio esp-rtos esp-storage esp-bootloader-esp-idf \
             esp-alloc esp-hal-smartled; do
      curl -s -H "User-Agent: $ua" "https://crates.io/api/v1/crates/$c" \
        | jq -r --arg n "$c" '.crate | "\($n): \(.max_stable_version)  (newest \(.newest_version))"'
    done

# Show ready-to-start beads (unblocked work).
ready:
    br ready

# Everything CI checks: format, lint both worlds, test, build.
ci: (fmt "check") lint lint-fw test build

# Remove build artifacts (host + firmware).
clean:
    cargo clean
    cd firmware && cargo clean
