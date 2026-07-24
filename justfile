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
baud := "115200"   # the board's FT232 is unreliable above this.
elf  := "firmware/target/xtensa-esp32-espidf/release/plant-monitor"
# `sg dialout -c` grants the group espflash needs; the absolute path dodges the
# ast-grep `sg` alias, and `require` proves it is there before a recipe leans on it.
sg := require("/usr/bin/sg") + " dialout -c"
# Firmware toolchain PATH: python 3.12 (ESP-IDF bootstrap), cargo/rustup shims,
# and ninja (via linuxbrew). Prepended inside `sg`, which may reset the env.
pyshim  := justfile_directory() / "firmware/tools/pyshim"
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
bb           := require("bb")
br           := require("br")
bv           := require("bv")
hex_lint     := require("hex-lint")
effect_audit := require("effect-audit")
espflash     := require("espflash")
bluetoothctl := require("bluetoothctl")
jq           := require("jq")
curl         := require("curl")
size_bin     := require("size")

# List recipes.
default:
    @just --list

# ---- Host domain (led-core) — stable rustc, no device ----

# Run the host domain suite (every host crate): unit + property + cucumber.
[group('host')]
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
[doc('render every screen to target/screens/*.png — the layout, not the glass')]
[group('host')]
screens:
    cargo run --quiet -p plant-display --example plant-screenshots
    cargo run --quiet -p pomodoro-display --example pomodoro-screenshots
    cargo run --quiet -p host-display --example host-screenshots
    cargo run --quiet -p orientation-display --example orientation-screenshots
    cargo run --quiet -p plume-display --example plume-screenshots
    cargo run --quiet -p buddy-display --example buddy-screenshots

# Re-bless the golden screens from the current render — host-monitor and the buddy. The
# `goldens` tests (part of `just test`) render every state and fail if the picture drifts
# from the committed PNGs in each crate's goldens/; run this after an INTENTIONAL
# layout/colour change to accept the new look, then commit the goldens. Both render through
# the same function the test checks, so a blessed golden and a checked render match.
[doc('accept the current render as the new golden screens')]
[group('host')]
screens-bless:
    BLESS_GOLDENS=1 cargo test -p host-display --test goldens
    BLESS_GOLDENS=1 cargo test -p buddy-display --test goldens

# Regenerate platform-display/src/sprite/generated.rs from the vendored ClaudePix frames
# (babashka; no JS, no network). The generator re-hashes every preset and refuses to emit
# unless all 13 match the digests captured from the live site in a real browser — so a
# corrupted or silently-edited copy fails here, not on the glass. See
# kb/sources/claudepix.md.
[doc('regenerate the vendored ClaudePix sprite tables (babashka)')]
[group('host')]
sprites:
    {{bb}} platform/platform-display/gen/generate.clj

# Fail if generated.rs has drifted from gen/frames.json. Part of `just ci`.
[group('host')]
sprites-check:
    {{bb}} platform/platform-display/gen/generate.clj --check

# Render every vendored sprite to target/screens/sprites.png — six frames sampled across
# each loop. The sprite unit tests cannot see: a transposed decode still paints a
# creature-shaped blob. This is how a human checks the creature is a creature.
[doc('render every vendored sprite to target/screens/sprites.png')]
[group('host')]
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
[doc('run the aioesphomeapi conformance oracles (the real Home Assistant client)')]
[group('host')]
[script]
oracle:
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
[group('host')]
lint:
    cargo clippy --workspace -- -D warnings

# ---- Bridge (the Linux BLE central — needs BlueZ; the device test needs the stick) ----

# THE one command: run the bridge daemon on the socket the Claude Code hook resolves. It finds
# the stick from cold, bonds if it has to, and then reconnects by itself forever — across the
# stick's reboots, yours, and any trip out of range. Bond once; after that this never asks you
# anything. Flash the peer first with `just run-buddy`. RUST_LOG=debug for more; Ctrl-C to stop.
#
# The signal to watch for is `link up: bonded`, NOT the passkey prompt — a passkey prompt on a
# stick you have already bonded means the bond was lost, which `just bridge-pair` re-establishes.
[doc('run the bridge daemon on the socket the Claude Code hook resolves')]
[group('bridge')]
[script]
bridge: bridge-preflight
    sock="{{bridge_sock}}"
    echo "▶ bridge ↔ claude-buddy, hook socket $sock"
    echo "  waiting for the stick; it is picked up whenever it appears. Ctrl-C to stop."
    RUST_LOG="${RUST_LOG:-info}" BUDDY_BRIDGE_SOCK="$sock" cargo run --release -p buddy-bridge

