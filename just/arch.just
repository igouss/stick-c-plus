# Architecture gates — the rules the code must keep, checked by tools that exit non-zero.
#
# Imported by the root justfile; the variables and `require`d tools it uses are declared there.

# Every app must say how it is built. An app whose bin/Cargo.toml carries no
# [package.metadata.board] cannot be built or flashed by `fw-build`/`fw-run`, and before this
# check the only symptom was a missing recipe nobody noticed. Cheap (one `cargo metadata`, no
# compile), so it sits in `precommit` beside hex-lint: an app that cannot be flashed cannot be
# added silently.
[doc('every firmware app must declare how it is built')]
[group('architecture')]
[group('firmware')]
[no-exit-message]
[script]
[working-directory('firmware')]
apps-check:
    meta="$(cargo metadata --no-deps --format-version 1)"
    apps="$(echo "$meta" | {{ jq }} -r '[.packages[] | select(.manifest_path | test("/apps/[^/]+/bin/Cargo.toml$"))] | length')"
    bad="$(echo "$meta" | {{ jq }} -r '.packages[]
        | select(.manifest_path | test("/apps/[^/]+/bin/Cargo.toml$"))
        | select((.metadata.board["idf-root-crate"] | type != "string") or (.metadata.board.summary | type != "string"))
        | .name')"
    if [ -n "$bad" ]; then
      echo "❌ apps-check: no usable [package.metadata.board] (needs idf-root-crate + summary):"
      echo "$bad" | sed 's/^/     /'
      echo "   declare it in firmware/apps/<app>/bin/Cargo.toml — see the plant-monitor one."
      exit 1
    fi
    echo "✅ apps-check: all $apps app(s) declare how they are built"
# Enforce the functional-core / imperative-shell split on every crate marked
# `[package.metadata.hex-arch] role = "domain"` (led-core, plant-core, esphome-api).
# Fails if a concrete effect — socket, file, thread, clock — leaks into a domain
# core. (effect-audit's own CI wiring is qhw.25.)
[doc('enforce the functional-core / imperative-shell split on domain crates')]
[group('architecture')]
audit:
    {{ effect_audit }} --strict --require-domain .
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
      if {{ hex_lint }} --manifest-path "$ws"; then :; else
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
