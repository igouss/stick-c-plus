# Handoff — the desk pet that lives off your Claude Code approvals

Written 2026-07-22. Epic: `stick-c-plus-claude-buddy-e3k` (+ 6 child beads).
Phase 1 is **done and proven on hardware** (`68280fd`); the other five are open.
The beads are the spec. This document is orientation and hard-won context.

---

# Part 1 — The idea, in the owner's words

> I want you to port `../claude-desktop-buddy/` to rust and flash it to my m5 stick.
> I want you to connect to my claude code

And later, narrowing it:

> We will not be using gifs, we will use ascii, this should make app smaller.

> I'm down with Single app slot, full 1.8 MB packs, but I don't want to brick my device
> and I want to be able to redeploy my other apps later.

> I guess some unsafe is required in microcontroller developpement, but keep it minimal
> and constraint it to one crate.

## Restated

Take `anthropics/claude-desktop-buddy` — a ~3 kLOC Arduino/C++ desk pet that targets this
exact board — and rebuild it in Rust on the shared platform in this repo. The pet sleeps
when nothing is happening, wakes when sessions start, gets visibly impatient when a
permission prompt is waiting, and **lets the owner approve or deny a tool call by pressing
a button on the device**.

The catch that shapes the whole project: upstream's counterpart is **Claude Desktop** on
macOS/Windows in developer mode, which runs the BLE bridge. **Claude Code ships no such
bridge.** So the host half does not exist and is ours to write.

## Decisions the owner made when asked

- **BLE Nordic UART, wire-compatible with upstream** — chosen over an easier WiFi/HTTP
  transport riding this repo's existing `host-monitor` rails. The cost is a NimBLE
  bring-up; the payoff is that the stick would also pair with real Claude Desktop.
- **ASCII pets only** *(reversed an earlier "full parity including GIF")*. 795 ASCII poses
  cost ~68 KB against 1.8 MB for GIF character packs. This one decision deleted ~640 lines
  of C++ from the port, the whole folder-push transport, the GIF decoder, the filesystem,
  and the entire partition-table problem.
- **Do not brick the device; other apps must stay redeployable.** Treated as a hard
  constraint on every step. It is currently satisfied trivially — see Part 2.
- **`unsafe` is permitted, minimal, confined to one crate.** Granted 2026-07-22 after the
  tree was found to be 100% unsafe-free and heap measurement was shown to need FFI. It is
  an exception for `esp-metrics`, not a general relaxation.

## How you will know it worked

The owner presses **A** on the stick and a tool call Claude Code was waiting on is
approved; presses **B** and it is denied. The pet's face reflects what the agent is doing
without anyone reading a log.

---

# Part 2 — Instructions for the next agent

## Before you touch anything

1. `br show stick-c-plus-claude-buddy-e3k`, then `br ready` and take the top unblocked
   bead. **The beads carry the real detail.** This file is orientation, not a substitute.
2. Read `/home/elendal/CLAUDE.md`. It is not boilerplate here: hexagonal/ECB with
   dependencies pointing inward, Gherkin at domain boundaries, cyclomatic complexity 1 per
   test (no loops; cover zero, one, many), explicit type annotations including on lambda
   parameters.
3. Read `kb/INDEX.md`. The board findings referenced below are real and were paid for.
4. The upstream C++ is at `/home/elendal/code/m5/claude-desktop-buddy`. `src/main.cpp` is
   the state machine; `REFERENCE.md` is the protocol spec and is the better document.

## The dependency graph (verified acyclic)

```
ble-peripheral-spike-mye  ──┬──→ bluer-bridge-spike-3zt ──┐
        (DONE, 68280fd)     │                             ├──→ buddy-permission-flow-es6 ──┐
                            └─────────────────────────────┤                                │
                                                          │                                ├──→ buddy-polish-ixx
buddy-domain-4n5 ──┬──→ buddy-display-214 ────────────────┼────────────────────────────────┘
   (READY NOW)     └───────────────────────────────────────┘
```

Two beads are workable in parallel right now: **`bluer-bridge-spike-3zt`** (the Linux
daemon) and **`buddy-domain-4n5`** (pure host code, no hardware needed).

## What is already true (measured 2026-07-22, not assumed)

Phase 1 shipped a working BLE peripheral. On the board, not merely compiled:

| | |
|---|---|
| Advertises as | `Claude-0292` (device `C8:85:41:4E:02:92`) |
| BlueZ resolves it as | "Nordic UART Service", RX + TX + CCCD |
| Bonding | `Paired: yes, Bonded: yes, LegacyPairing: no` ← LE Secure Connections |
| Bond across reboot | survives (`CONFIG_BT_NIMBLE_NVS_PERSIST`) |
| BLE heap cost | **36,692 B** (planning estimated 60–75 KB) |
| Largest contiguous block | **110,592 B, unchanged by BLE init** |
| Flash image | 711,872 B |

Run it: `just run-bin-buddy ble-spike`. It echoes newline-delimited lines back and emits an
unprompted `{"alive":N}` notification every 10 s.

### Six findings that will cost you a day each if you rediscover them

1. **`partitions.csv` is not in use, and never has been.** The board boots espflash's
   *default* table — one `factory` app at `0x10000` of `0x3F0000` (**3.9 MB**), `nvs`
   `0x6000`, **no OTA at all**. The dual-OTA layout in `partitions.csv` is aspirational, as
   its own comment admits. Consequences: the flash budget is enormous (712 KB used of
   3.9 MB), there is no OTA capability to protect, and the owner's "don't brick / keep
   other apps deployable" constraint is satisfied **because we change nothing**. Do not
   flash a new partition table without discussing it.
2. **`esp32-nimble` does not fragment notifications.** `send_value` reads the MTU only to
   reject a zero, then hands the whole buffer to `ble_gatts_notify_custom`, which
   truncates. Chunking at `mtu - 3` is the caller's job — `notify_chunked` in
   `ble_spike.rs` does it. Any new TX path must too.
3. **A mistyped Kconfig key is silently ignored.** `CONFIG_BT_BLE_ENABLED` is Bluedroid-only
   and did nothing under NimBLE. The *only* way to catch this is reading the **generated**
   `sdkconfig` under `firmware/target/.../esp-idf-sys-*/out/sdkconfig`. Do it every time you
   touch `sdkconfig.buddy.defaults`. (This is the qhw.1 rule; it earns its keep.)
4. **`claude-buddy` cannot compile against the shared IDF.** Its BT Kconfig lives in
   `firmware/sdkconfig.buddy.defaults`, deliberately separate so the other four apps do not
   link a ~250 KB stack they never use. Against the shared BT-disabled IDF, `esp32-nimble`
   produces **655 unresolved symbols**. Hence `claude-buddy` is *excluded* from
   `just build` / `check` / `lint-fw`, with `build-buddy` / `lint-buddy` beside them.
   `just ci` runs both and neither may warn. If you add a crate to the buddy, keep this
   split intact.
5. **`esp-metrics` is the one crate allowed to speak C.** Three heap counters, each a single
   documented FFI call. Every other crate in both workspaces keeps `#![forbid(unsafe_code)]`
   — that was universally true before and should stay so. If you need more FFI, put it here
   and justify it in a comment, or ask.
6. **`btleplug` cannot bond, on any platform.** Verified by enumerating its entire
   `Peripheral` trait: no `pair`, `bond`, or `unpair` method exists. Use **`bluer` 0.17.4**,
   and note its `bluetoothd` feature is **not** in the defaults — without it, `Device`,
   the agent, and the GATT client do not exist.

## The hazard that matters more than any bug

**The `PreToolUse` hook fails OPEN.** Claude Code's documented behaviour on hook timeout is
that it "proceeds" with the tool call. So a stick that is asleep, out of range, or unbonded
would **silently wave every tool call through** — a false green on the exact path whose
only job is to stop dangerous commands.

Required design, spelled out in `buddy-permission-flow-es6`:

- The hook holds its **own deadline, shorter** than the configured timeout.
- On device-no-answer, daemon-down, or nothing-bonded, the hook **exits 0 printing no
  JSON**, so Claude Code's ordinary permission prompt takes over.