# Everything that must be true before the daemon can work, asserted rather than assumed. Each
# check exists because its absence produced a silent hang or a mystery failure, not a message.
[doc('assert everything the bridge daemon needs, before it needs it')]
[group('bridge')]
[script]
[no-exit-message]
bridge-preflight:
    if ! ls /sys/class/bluetooth/hci* >/dev/null 2>&1; then
      echo "❌ no BlueZ adapter under /sys/class/bluetooth — is Bluetooth up?"; exit 2
    fi
    # A soft-blocked adapter accepts every D-Bus call and simply never sees an advertisement:
    # the daemon scans forever and BlueZ reports no error at all. rfkill is deliberately NOT a
    # `require`d tool — not every host ships it — so an absent one skips this check instead of
    # failing the recipe.
    if command -v rfkill >/dev/null 2>&1 && rfkill list bluetooth 2>/dev/null | grep -q 'blocked: yes'; then
      echo "❌ bluetooth is rfkill-blocked — 'rfkill unblock bluetooth' first"; exit 2
    fi
    # A daemon already holding the hook socket wins the bind; a second one dies on it, or worse,
    # the two fight over the adapter and neither keeps a link.
    sock="{{bridge_sock}}"
    if pgrep -x buddy-bridge >/dev/null 2>&1; then
      echo "❌ a buddy-bridge is already running (socket $sock) — stop it first: pkill -x buddy-bridge"; exit 2
    fi
    echo "✅ preflight: adapter present, radio unblocked, no bridge already running"

# Deliberately re-bond: forget BOTH halves of the bond, then bring the daemon up so a fresh
# pairing happens now, while you are looking at the glass. This is the ONLY recipe that should
# ever ask you for six digits — `just bridge` is the everyday command, and a bond survives
# reboots on both sides. Reach for this when a passkey prompt appears where none should.
# Inherits `bridge-forget`'s confirmation; `just --yes bridge-pair` skips it.
[doc('deliberately re-bond: forget both halves, then pair afresh')]
[group('bridge')]
bridge-pair: bridge-forget bridge

# Forget the HOST half of an out-of-sync bond to the Claude-XXXX stick. The DEVICE half lives in
# the stick's NVS and survives a reflash — this prints the erase command to clear it too. See
# kb/guides/buddy-permission-hook.md.
#
# Removing the device from BlueZ is now safe: it evicts the cache entry, and the daemon's
# discovery finds an advertising stick with no cache entry (which is what the old one could not
# do — that is why this recipe used to leave the stick undiscoverable). Confirmed because the
# way back is a trip to the glass to read six digits.
[doc('forget the host half of the Claude-XXXX bond')]
[group('bridge')]
[confirm('drop the host half of the Claude-XXXX bond? re-bonding needs the passkey on the glass.')]
[script]
bridge-forget:
    mapfile -t addrs < <({{bluetoothctl}} devices 2>/dev/null | awk '/Claude-/{print $2}')
    if [ "${#addrs[@]}" -eq 0 ]; then echo "no Claude- bond on the host to forget"; else
      for a in "${addrs[@]}"; do echo "forgetting $a"; {{bluetoothctl}} remove "$a" >/dev/null || true; done
      echo "✅ host bond(s) cleared."
    fi
    echo "the DEVICE half persists across reflash — erase the stick's NVS to fully reset the bond:"
    echo "  {{sg}} 'espflash erase-region -p {{port}} -c esp32 0x9000 0x6000'"

# The device-in-the-loop proof for the bridge: the #[ignore]d test that drives the REAL
# BluerCentral against a flashed Claude-XXXX stick — bond, heartbeat, chunked round-trip,
# reconnect-across-reboot, and the Just-Works-downgrade regression (the passkey callback must
# fire). Like `oracle`, it is #[ignore]d so a plain `cargo test` shows it *ignored*, never a
# false green, and it is deliberately NOT in `just ci` (a bond needs the physical device).
# Preflight asserts a BlueZ adapter is present; set STICK_PASSKEY to the six digits the glass
# shows (the firmware draws a fresh one per pairing — there is no constant to fall back on).
[doc('device-in-the-loop bridge test against a flashed stick')]
[group('bridge')]
[script]
bridge-device: bridge-preflight
    echo "▶ bridge device test — flash the peer first: just run-buddy"
    echo "  you will be asked to enter the passkey shown on the glass, and to power-cycle the stick."
    cargo test -p buddy-bridge-shell --test device_bridge -- --ignored --nocapture

