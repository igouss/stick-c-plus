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
# Every `[script]` recipe runs under `bash -euo pipefail`: strict in ONE place instead of a
# `set -euo pipefail` line each recipe could forget. `hex-lint` opts out with its own shebang
# — it collects failures across both workspaces and must not die on the first one.
set script-interpreter := ["bash", "-euo", "pipefail"]
# Evaluate variables on use, not at startup. This is what lets the `require(...)` tool
# declarations below fail only in a recipe that actually needs the tool: without it a missing
# `bv` would break `just test`, and even `just --list`.
set lazy

port := "/dev/ttyUSB0"
chip := "esp32"
baud := "115200" # the board's FT232 is unreliable above this.
elf := "firmware/target/xtensa-esp32-espidf/release/plant-monitor"
# `sg dialout -c` grants the group espflash needs; the absolute path dodges the
# ast-grep `sg` alias, and `require` proves it is there before a recipe leans on it.
sg := require("/usr/bin/sg") + " dialout -c"
# Firmware toolchain PATH: python 3.12 (ESP-IDF bootstrap), cargo/rustup shims,
# and ninja (via linuxbrew). Prepended inside `sg`, which may reset the env.
pyshim := justfile_directory() / "firmware/tools/pyshim"
fw_path := pyshim + ":$HOME/.cargo/bin:/home/linuxbrew/.linuxbrew/bin"
# The aioesphomeapi conformance-oracle venv (git-ignored, survives `cargo clean`).
oracle_venv := justfile_directory() / ".oracle-venv"
# The hook socket, written ONCE: the daemon and the Claude Code hook binary must agree on this
# path or the hook talks to nobody, silently. $XDG_RUNTIME_DIR is what the hook binary resolves
# when Claude Code spawns it with no BUDDY_BRIDGE_SOCK; the temp dir is the fallback only when
# that is unset. A /tmp default here would miss the hook.
bridge_sock := "${BUDDY_BRIDGE_SOCK:-${XDG_RUNTIME_DIR:-/tmp}/buddy-bridge.sock}"

# ---- External tools ----
#
# Resolved by name at the moment a recipe needs one (see `set lazy`), so an absent tool fails
# as "could not find executable <name>" instead of a 127 surfacing several layers down. Not
# cosmetic: in `hex-lint` a missing binary landed in the `unexpected exit` branch and read as
# an architecture violation — a false red on the gate. These resolve against just's PATH; the
# device recipes re-exec under `sg` with {{fw_path}}, so for those this is a proxy check.
bb := require("bb")
br := require("br")
bv := require("bv")
hex_lint := require("hex-lint")
effect_audit := require("effect-audit")
espflash := require("espflash")
bluetoothctl := require("bluetoothctl")
jq := require("jq")
curl := require("curl")
size_bin := require("size")

# ---- The rest of the file, one area per file ----
#
# `import` and not `mod`: imported recipes join THIS namespace, so they keep using the
# variables and tools declared above by plain interpolation, `just ci` still names them
# directly, and every recipe keeps the spelling it has always had. A module would have put
# them behind `board::run` and reduced the shared variables to exported environment.
import 'just/arch.just'
import 'just/beads.just'
import 'just/board.just'
import 'just/bridge.just'
import 'just/host.just'

# List recipes.
default:
    @just --list

# `just flash` == `just run` (build + flash + monitor).
alias flash := run

# ---- Project meta ----

# Format all code (host + firmware); `just fmt check` to only verify.
[group('meta')]
[script]
fmt mode="write":
    cargo fmt --all {{ if mode == "check" { "--check" } else { "" } }}
    cd firmware && cargo fmt {{ if mode == "check" { "--check" } else { "" } }}
    # `just --fmt` only ever touches the file it was pointed at — an imported file is left
    # alone — so each one is formatted in its own right, or the split would quietly drop
    # four fifths of the justfile out of the formatting gate.
    just --fmt {{ if mode == "check" { "--check" } else { "" } }}
    for f in {{ justfile_directory() }}/just/*.just; do
      just --justfile "$f" --fmt {{ if mode == "check" { "--check" } else { "" } }}
    done

# Re-check the pinned esp-rs stack against crates.io latest.
[group('meta')]
[script]
versions:
    ua="stick-c-plus just versions (i.gouss@gmail.com)"
    for c in esp-idf-svc esp-idf-hal esp-idf-sys embuild ldproxy; do
      {{ curl }} -s -H "User-Agent: $ua" "https://crates.io/api/v1/crates/$c" \
        | {{ jq }} -r --arg n "$c" '.crate | "\($n): \(.max_stable_version)  (newest \(.newest_version))"'
    done

# The fast pre-commit gate: formatting + architecture, no compile (so it stays
# quick). This is what .githooks/pre-commit runs; the slow gates (clippy, tests,
# build) stay in `just ci`.
[doc('the fast pre-commit gate: formatting + architecture, no compile')]
[group('meta')]
precommit: (fmt "check") hex-lint apps-check

# Install the git hooks (points core.hooksPath at .githooks). Run once per clone.
# The pre-commit hook runs `just precommit` (fmt + hex-lint) before a commit.
[doc('install the git hooks (core.hooksPath -> .githooks); once per clone')]
[group('meta')]
setup-hooks:
    git config core.hooksPath .githooks
    @echo "hooks installed: .githooks/pre-commit → just precommit  (bypass: git commit -n)"

# The host leg of the gate: stable rustc, no device, no ESP-IDF.
[private]
host-gate: lint test

# The firmware leg: Xtensa, both ESP-IDF configurations. Deliberately serial inside — the two
# IDF builds share firmware/.embuild, so running them at once would race over one directory.
[private]
fw-gate: lint-fw lint-buddy build build-buddy

# The two legs at once. They share nothing: separate workspaces, separate target directories,
# so neither blocks the other on cargo's target-dir lock — which is the whole reason this can
# be parallel at all. A failing leg still aborts the run and propagates its exit code; the
# price is interleaved output while both are talking.
[parallel]
[private]
_gates: host-gate fw-gate

# Everything CI checks: format, architecture, lint both worlds, test, build. The cheap checks
# stay serial and FIRST, so an architecture breach or an undeclared app fails in seconds rather
# than behind two compilers; only the two expensive legs run together.
[doc('everything CI checks: fmt, architecture, lint both worlds, test, build')]
[group('meta')]
ci: (fmt "check") hex-lint apps-check sprites-check _gates

# Remove build artifacts (host + firmware). Confirmed because a firmware clean throws away
# the built ESP-IDF with it, and that costs minutes to put back. `just --yes clean` skips
# the prompt for a scripted caller.
[confirm('delete host AND firmware build artifacts? the ESP-IDF rebuild takes minutes.')]
[doc('remove build artifacts (host + firmware)')]
[group('meta')]
clean:
    cargo clean
    cd firmware && cargo clean

# ---- Knowledge base ----

# Verify the KB against itself: dangling cross-links, finding checks, script
# sanity. `just kb` runs the full gate; target one with `just kb links` /
# `checks` / `scripts`. Pure-read, no board. See kb/Justfile and kb/README.md.
[doc('verify the knowledge base against itself (links, checks, scripts)')]
[group('kb')]
kb *args:
    @just --justfile {{ justfile_directory() }}/kb/Justfile {{ args }}