- **Do not** use `permissionDecision: "ask"` as the fallback. Open upstream bug
  [anthropics/claude-code#39344](https://github.com/anthropics/claude-code/issues/39344):
  a hook returning `"ask"` can silently disable `permissions.deny` rules.
- **Test the fallback for real.** Kill the daemon, unplug the stick, let the deadline
  expire, and prove a normal prompt appears and nothing slipped through. A passing
  happy-path proves nothing here.

The working schema, for reference:

```json
{"hookSpecificOutput":{"hookEventName":"PreToolUse",
 "permissionDecision":"deny","permissionDecisionReason":"denied on the stick"}}
```

`permissionDecision` accepts `allow` / `deny` / `ask` / `defer`. Command-hook default
timeout is 600 s and it blocks synchronously — ample time for a human to press a button.

## Two things the upstream README says that the upstream code contradicts

The C++ is the truth. Port the code, not the prose:

- **Disconnected maps to `idle`, not `sleep`** (`main.cpp:480`).
- **`busy` needs three or more running sessions**, not one (`main.cpp:483`).

## Known deviation from REFERENCE.md, accepted deliberately

`REFERENCE.md` recommends encrypting the TX **CCCD**. NimBLE creates that descriptor itself
with plain ATT flags, and `esp32-nimble` *panics* if you try to build your own `0x2902`.
Reaching it needs raw C FFI. The characteristics themselves **are** encrypted
(`READ_ENC | READ_AUTHEN`, `WRITE_ENC | WRITE_AUTHEN`), so a sniffer learns subscription
state and never payload. Espressif's own examples do not encrypt it either. Document it;
do not quietly drop to `unsafe` to chase it.

## The thing in the spike that must NOT survive

`ble_spike.rs` uses a **fixed passkey, `123456`**, so pairing is reproducible across
flashes. That is fine for a spike and **fatal in the product** — a static passkey defeats
the MITM protection the bonding exists to provide. The real firmware must show a
**per-pairing random passkey on the glass** (`BLEServer::on_passkey_request`). This belongs
with `buddy-display-214`.

## Recipes that actually worked, so you do not re-derive them

```sh
# Flash + monitor the spike
just run-bin-buddy ble-spike

# Read serial WITHOUT the reset-on-connect that would drop an active BLE link
/usr/bin/sg dialout -c "stty -F /dev/ttyUSB0 115200 raw -hupcl -echo && timeout 30 cat /dev/ttyUSB0"

# Bond from Linux (KeyboardOnly agent + passkey entry)
{ echo "agent KeyboardOnly"; sleep 1; echo "default-agent"; sleep 1;
  echo "pair C8:85:41:4E:02:92"; sleep 6; echo "123456"; sleep 8; echo "quit"; } | bluetoothctl

# Watch notifications — keep TX selected; bluetoothctl stops printing them if you
# select-attribute away to RX, which will make you think notify is broken when it is not.
{ echo "connect C8:85:41:4E:02:92"; sleep 8; echo "menu gatt"; sleep 1;
  echo "select-attribute 6e400003-b5a3-f393-e0a9-e50e24dcca9e"; sleep 1;
  echo "notify on"; sleep 25; echo "quit"; } | bluetoothctl
```

`bluetoothctl` bond state lives in `/var/lib/bluetooth/<adapter>/<device>/info` (root-only).
If the **device** clears its bonds (factory reset does), BlueZ keeps offering a stale LTK
and `Device::pair()` is a **no-op when already paired** — it will not re-key. Recovery is
`adapter.remove_device(addr)` then re-pair, and `remove_device` invalidates the handle.
Build that as an explicit fallback; it is the sharpest edge in the bridge bead.

## Constraints inherited from elsewhere in this tree

- `CONFIG_FREERTOS_HZ=100` — sleeps under 10 ms busy-wait instead of yielding. **10 ms is
  the hard floor** for every thread period. Host tests cannot see this.
- The display thread needs **16 KiB**, not the default 8 KiB: a stack-hungry subsystem
  preempting it mid-SPI has previously corrupted the SPI lock. BLE is exactly such a
  subsystem.
- **A green host gate is not evidence the device works.** Flash it and watch serial for a
  reboot loop. Every claim in this document was made that way.
- On-glass confirmation is its own thing: describe what the panel actually showed. A
  screenshot answers layout; only the glass answers the panel.

## Working with the owner

Laconic voice. They are an experienced developer (industrial automation, IBM J9 GC/VM in
C++, latterly Spring Boot) and this is their homelab, where they choose Rust for pleasure
and hold it to a high bar. They care about clean architecture over easy wins, and they will
take the longer path if it is the right one — the ASCII decision and the `unsafe`
containment crate were both their calls, and both improved the design.

Ask before scope changes. Give a recommendation, not a survey.

## Definition of done for the epic

- Press **A** on the stick, a real Claude Code tool call is approved; **B** denies it.
- The fallback path is **tested**: no daemon, no device, no bond — a normal terminal prompt
  appears and nothing is silently allowed.
- The pet reflects session state without anyone reading a log.
- `just ci` green (which now includes `lint-buddy` and `build-buddy`), hex-lint clean, and
  the device **flashed and observed**.
- A `kb/` guide covering cold-start pairing, recovering from out-of-sync bonds, and
  installing the hook into `settings.json`.