# Enforce the functional-core / imperative-shell split on every crate marked
# `[package.metadata.hex-arch] role = "domain"` (led-core, plant-core, esphome-api).
# Fails if a concrete effect — socket, file, thread, clock — leaks into a domain
# core. (effect-audit's own CI wiring is qhw.25.)
[doc('enforce the functional-core / imperative-shell split on domain crates')]
[group('architecture')]
audit:
    {{effect_audit}} --strict --require-domain .

# Enforce hexagonal role + bounded-context boundaries (hex-lint, ~/code/tools) on
# BOTH workspaces: a cross-role dependency edge (e.g. the ESPHome transport or the
# ADC HAL sneaking into a role=domain crate) fails the build. Suite exit contract
# is 0 clean · 1 policy violation · 2 tool error; ANY non-zero is fatal here — a
# tool error must never read as clean (that would be a false green). NB: hex-lint
# 0.2 reports tool errors (e.g. `cargo metadata failed`) as exit 1, so we fail red
# on 1 and 2 alike. The binary itself is `require`d, so an ABSENT hex-lint now says
# so instead of arriving here as exit 127 and reading as a policy breach. The tree
# is clean, so this is blocking from day one (no advisory grace period needed);
# grandfather any future debt in hex-lint-exceptions.toml.
[doc('enforce hexagonal role + context boundaries on both workspaces')]
[group('architecture')]
[no-exit-message]
hex-lint:
    #!/usr/bin/env bash
    # Deliberately NOT the project-wide `bash -euo pipefail`: this recipe runs on past the
    # first failing workspace so BOTH are reported, then fails once at the end.
    set -uo pipefail
    fail=0
    for ws in Cargo.toml firmware/Cargo.toml; do
      echo "── hex-lint $ws"
      if {{hex_lint}} --manifest-path "$ws"; then :; else
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
#
# Six apps, ONE build engine and ONE flash engine. Everything that differs between them — the
# ESP-IDF root crate, any layered sdkconfig — is declared beside the app itself, in
# firmware/apps/<app>/bin/Cargo.toml under [package.metadata.board], never here. Adding an app
# means writing that table; the recipes below then build, lint and flash it with no edit to
# this file. `just apps` lists what is declared. The named recipes that follow are wrappers
# kept for the fingers that already know them.

# The build env for one app, one KEY=VALUE per line. ESP_IDF_SYS_ROOT_CRATE is emitted for
# EVERY app, including the four that merely repeat the workspace default from
# firmware/.cargo/config.toml: one rule, no silent defaults, and each app states which ESP-IDF
# it links. ESP_IDF_SDKCONFIG_DEFAULTS appears only where the app layers Kconfig of its own.
[private]
[working-directory: 'firmware']
_app-env app:
    @cargo metadata --no-deps --format-version 1 | {{jq}} -r --arg a '{{app}}' '\
        [.packages[] | select(.name == $a) | .metadata.board]                                  \
        | if length == 0 then                                                                  \
            error("\($a): no [package.metadata.board] — declare it in the app bin/Cargo.toml") \
          else .[0] end                                                                        \
        | ["ESP_IDF_SYS_ROOT_CRATE=" + .["idf-root-crate"]]                                    \
          + (if .sdkconfig then ["ESP_IDF_SDKCONFIG_DEFAULTS=" + (.sdkconfig|join(";"))] else [] end) \
        | .[]'

# The apps that CANNOT join the shared workspace build, derived rather than listed: one
# `cargo build --workspace` builds exactly one ESP-IDF, so an app that layers its own
# sdkconfig — i.e. needs a differently CONFIGURED IDF — cannot be in it. Today that is
# claude-buddy alone, because esp32-nimble does not compile at all against a BT-disabled IDF
# (655 unresolved NimBLE symbols, not a warning). Its own recipes build it; `just ci` runs both.
[private]
[working-directory: 'firmware']
_workspace-excludes:
    @cargo metadata --no-deps --format-version 1 | {{jq}} -r '.packages[] | select(.metadata.board.sdkconfig) | "--exclude", .name'

