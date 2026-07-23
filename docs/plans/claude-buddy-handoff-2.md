# Handoff 2 — the desk pet, after the portable heart landed

Written 2026-07-23. Epic: `stick-c-plus-claude-buddy-e3k`.
Continues `docs/plans/claude-buddy-handoff.md` (Handoff 1), which is still true for
everything about the BLE peripheral and the hard-won board findings. **Read Handoff 1
first** — this file only records what changed and what to do next. The beads remain the
spec; this is orientation.

Two phases are now done and in `main`:
- **Phase 1 — the BLE peripheral** (`68280fd`), proven on hardware.
- **The portable heart — `buddy-domain-4n5`** (`8da9e01`): `buddy-core` + `buddy-wire`,
  pure host, 132 unit+property tests and 33 Gherkin scenarios green, hex-lint clean.

---

# Part 1 — The idea, in the owner's words

Unchanged from Handoff 1, repeated so this file stands alone.

> I want you to port `../claude-desktop-buddy/` to rust and flash it to my m5 stick.
> I want you to connect to my claude code

Narrowed later:

> We will not be using gifs, we will use ascii, this should make app smaller.

> I'm down with Single app slot, full 1.8 MB packs, but I don't want to brick my device
> and I want to be able to redeploy my other apps later.

> I guess some unsafe is required in microcontroller developpement, but keep it minimal
> and constraint it to one crate.

And the clarification that reshaped the render decision (2026-07-22):

> "ASCII" means the style the creature ALREADY renders in — the vendored ClaudePix 20x20
> 4-bit pixel sprites in platform-display, the same pipeline pomodoro and plant use — NOT
> monospace text and NOT GIF.

## Restated

Take `anthropics/claude-desktop-buddy` — a ~3 kLOC Arduino/C++ desk pet for this exact
board — and rebuild it in Rust on the shared platform in this repo. The pet sleeps when
nothing is happening, wakes when sessions start, gets visibly impatient when a permission
prompt is waiting, and **lets the owner approve or deny a tool call by pressing a button on
the device**. Upstream's host half is Claude Desktop in developer mode; **Claude Code ships
no BLE bridge**, so the host half is ours to write. The wire protocol stays byte-compatible
with upstream's `REFERENCE.md`, so the stick would also pair with real Claude Desktop.

## How the owner will know it worked

Press **A** on the stick and a tool call Claude Code was waiting on is approved; press **B**
and it is denied. The pet's face reflects what the agent is doing without anyone reading a
log.

---

# Part 2 — Instructions for the next agent

## Before you touch anything

1. Read Handoff 1 (`docs/plans/claude-buddy-handoff.md`) end to end. Its six board findings
   and its "hazard that matters more than any bug" (the fail-open hook) are still live.
2. `br show stick-c-plus-claude-buddy-e3k`, then `br ready`, and take the top unblocked
   buddy bead. **The beads carry the real detail.**
3. Read `/home/elendal/CLAUDE.md`. Hexagonal/ECB, dependencies inward, domain
   framework-free, Gherkin at boundaries, cyclomatic complexity 1 per test (no loops; cover
   zero, one, many), explicit type annotations including on lambda parameters. This bar is
   the point of the project, not overhead.
4. Read `kb/sources/claude-desktop-buddy-domain-model.md` — the full behaviour model read
   out of the C++ with file:line for every claim. It is the specification for anything that
   touches persona/wire behaviour.
5. The upstream C++ is at `/home/elendal/code/m5/claude-desktop-buddy` (`src/main.cpp` is
   the state machine; `REFERENCE.md` is the protocol spec, and the better document).

## Where the graph stands (verified 2026-07-23)

```
ble-peripheral-spike-mye  DONE ─→ bluer-bridge-spike-3zt  READY ─┐
                                                                 ├─→ buddy-permission-flow-es6 ─┐
buddy-domain-4n5  DONE ─┬─────────────────────────────────────────┘                             ├─→ buddy-polish-ixx
                        └─→ e3k.1 creature crate  READY ─→ buddy-display-214 ─┬──────────────────┘
                                                                              └─→ e3k.2 other creatures
```

**Two beads are ready now, both pure host, no board, and independent — run in parallel:**

