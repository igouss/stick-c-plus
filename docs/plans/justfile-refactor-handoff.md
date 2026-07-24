> **Superseded 2026-07-24 — implemented, with two corrections.** This was the plan; the work
> landed in commits `563bcf4`..`afa0562`. Two claims below did not survive measurement, and are
> struck through in place rather than quietly deleted:
>
> 1. **The `CARGO_TARGET_DIR` split for clippy was not done, because its premise was false.**
>    Clippy and rustc artifacts do *not* invalidate each other in a shared target dir here:
>    alternating `cargo clippy` and `cargo build` over the firmware workspace compiles zero
>    crates in either direction. The split would have cost a full rebuild and double the disk
>    to buy nothing.
> 2. **The split used `import`, not `mod`.** Imported recipes join the root namespace, so they
>    keep the shared variables and every recipe keeps its spelling; `mod` would have forced
>    every `{{ port }}` to become `$PORT`. One trap this surfaced: `just --fmt` ignores imported
>    files, so `fmt` formats each one in its own right or four fifths of the justfile falls out
>    of the formatting gate.
>
> Measured outcome of the parallel gate: 2m00 → 1m28 (27%) after a source change in both
> worlds; 32.1s → 29.3s on an already-warm tree.

# Handoff: rationalize the root justfile

## Goal

The root `justfile` (477 lines) documents itself well but duplicates its plumbing and
serializes its gate. Collapse the duplication, move per-app facts next to the app crates,
and make the gate parallel — without losing a single line of the doc comments, which are
the most valuable part of the file and must survive verbatim wherever a recipe survives.

Target: ~250 lines across a root justfile plus modules, with the deleted half being
duplication, not documentation.

## Current state

- `just 1.55.1`. Two Cargo workspaces: host (root `Cargo.toml`, stable rustc) and firmware
  (`firmware/Cargo.toml`, detached, Xtensa/ESP-IDF, own target dir).
- Seven apps under `firmware/apps/*/bin`, each already carrying
  `[package.metadata.hex-arch] role = "composition-root"` — `hex-lint` reads that table, so
  package metadata is an established mechanism in this repo, not a new one.
- `apps/plume` exists in the working tree and no recipe builds or flashes it.
- `.githooks/pre-commit` runs `just precommit`. `just ci` is the full gate.

## Verified facts (measured on this host — do not re-derive, do not doubt)

- `[parallel]` on a recipe runs its **dependencies** concurrently. Toy 3x2s: 6.0s serial →
  2.0s parallel. A failing dep aborts the run, propagates its exit code, and the recipe body
  does **not** execute. Safe for a gate.
- Concurrent `cargo` invocations on the **same** target dir block on the file lock. So
  `[parallel]` only pays across differing target dirs. Host and firmware workspaces already
  differ. ~~Splitting clippy onto its own `CARGO_TARGET_DIR` both stops clippy and rustc
  artifacts invalidating each other AND makes lint‖build parallelizable within one world.~~
  **(FALSE — measured. Clippy and rustc artifacts coexist in one target dir here; alternating
  the two compiles nothing. The split was dropped.)**
- `cargo metadata --no-deps --format-version 1` costs ~57ms on the firmware workspace and
  ~67ms on the host workspace. No build, no toolchain surprise. Cheap enough to call per
  recipe invocation.
- Module recipes **can** read parent variables declared with `export`.
- `alias r := module::recipe` works, so modularizing costs nothing at the prompt.
- Stable (no `set unstable` needed): `[parallel]`, `[script]`, `[group]`, `[private]`,
  `[doc]`, `[confirm]`, `[no-exit-message]`, `[working-directory: '...']`,
  `[positional-arguments]`, `require()`, `semver_matches()`, `source_file()`,
  `canonicalize()`, `--fmt --check`, `mod`.
- Unstable (needs `set unstable`): `set lists`, and `which()` which depends on it. Prefer
  `require()`, which is stable and fails with `could not find executable <name>`.

## The work, in five stages

Each stage is one commit. `just ci` must pass before starting the next. Scoped-commit
style, scope `justfile` (or `build`).

### Stage 1 — mechanical, no behaviour change

Adopt the attributes and settings the file predates:

- `set script-interpreter := ['bash', '-euo', 'pipefail']` + `[script]`, replacing the
  `#!/usr/bin/env bash` + `set -euo pipefail` header in all five shebang recipes. `hex-lint`
  deliberately uses `-uo` **without** `-e` — preserve that as an explicit override, and keep
  the comment saying why.
- `[working-directory: 'firmware']` in place of every `cd firmware &&` (14 occurrences).
- `[group(...)]` replacing the `# ---- ... ----` banner comments, which today do not survive
  into `just --list` at all.
- `[private]` on helpers; `[confirm]` on `clean` and `bridge-forget`.
- `[no-exit-message]` on recipes that print their own diagnosis (`hex-lint`, `bead-check`,
  `bridge-preflight`) so just's redundant `error: recipe X failed on line N` stops trailing
  the ❌ line.
- Hoist the socket path — `${BUDDY_BRIDGE_SOCK:-${XDG_RUNTIME_DIR:-/tmp}/buddy-bridge.sock}`
  is written twice, in `bridge` and `bridge-preflight`, and the surrounding comment insists
  daemon and hook must agree on it. One definition.