# The flashable apps and what each one is, from the metadata itself — so this list cannot go
# stale the way a hand-kept one does.
[doc('list the flashable apps and what each one is')]
[group('firmware')]
[working-directory: 'firmware']
apps:
    @cargo metadata --no-deps --format-version 1 | {{jq}} -r '.packages[] | select(.metadata.board) | "\(.name)\n    \(.metadata.board.summary)"'

# Build ONE app by name, with the ESP-IDF configuration it declares.
[doc('build one app by name (Xtensa release), with its declared ESP-IDF config')]
[group('firmware')]
[working-directory: 'firmware']
[script]
fw-build app:
    mapfile -t appenv < <(just --justfile {{justfile()}} _app-env {{app}})
    PATH="{{pyshim}}:$PATH" env "${appenv[@]}" cargo build --release -p {{app}}

# Lint ONE app by name against the ESP-IDF configuration it declares, warnings as errors.
[doc('lint one app by name (Xtensa release), warnings as errors')]
[group('firmware')]
[working-directory: 'firmware']
[script]
fw-lint app:
    mapfile -t appenv < <(just --justfile {{justfile()}} _app-env {{app}})
    PATH="{{pyshim}}:$PATH" env "${appenv[@]}" cargo clippy --release -p {{app}} -- -D warnings

# Build the firmware (release).
#
# Excludes the apps that need their own IDF configuration — see `_workspace-excludes`, which
# derives that list from the app metadata instead of naming claude-buddy here.
[doc('build the firmware, release (apps needing their own IDF config excluded)')]
[group('firmware')]
[working-directory: 'firmware']
[script]
build:
    mapfile -t excl < <(just --justfile {{justfile()}} _workspace-excludes)
    PATH="{{pyshim}}:$PATH" cargo build --release --workspace "${excl[@]}"

# Build just the standalone pomodoro timer (Xtensa release). It links the shared ESP-IDF the
# workspace builds (root crate = plant-monitor); the pomodoro ELF drops the unused mdns
# symbols, so the flashed image is offline and clean. Point its `idf-root-crate` at itself to
# build a lean mdns-free IDF instead (a separate, slower first build).
[doc('build just the pomodoro timer (Xtensa release)')]
[group('firmware')]
build-pomodoro: (fw-build "pomodoro")

# Build just the orientation readout (Xtensa release). Like the pomodoro timer it links the
# shared ESP-IDF the workspace builds (root crate = plant-monitor); the orientation ELF drops
# the unused mdns symbols, so the flashed image is offline and clean.
[doc('build just the orientation readout (Xtensa release)')]
[group('firmware')]
build-orientation: (fw-build "orientation")

# Build just the plume (Xtensa release). Like the orientation readout it links the shared
# ESP-IDF the workspace builds (root crate = plant-monitor); with no network the plume ELF
# drops the unused mdns symbols, so the flashed image is offline and clean.
[doc('build just the plume (Xtensa release)')]
[group('firmware')]
build-plume: (fw-build "plume")

# Build just the host monitor (Xtensa release), with a lean mdns-free ESP-IDF. Unlike
# `just build` (root crate = plant-monitor, which pulls the espressif/mdns managed
# component), its `idf-root-crate = "host-monitor"` builds an IDF from host-monitor's own
# deps — no mdns, which this app never uses. A separate (slower first) IDF build.
[doc('build just the host monitor, on a lean mdns-free ESP-IDF')]
[group('firmware')]
build-host-monitor: (fw-build "host-monitor")

# Build the Claude buddy (Xtensa release) with the Bluetooth stack linked in. The BT Kconfig
# lives in its own sdkconfig.buddy.defaults, layered rather than added to the shared
# sdkconfig.defaults, so the other five apps do not carry ~250 KB of controller and host they
# never use. ESP_IDF_SDKCONFIG_DEFAULTS is semicolon-separated and later files win.
[doc('build the Claude buddy, with the Bluetooth stack linked in')]
[group('firmware')]
build-buddy: (fw-build "claude-buddy")

