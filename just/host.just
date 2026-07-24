# Host domain — stable rustc, no device, no ESP-IDF.
#
# Imported by the root justfile; the variables and `require`d tools it uses are declared there.

# ---- Host domain (led-core) — stable rustc, no device ----

# Run the host domain suite (every host crate): unit + property + cucumber.
[group('host')]
test:
    cargo test --workspace
# Every crate that renders a screen, and every crate that pins one with goldens — discovered
# from the crate targets, so a new app joins `screens` and `screens-bless` by having the
# target, not by being remembered here.
[private]
_screenshot-targets:
    @cargo metadata --no-deps --format-version 1 | {{ jq }} -r '.packages[] | . as $p | .targets[] | select(.kind == ["example"]) | select(.name | endswith("screenshots")) | "\($p.name) \(.name)"'
[private]
_goldens-packages:
    @cargo metadata --no-deps --format-version 1 | {{ jq }} -r '.packages[] | select(any(.targets[]; .kind == ["test"] and .name == "goldens")) | .name'
# Which rendered screens nothing pins. A crate can render a picture and have no golden behind
# it — and then a layout change there is checked by no one. `screens` and `screens-bless` both
# end by naming those crates, so the hole is stated every time rather than being an absence.
[private]
[script]
_goldens-coverage:
    naked="$(comm -23 <(just --justfile {{ justfile() }} _screenshot-targets | cut -d" " -f1 | sort -u) \
                      <(just --justfile {{ justfile() }} _goldens-packages | sort -u) | paste -sd" ")"
    if [ -n "$naked" ]; then
      echo "⚠ renders a screen, but no goldens pin it: $naked"
    else
      echo "✅ every crate that renders a screen has goldens behind it"
    fi
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
[script]
screens:
    while read -r pkg example; do
      cargo run --quiet -p "$pkg" --example "$example"
    done < <(just --justfile {{ justfile() }} _screenshot-targets)
    just --justfile {{ justfile() }} _goldens-coverage
# Re-bless the golden screens from the current render — host-monitor and the buddy. The
# `goldens` tests (part of `just test`) render every state and fail if the picture drifts
# from the committed PNGs in each crate's goldens/; run this after an INTENTIONAL
# layout/colour change to accept the new look, then commit the goldens. Both render through
# the same function the test checks, so a blessed golden and a checked render match.
[doc('accept the current render as the new golden screens')]
[group('host')]
[script]
screens-bless:
    while read -r pkg; do
      BLESS_GOLDENS=1 cargo test -p "$pkg" --test goldens
    done < <(just --justfile {{ justfile() }} _goldens-packages)
    just --justfile {{ justfile() }} _goldens-coverage
# Regenerate platform-display/src/sprite/generated.rs from the vendored ClaudePix frames
# (babashka; no JS, no network). The generator re-hashes every preset and refuses to emit
# unless all 13 match the digests captured from the live site in a real browser — so a
# corrupted or silently-edited copy fails here, not on the glass. See
# kb/sources/claudepix.md.
[doc('regenerate the vendored ClaudePix sprite tables (babashka)')]
[group('host')]
sprites:
    {{ bb }} platform/platform-display/gen/generate.clj
# Fail if generated.rs has drifted from gen/frames.json. Part of `just ci`.
[group('host')]
sprites-check:
    {{ bb }} platform/platform-display/gen/generate.clj --check
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
    venv="{{ oracle_venv }}"
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
    cargo clippy --workspace --all-targets -- -D warnings