- **`bluer-bridge-spike-3zt`** — the Linux BLE central Claude Code does not ship. This is
  the critical path to the epic's whole reason for existing ("press A, approve a real tool
  call"), and the spike with the sharpest unknowns, so prove it early. Non-obvious edges,
  all from Handoff 1's findings:
  - `btleplug` **cannot bond** on any platform. Use **`bluer` 0.17.4**, and note its
    `bluetoothd` feature is **not** in the defaults — without it `Device`, the agent, and
    the GATT client do not exist.
  - `esp32-nimble` does not fragment notifications; any TX path chunks at `mtu - 3`. The
    central must reassemble newline-delimited lines (that is what `buddy_wire::framing`
    already models — link it, do not re-derive it).
  - The stale-LTK trap: if the **device** clears its bonds, BlueZ keeps offering a stale
    LTK and `Device::pair()` is a **no-op when already paired**. Recovery is
    `adapter.remove_device(addr)` then re-pair, and `remove_device` invalidates the handle.
    Build that as an explicit fallback; it is the sharpest edge in this bead.
  - Read serial without the reset-on-connect that drops an active BLE link — the recipe is
    in Handoff 1.

- **`e3k.1` creature crate** — the `(species, PersonaState) → &Sprite` seam over
  `platform-display`, so adding a creature is adding DATA, not a match arm. Smaller, lower
  risk. Owns the state→preset binding for the first (ClaudePix) creature and the ordered
  species registry (the index is on the wire and in NVS, so **registry order is a
  compatibility surface**). Pure and host-testable — property-test that every species
  answers for all seven states and every answer resolves to a real sprite.

**Then, unblocked by those:**

- **`buddy-display-214`** (needs `e3k.1`; board work). It also carries the thing that
  **must not survive**: the spike's fixed passkey `123456`. The product must show a
  **per-pairing random passkey on the glass** (`BLEServer::on_passkey_request`) — a static
  passkey defeats the MITM protection the bonding exists for.
- **`buddy-permission-flow-es6`** (needs `bluer-bridge`). See the hazard below.
- **`e3k.2`** (more creatures) and **`buddy-polish-ixx`** (menu, persistence, clock, docs)
  are the leaves.

## The hazard that outranks any bug (still true, now closer)

**The `PreToolUse` hook fails OPEN.** Claude Code proceeds with the tool call on hook
timeout. A stick that is asleep, out of range, or unbonded would **silently wave every tool
call through** — a false green on the one path whose only job is to stop dangerous commands.
Required design lives in `buddy-permission-flow-es6`, and its heart is:

- the hook holds its **own deadline, shorter** than the configured timeout;
- on device-no-answer / daemon-down / nothing-bonded, the hook **exits 0 printing no JSON**,
  so Claude Code's ordinary permission prompt takes over;