# Type-check the firmware without linking (fast). Same exclusions as `build`.
[group('firmware')]
[working-directory: 'firmware']
[script]
check:
    mapfile -t excl < <(just --justfile {{justfile()}} _workspace-excludes)
    PATH="{{pyshim}}:$PATH" cargo check --release --workspace "${excl[@]}"

# Lint the firmware, warnings as errors. Same exclusions as `build`.
[group('firmware')]
[working-directory: 'firmware']
[script]
lint-fw:
    mapfile -t excl < <(just --justfile {{justfile()}} _workspace-excludes)
    PATH="{{pyshim}}:$PATH" cargo clippy --release --workspace "${excl[@]}" -- -D warnings

# Lint the Claude buddy against its own BT-enabled ESP-IDF, warnings as errors. Split from
# `lint-fw` because the two need different IDF builds, not because the buddy is held to a
# looser standard — `just ci` runs both and neither may warn.
[doc('lint the Claude buddy against its own BT-enabled ESP-IDF')]
[group('firmware')]
lint-buddy: (fw-lint "claude-buddy")

# Report the firmware binary's section sizes (text/data/bss).
[group('firmware')]
size: build
    {{size_bin}} {{elf}}

# ---- Device (needs the board on {{port}}) ----
#
# One flash engine, `fw-run`, split into the command it composes and the act of running it.
# `_flash-cmd` is pure — it prints the command and touches no device — which is what makes it
# checkable without a board: its output can be diffed against what the old per-app recipes
# composed by hand. The named recipes below are wrappers over the engine.

# The exact command `fw-run` hands to `sg`, printed and not run. `%q`-quoting each declared
# variable is what keeps the semicolon in ESP_IDF_SDKCONFIG_DEFAULTS a separator between two
# sdkconfig files rather than one between two shell commands.
[private]
[script]
_flash-cmd app bin='':
    mapfile -t appenv < <(just --justfile {{justfile()}} _app-env {{app}})
    exports=""
    for v in "${appenv[@]}"; do exports+=" $(printf '%q' "$v")"; done
    binarg=""
    if [ -n "{{bin}}" ]; then binarg=" --bin {{bin}}"; fi
    printf 'export PATH="%s:$PATH" ESPFLASH_PORT="%s"%s; cargo run --release -p %s%s\n' \
      '{{fw_path}}' '{{port}}' "$exports" '{{app}}' "$binarg"

# Build, flash, and monitor ONE app by name — optionally one named bin of its package, for the
# bench tools that live beside an app. Runner is `espflash flash --monitor --non-interactive
# --baud 115200` (firmware/.cargo/config.toml); ESPFLASH_PORT names the port so espflash never
# prompts, and --non-interactive streams serial without a controlling TTY, so this works from a
# pipe/CI/agent (no crossterm input reader to fail). Ctrl-C exits.
[doc('build, flash, and monitor one app by name (optionally one of its bins)')]
[group('device')]
[working-directory: 'firmware']
[script]
fw-run app bin='':
    cmd="$(just --justfile {{justfile()}} _flash-cmd {{app}} {{bin}})"
    echo "▶ flashing {{app}} {{bin}} on {{port}}"
    {{sg}} "$cmd"

# Build, flash, and monitor the firmware (the qhw.1 board session, automated).
[doc('build, flash, and monitor the plant monitor')]
[group('device')]
run: (fw-run "plant-monitor")

# `just flash` == `just run` (build + flash + monitor).
alias flash := run

# Flash and monitor one named bin of the plant-monitor package — the bench tools
# that live beside the monitor itself. `just run-bin probe-rail-check` measures
# whether the AXP192's EXTEN bit gates the Grove 5 V probe rail (qhw.31); see
# kb/experiments/2026-07-08-probe-rail-gating/. Leaves that bin on the board —
# `just run` puts the monitor back.
[doc('flash one named bin of the plant-monitor package (the bench tools)')]
[group('device')]
run-bin bin: (fw-run "plant-monitor" bin)

# Flash and monitor one named bin of the pomodoro package — the bench tools that live beside
# the timer. `just run-bin-pomodoro paint-profile` times the paint at each of the four
# rotations, for a picture with a large contiguous fill and one without, so an over-budget
# paint can be located rather than only noticed. `just run-pomodoro` puts the timer back.
[doc('flash one named bin of the pomodoro package (the bench tools)')]
[group('device')]
run-bin-pomodoro bin: (fw-run "pomodoro" bin)