- `require(...)` for the eleven external tools the recipes shell out to (`br`, `bv`, `bb`,
  `hex-lint`, `effect-audit`, `espflash`, `bluetoothctl`, `rfkill`, `jq`, `curl`, `size`).
  Today a missing tool exits 127 and, in `hex-lint`, lands in the `*) unexpected exit`
  branch — a tool-absent reads as a policy failure. That is the false-red this kills.

### Stage 2 — per-app facts next to the app crate

Add `[package.metadata.board]` to each `firmware/apps/*/bin/Cargo.toml`, beside the existing
`hex-arch` table. It must express what today is smeared across ten recipes: the ESP-IDF root
crate, the sdkconfig layering, and a one-line summary of the app. Example shape (name the
keys as you see fit, but pick one shape and hold it):

```toml
[package.metadata.board]
idf-root-crate = "claude-buddy"
sdkconfig = ["sdkconfig.defaults", "sdkconfig.buddy.defaults"]
summary = "Claude Code desk pet — BLE Nordic UART, passkey on the glass"
```

Then reduce `build`, `build-pomodoro`, `build-orientation`, `build-host-monitor`,
`build-buddy`, `run`, `run-buddy`, `run-pomodoro`, `run-orientation`, `run-host-monitor`,
`run-bin`, `run-bin-pomodoro`, `run-bin-buddy`, `lint-fw`, `lint-buddy`, `run-chime-selftest`
to thin wrappers over **one** generic build engine and **one** generic flash engine that read
that metadata. Existing doc comments move onto the wrappers unchanged.

Two known drift bugs this must resolve rather than reproduce: `run-bin-buddy` and `run-buddy`
carry the BT sdkconfig separately and can diverge; `build` excludes `claude-buddy` because
esp32-nimble will not compile against a BT-disabled IDF (655 unresolved NimBLE symbols — the
existing comment explains it). The exclusion must survive, expressed as a property of the
app, not as a hardcoded `--exclude` flag.

`plume` gets its metadata too, so it becomes buildable/flashable by the same engine.

### Stage 3 — completeness gates

- `apps-check`: every workspace member under `apps/*/bin` must declare
  `[package.metadata.board]`. Wire into `precommit`. After this, an app that cannot be
  flashed cannot be added silently.
- `screens` hardcodes five crates and `screens-bless` blesses two. Derive both from
  `cargo metadata` targets instead — every `*-screenshots` example, every crate with a
  `goldens` test target. Report, do not silently skip, any app rendering screens with no
  goldens behind them: today three apps are in exactly that state and nothing says so.
- `just --fmt --check` on the justfile itself, in `precommit`. It is currently the one file
  the formatting gate does not cover.
- Add `--all-targets` to the host and firmware clippy invocations. Tests, examples, and the
  screenshot generators are unlinted today.

### Stage 4 — parallel gate

- ~~Split clippy onto its own `CARGO_TARGET_DIR` (host and firmware both).~~ **Dropped: the
  premise was measured and found false — see the banner.**
- Restructure `ci` into per-world serial chains joined by `[parallel]`. Verify the win by
  timing `just ci` before and after and report both numbers — if the measured speedup is
  small, say so plainly rather than assuming the design worked.

### Stage 5 — modules

Split into `just/host.just`, `just/board.just`, `just/bridge.just`, `just/beads.just`,
declared with `mod`; `kb/Justfile` becomes `mod kb`, replacing the `kb *args` forwarder. Root
keeps only settings, exported board constants (`PORT`, `BAUD`, chip, elf path, pyshim path),
`ci`, `precommit`, and the `mod`/`alias` declarations — no app knowledge, mirroring what
`platform/` is to `apps/`.

Preserve every current invocation spelling via `alias` (`just ready`, `just flash`, `just kb
links`, …). A user who never reads this document should notice nothing but speed.

## Constraints

- **Doc comments are the deliverable's other half.** Every surviving recipe keeps its
  comment. Where two recipes merge, merge the prose too — do not drop the reasoning
  (`screens`' "what it does NOT show", the `oracle`/`bridge-device` false-green arguments,
  the `sg`-alias and python-3.14 header). Where a comment explains a workaround that stage 1
  or 2 makes assertable, convert it into the assertion and keep the comment as the *why*.
- Verification is not optional. `just ci` green after every stage, and state which recipes
  you could not exercise. Several need the physical board on `/dev/ttyUSB0` (`run*`,
  `monitor`, `board-info`, `bridge-device`) or the network (`oracle`, `versions`) — for
  those, `just --dry-run` the recipe, confirm the composed command line is byte-identical to
  what the old recipe produced, and report that as what it is: a command-shape check, not a
  device test. Never report an unrun recipe as verified.
- Formatting is the last step: one `just fmt` at the very end of each stage, directly.
- Do not change what any recipe *does* to the board, the bond, or the beads db.

## Out of scope

Recipe semantics, the pyshim, the `sg dialout` mechanism, sdkconfig contents, CI outside this
repo, and the kb Justfile's internals.
