# The board: building an app for the ESP32, and putting it on the glass.
#
# Imported by the root justfile; the variables and `require`d tools it uses are declared there.

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
[working-directory('firmware')]
_app-env app:
    @cargo metadata --no-deps --format-version 1 | {{ jq }} -r --arg a '{{ app }}' '\
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
[working-directory('firmware')]
_workspace-excludes:
    @cargo metadata --no-deps --format-version 1 | {{ jq }} -r '.packages[] | select(.metadata.board.sdkconfig) | "--exclude", .name'
# The flashable apps and what each one is, from the metadata itself — so this list cannot go
# stale the way a hand-kept one does.
[doc('list the flashable apps and what each one is')]
[group('firmware')]
[working-directory('firmware')]
apps:
    @cargo metadata --no-deps --format-version 1 | {{ jq }} -r '.packages[] | select(.metadata.board) | "\(.name)\n    \(.metadata.board.summary)"'
# Build ONE app by name, with the ESP-IDF configuration it declares.
[doc('build one app by name (Xtensa release), with its declared ESP-IDF config')]
[group('firmware')]
[script]
[working-directory('firmware')]
fw-build app:
    mapfile -t appenv < <(just --justfile {{ justfile() }} _app-env {{ app }})
    PATH="{{ pyshim }}:$PATH" env "${appenv[@]}" cargo build --release -p {{ app }}
# Lint ONE app by name against the ESP-IDF configuration it declares, warnings as errors.
[doc('lint one app by name (Xtensa release), warnings as errors')]
[group('firmware')]
[script]
[working-directory('firmware')]
fw-lint app:
    mapfile -t appenv < <(just --justfile {{ justfile() }} _app-env {{ app }})
    PATH="{{ pyshim }}:$PATH" env "${appenv[@]}" cargo clippy --release -p {{ app }} --all-targets -- -D warnings
# Build the firmware (release).
#
# Excludes the apps that need their own IDF configuration — see `_workspace-excludes`, which
# derives that list from the app metadata instead of naming claude-buddy here.
[doc('build the firmware, release (apps needing their own IDF config excluded)')]
[group('firmware')]
[script]
[working-directory('firmware')]
build:
    mapfile -t excl < <(just --justfile {{ justfile() }} _workspace-excludes)
    PATH="{{ pyshim }}:$PATH" cargo build --release --workspace "${excl[@]}"
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
[script]
[working-directory('firmware')]
check:
    mapfile -t excl < <(just --justfile {{ justfile() }} _workspace-excludes)
    PATH="{{ pyshim }}:$PATH" cargo check --release --workspace "${excl[@]}"
# Lint the firmware, warnings as errors. Same exclusions as `build`.
[group('firmware')]
[script]
[working-directory('firmware')]
lint-fw:
    mapfile -t excl < <(just --justfile {{ justfile() }} _workspace-excludes)
    PATH="{{ pyshim }}:$PATH" cargo clippy --release --workspace "${excl[@]}" --all-targets -- -D warnings
# Lint the Claude buddy against its own BT-enabled ESP-IDF, warnings as errors. Split from
# `lint-fw` because the two need different IDF builds, not because the buddy is held to a
# looser standard — `just ci` runs both and neither may warn.
[doc('lint the Claude buddy against its own BT-enabled ESP-IDF')]
[group('firmware')]
lint-buddy: (fw-lint "claude-buddy")
# Report the firmware binary's section sizes (text/data/bss).
[group('firmware')]
size: build
    {{ size_bin }} {{ elf }}
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
    mapfile -t appenv < <(just --justfile {{ justfile() }} _app-env {{ app }})
    exports=""
    for v in "${appenv[@]}"; do exports+=" $(printf '%q' "$v")"; done
    binarg=""
    if [ -n "{{ bin }}" ]; then binarg=" --bin {{ bin }}"; fi
    printf 'export PATH="%s:$PATH" ESPFLASH_PORT="%s"%s; cargo run --release -p %s%s\n' \
      '{{ fw_path }}' '{{ port }}' "$exports" '{{ app }}' "$binarg"
# Build, flash, and monitor ONE app by name — optionally one named bin of its package, for the
# bench tools that live beside an app. Runner is `espflash flash --monitor --non-interactive
# --baud 115200` (firmware/.cargo/config.toml); ESPFLASH_PORT names the port so espflash never
# prompts, and --non-interactive streams serial without a controlling TTY, so this works from a
# pipe/CI/agent (no crossterm input reader to fail). Ctrl-C exits.
[doc('build, flash, and monitor one app by name (optionally one of its bins)')]
[group('device')]
[script]
[working-directory('firmware')]
fw-run app bin='':
    cmd="$(just --justfile {{ justfile() }} _flash-cmd {{ app }} {{ bin }})"
    echo "▶ flashing {{ app }} {{ bin }} on {{ port }}"
    {{ sg }} "$cmd"
# Build, flash, and monitor the firmware (the qhw.1 board session, automated).
[doc('build, flash, and monitor the plant monitor')]
[group('device')]
run: (fw-run "plant-monitor")
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
    {{ sg }} '{{ espflash }} monitor -p {{ port }} -c {{ chip }} --non-interactive'
# Print board / flash info over serial.
[group('device')]
board-info:
    {{ sg }} '{{ espflash }} board-info -p {{ port }} -c {{ chip }}'