# Build, flash, and monitor the Claude desk pet. It advertises as Claude-XXXX, demands LE Secure
# Connections bonding, shows a FRESH RANDOM passkey on the glass for each pairing, renders the
# creature and the transcript HUD, and answers a pending tool call on A (allow) or B (deny).
# Pair it with `just bridge-pair` and type the digits the glass shows. `just run` puts the plant
# monitor back.
[doc('build, flash, and monitor the Claude desk pet')]
[group('device')]
run-buddy: (fw-run "claude-buddy")

# Flash and monitor one named bin of the Claude buddy package — for a bench tool alongside the
# desk pet itself. `just run-buddy` is the app.
[doc('flash one named bin of the Claude buddy package')]
[group('device')]
run-bin-buddy bin: (fw-run "claude-buddy" bin)

# Build, flash, and monitor the standalone pomodoro timer. `just run` puts the plant monitor
# back. Front click = start/pause, front double-click = restart session, front long hold =
# reset, side click = skip, power click = light or darken the glass (a dark screen is not
# painted at all). Only the front button reports double-clicks, so only its lone click waits
# out the 300 ms window; the side and power buttons stay immediate.
[doc('build, flash, and monitor the standalone pomodoro timer')]
[group('device')]
run-pomodoro: (fw-run "pomodoro")

# Build, flash, and monitor the orientation readout: the MPU6886's gravity vector as three
# live X/Y/Z bars, the pitch and roll, and the face the board is resting on. No buttons —
# turn the board and watch. The serial heartbeat logs the same vector beside the pose it was
# read as, which is how the board's axis convention is checked rather than assumed.
# `just run` puts the plant monitor back.
[doc('build, flash, and monitor the live IMU orientation readout')]
[group('device')]
run-orientation: (fw-run "orientation")

# Build, flash, and monitor the plume: an ambient, clock-driven feathered frond that breathes
# on the panel, stood on its USB-C port. No sensor and no buttons — just watch it move. The
# serial heartbeat only says it is alive; the plume has no state. `just run` puts the plant
# monitor back.
[doc('build, flash, and monitor the plume')]
[group('device')]
run-plume: (fw-run "plume")

# Build, flash, and monitor the homelab host monitor. Fetches the bearer-gated hostpulse
# endpoint ([host_monitor] endpoint+token in firmware/secrets.toml) and draws one row per
# host — name, live CPU/memory %, and two scrolling sparklines — for all three hosts.
# ESP_IDF_SYS_ROOT_CRATE=host-monitor builds the lean mdns-free IDF (see build-host-monitor).
# `just run` puts the plant monitor back.
[doc('build, flash, and monitor the homelab host monitor')]
[group('device')]
run-host-monitor: (fw-run "host-monitor")

# Flash and monitor the chime self-test: it plays every jingle note on the buzzer while
# listening on the PDM mic, and logs each note's acoustic level vs. the silent floor + PASS/FAIL
# — so "the chime is audible" is checked on-device, not by ear. (The tiny buzzer is a resonant
# transducer, so the test measures loudness above the floor, not pitch.) Read the floor vs. note
# levels and, if needed, calibrate MARGIN/MIN_LEVEL in src/chime_selftest.rs. `just run-pomodoro`
# puts the timer back.
[doc('flash the acoustic chime self-test (buzzer heard by the PDM mic)')]
[group('device')]
run-chime-selftest: (fw-run "pomodoro" "chime-selftest")

# Attach a serial monitor only, no flash. Ctrl-C to exit. `--non-interactive`
# skips espflash's crossterm input reader, so no controlling TTY (and no
# `script` pty shim) is needed — it streams serial straight to stdout, which is
# what a piped / CI / agent invocation wants. Reset-on-connect still yields a
# fresh boot; you forgo only the interactive Ctrl-R chip reset.
[doc('attach a serial monitor only, no flash')]
[group('device')]
monitor:
    {{sg}} '{{espflash}} monitor -p {{port}} -c {{chip}} --non-interactive'

# Print board / flash info over serial.
[group('device')]
board-info:
    {{sg}} '{{espflash}} board-info -p {{port}} -c {{chip}}'

# ---- Project meta ----