- do **not** use `permissionDecision: "ask"` as the fallback (upstream bug
  [#39344](https://github.com/anthropics/claude-code/issues/39344) can silently disable
  `permissions.deny` rules);
- **test the fallback by breaking it** — kill the daemon, unplug the stick, let the deadline
  expire, prove a normal prompt appears and nothing slipped through. A passing happy-path
  proves nothing here.

## What the domain bead already gives you (do not re-derive)

`buddy-core` and `buddy-wire` are done and are the single source of truth. The firmware and
the bridge both link `buddy-wire`; the firmware links `buddy-core`. Reuse, don't
reimplement:
- `buddy_core::step` — the whole persona use case, `now` injected, no clock read. Feed it a
  `StepInput` each loop; it returns the persona to render plus menu/nap transitions.
- `buddy_wire::framing` — line framing with truncation as a **typed** error (`LineTooLong` /
  `RxOverflow`), never a silent drop.
- `buddy_wire::parse_inbound` — the exhaustive `Inbound` where **only `Snapshot` can touch
  prompt state** (this is defect fix b — a stray `{"evt":"turn"}` can no longer wipe a live
  approval). Strict time arity; asymmetric snapshot merge.
- `buddy_wire::command::dispatch` — exhaustive, with an explicit `Unknown` nack.
- `buddy_wire::permission::PermissionResponse` — the one message the device originates.

All five upstream defects are fixed and the two load-bearing quirks (stale shake baseline;
frozen nap counter with a still-live transition) are preserved. If board behaviour ever
disagrees with these crates, the crates are the spec — fix the adapter, not the core.

## Constraints inherited from elsewhere in this tree (still binding)

- `esp-metrics` is **the one crate allowed `unsafe`** (three documented FFI heap counters).
  Every other crate keeps `#![forbid(unsafe_code)]`. Need more FFI? Put it there and justify
  it, or ask.
- `claude-buddy` cannot compile against the shared BT-disabled IDF (655 unresolved NimBLE
  symbols), so it is **excluded** from `just build`/`check`/`lint-fw` and built by
  `build-buddy` / `lint-buddy`. `just ci` runs both; neither may warn. Keep the split.
- `partitions.csv` is **not** the table this board boots — do not reason from it, and do not
  flash a new partition table without discussing it. "Don't brick / keep other apps
  deployable" is satisfied because we change nothing.
- A mistyped Kconfig key is silently ignored — verify the **generated** `sdkconfig` every
  time you touch `sdkconfig.buddy.defaults`.
- `CONFIG_FREERTOS_HZ=100` → 10 ms is the hard floor for every thread period. The display
  thread needs **16 KiB**, not 8. BLE is a stack-hungry preemptor.
- **A green host gate is not evidence the device works.** For any board bead, flash it and
  watch serial for a reboot loop. On-glass claims describe what the panel actually showed.

## How this repo wants the work done

- **Verification is not optional.** Gherkin at each domain boundary, unit + property tests
  for the fine grain, cyclomatic complexity 1 in tests (no loops; cover zero, one, many).
  Time is a parameter, never a call — follow `pomodoro-core` and the shipped `buddy-core`.
- **Formatting is the LAST step, run directly** (owner's workflow rule, 2026-07-23): finish
  and verify the code, then `just fmt` once at the end — do not format-then-keep-editing,
  and do not delegate the format to a sub-agent. The pre-commit hook runs `fmt --check` +
  hex-lint and will reject an unformatted tree.
- **Gates:** `just test` (host), `just hex-lint` (both workspaces), `just lint`; for board
  beads add `just build-buddy` / `lint-buddy` and a real flash. `just ci` is the whole set.
- **Scope:** leave `.claude/` untracked — it is harness scaffolding, not project code.
- Commit style is Scoped Commits: `scope: imperative description`, scope names the
  subsystem (`buddy`, `bridge`, `render`), not a type.

## Working with the owner

Laconic voice. Experienced developer (industrial automation, IBM J9 GC/VM in C++, latterly
Spring Boot); this is their homelab and they choose Rust for pleasure, held to a high bar.
They value clean architecture over easy wins and will take the longer path when it is right
— the ASCII/pixel decision and the single `unsafe` containment crate were both their calls.
Give a recommendation, not a survey. Ask before scope changes.

## Definition of done for the epic (unchanged)

- Press **A**, a real Claude Code tool call is approved; **B** denies it.
- The fallback path is **tested**: no daemon, no device, no bond — a normal terminal prompt
  appears and nothing is silently allowed.
- The pet reflects session state without anyone reading a log.
- `just ci` green (incl. `lint-buddy` / `build-buddy`), hex-lint clean, device flashed and
  observed.
- A `kb/` guide covering cold-start pairing, recovering from out-of-sync bonds, and
  installing the hook into `settings.json`.

---

# The prompt to paste to the next agent

> Read `docs/plans/claude-buddy-handoff-2.md` (both parts) and the Handoff 1 it continues,
> then `br show stick-c-plus-claude-buddy-e3k` and `br ready`.
>
> Phase 1 (the BLE peripheral) and the portable heart (`buddy-domain-4n5`: `buddy-core` +
> `buddy-wire`) are done and in `main`. Two beads are ready and independent, both pure host
> with no board: **`bluer-bridge-spike-3zt`** (the Linux BLE central — Claude Code ships no
> bridge, so the host half is ours) and **`e3k.1`** (the creature crate, the
> `(species, state) → Sprite` seam). Take `bluer-bridge-spike-3zt` first — it is the
> critical path to the epic's whole point and the spike with the sharpest unknowns.
>
> Non-negotiables: `bluer` 0.17.4, not `btleplug` (which cannot bond), and enable its
> non-default `bluetoothd` feature; build the stale-LTK recovery
> (`remove_device` + re-pair) explicitly. Link `buddy_wire` for framing/parsing — do not
> re-derive the protocol. The `PreToolUse` hook fails OPEN, so the permission fallback must
> be designed and proven by breaking it. The spike's fixed passkey `123456` must not survive
> into the product (that belongs to `buddy-display-214`). `partitions.csv` is not the table
> the board boots. `esp-metrics` is the only crate allowed `unsafe`.
>
> This repo verifies everything: Gherkin at boundaries, unit + property tests, cyclomatic
> complexity 1 (no loops; cover zero/one/many), time injected as a parameter. Formatting is
> the last step — run `just fmt` once at the end, directly, never mid-work. A green host
> gate is not evidence the device works — for any board bead, flash it and watch serial.