# Format all code (host + firmware); `just fmt check` to only verify.
[group('meta')]
fmt mode="write":
    cargo fmt --all {{ if mode == "check" { "--check" } else { "" } }}
    cd firmware && cargo fmt {{ if mode == "check" { "--check" } else { "" } }}

# Re-check the pinned esp-rs stack against crates.io latest.
[group('meta')]
[script]
versions:
    ua="stick-c-plus just versions (i.gouss@gmail.com)"
    for c in esp-idf-svc esp-idf-hal esp-idf-sys embuild ldproxy; do
      {{curl}} -s -H "User-Agent: $ua" "https://crates.io/api/v1/crates/$c" \
        | {{jq}} -r --arg n "$c" '.crate | "\($n): \(.max_stable_version)  (newest \(.newest_version))"'
    done

# The fast pre-commit gate: formatting + architecture, no compile (so it stays
# quick). This is what .githooks/pre-commit runs; the slow gates (clippy, tests,
# build) stay in `just ci`.
[doc('the fast pre-commit gate: formatting + architecture, no compile')]
[group('meta')]
precommit: (fmt "check") hex-lint

# Install the git hooks (points core.hooksPath at .githooks). Run once per clone.
# The pre-commit hook runs `just precommit` (fmt + hex-lint) before a commit.
[doc('install the git hooks (core.hooksPath -> .githooks); once per clone')]
[group('meta')]
setup-hooks:
    git config core.hooksPath .githooks
    @echo "hooks installed: .githooks/pre-commit → just precommit  (bypass: git commit -n)"

# Everything CI checks: format, architecture (hex-lint), lint both worlds, test,
# build. hex-lint runs early — an architecture breach fails fast, before builds.
[doc('everything CI checks: fmt, architecture, lint both worlds, test, build')]
[group('meta')]
ci: (fmt "check") hex-lint sprites-check lint lint-fw lint-buddy test build build-buddy

# Remove build artifacts (host + firmware). Confirmed because a firmware clean throws away
# the built ESP-IDF with it, and that costs minutes to put back. `just --yes clean` skips
# the prompt for a scripted caller.
[doc('remove build artifacts (host + firmware)')]
[group('meta')]
[confirm('delete host AND firmware build artifacts? the ESP-IDF rebuild takes minutes.')]
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
#   - No recipe here carries `[confirm]` — a prompt would break the contract.
# Full workflow: kb/guides/beads-triage.md.

# Ready work: open, unblocked, not deferred (br, human list — prints and exits).
[group('beads')]
ready:
    {{br}} ready

# Blocked work and what each waits on (br, human list — prints and exits).
[group('beads')]
blocked:
    {{br}} blocked

# Project counts by status / type / priority (br — prints and exits).
[group('beads')]
stats:
    {{br}} stats

# Top pick + its `br update … --status=in_progress` claim command (bv robot JSON).
[group('beads')]
next:
    {{bv}} --robot-next -f json

# Full triage — recommendations + blockers + graph health (bv, the mega-command).
[group('beads')]
triage:
    {{bv}} --robot-triage -f json

# Parallel execution tracks: what can run concurrently right now (bv robot JSON).
[group('beads')]
plan:
    {{bv}} --robot-plan -f json

# Flush the db → the committed .beads/issues.jsonl so beads changes can land.
# br is non-invasive (never runs git), so stage + commit the JSONL yourself —
# the recipe prints the exact command.
[doc('flush the beads db to the committed .beads/issues.jsonl')]
[group('beads')]
bead-sync:
    {{br}} sync --flush-only
    @echo "flushed → .beads/issues.jsonl  ·  commit: git add .beads/issues.jsonl && git commit -m 'beads: …'"

# Graph-health gate. bv's insight metrics are lazily computed and report
# Cycles:null when uncomputed (a false red), so the cycle count comes from
# `br dep cycles` instead. Runs br diagnostics + asserts zero cycles; else fails.
[doc('beads graph-health gate: br doctor + zero dependency cycles')]
[group('beads')]
[script]
[no-exit-message]
bead-check:
    {{br}} doctor
    if {{br}} dep cycles --json | {{jq}} -e '.count == 0' >/dev/null; then
      echo "✅ beads: no dependency cycles"
    else
      {{br}} dep cycles
      echo "❌ beads: dependency cycle(s) — break one edge with: br dep remove <child> <parent>"
      exit 1
    fi
