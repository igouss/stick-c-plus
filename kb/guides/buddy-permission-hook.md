---
id: buddy-permission-hook
title: "Press A to approve: wiring the buddy permission hook into Claude Code"
kind: guide
scope: project:stick-c-plus
reviewed: 2026-07-23
---

The desk-pet's headline feature: a PreToolUse hook blocks each Claude Code tool
call while the stick decides — **A approves, B denies** — and Claude Code honours
the button. This guide covers cold-start pairing, installing the hook, the config
knobs, recovering an out-of-sync bond, and — the load-bearing part — **proving the
fail-safe by breaking it**.

Everything here except *activating the hook in your live settings* and the
on-device proof was verified on the host with no device (`buddy-permission-flow-es6`).

## The shape of the loop

Two long-lived pieces plus one per-call hook:

```
claude code --spawns per tool call--> buddy-hook ⎫
                                                  ⎬ unix socket (BUDDY_BRIDGE_SOCK)
buddy-bridge (daemon, owns the BLE bond) <────────⎭
     │  BLE (Nordic UART, bonded)
     ▼
Claude-XXXX stick  —  A = approve, B = deny
```

- **`buddy-bridge`** is the daemon Claude Code does not ship: a bonding BLE central.
  It registers the pairing agent, scans for `Claude-XXXX`, holds the bond, and runs
  the permission coordinator behind a unix socket. One per machine, long-lived.
- **`buddy-hook`** is spawned *per tool call*. It cannot own a BLE bond, so it
  forwards the tool-call context to the daemon over the socket and blocks on the
  answer — **but only for its own deadline** (default 25 s, under the 30 s hook
  `timeout`). See the fail-safe section: a hook that cannot get a decision prints
  **nothing** and exits 0, handing control back to Claude Code's normal prompt.

## Build

```sh
cargo build --release -p buddy-bridge -p buddy-hook
# → target/release/buddy-bridge   (the daemon)
# → target/release/buddy-hook     (the PreToolUse hook)
```

The firmware side (the stick) is the `claude-buddy` app; flash it with `just run-buddy`
(see [flashing-and-serial-access](flashing-and-serial-access.md)). It advertises as
`Claude-XXXX` (last two BT-MAC bytes) and demands LE Secure Connections bonding with
passkey entry — a **fresh random passkey per pairing**, drawn on the device and shown
full-screen on the glass, which is what you type at the daemon's prompt below.

## Cold-start pairing

1. Power the stick; it advertises `Claude-XXXX` and shows a 6-digit passkey when a
   central attempts to bond.
2. Run the daemon in a terminal you can type into (the first bond reads the passkey
   from stdin, prompting on stderr):

   ```sh
   just bridge
   ```

3. It looks for the stick — retrying until it appears, so the order you start things
   in does not matter. On the **first** bond it prints
   `Enter the 6-digit passkey shown on the stick:`; type the digits from the glass and
   press enter.
4. On success the log shows `link up: bonded` and `hook socket listening at <path>`.

Leave `buddy-bridge` running (a `systemd --user` service or a `tmux` pane). It is
the process every hook talks to.

> **Bond once, and forever.** The bond persists across reboots on both sides, and the
> daemon rediscovers and reconnects by itself — a stick carried out of range and brought
> back rejoins with no prompt and no restart. `just bridge` is therefore the only command
> you normally run; it also brings the daemon up on the **same socket the hook resolves**,
> which a bare `cargo run` does not guarantee.
>
> The success signal is the `link up: bonded` log line, not the passkey prompt. A passkey
> prompt on a stick you have already bonded means the bond was lost — `just bridge-pair`
> clears both halves and re-establishes it deliberately. There is no constant to automate:
> the stick draws a fresh passkey per pairing.
>
> The bring-up traps are catalogued in
> [buddy-bridge-bonding-gotchas](../findings/buddy-bridge-bonding-gotchas.md) — including
> the discovery defect that made the daemon unable to find a stick from cold, whose
> mis-diagnosis this guide previously repeated.

## Installing the hook into settings.json

The hook is a `PreToolUse` command hook. Point it at the **built binary** (an
absolute path — Claude Code does not resolve `cargo`), with a `timeout` comfortably
longer than the hook's own deadline so the hook's deadline always fires first.

Ready-to-paste `~/.claude/settings.json` snippet (adjust the path):

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "/home/elendal/code/m5/stick-c-plus/target/release/buddy-hook",
            "timeout": 30
          }
        ]
      }
    ]
  }
}
```

`matcher: "*"` asks the stick for **every** tool call. To scope it to the dangerous
ones only, use e.g. `"Bash|Write|Edit"`. Only `PreToolUse` is wired — the daemon's
socket speaks exactly one message shape (the tool-call request); session lifecycle
events are not consumed, so do not add `SessionStart`/`Stop` hooks pointing here.

> **Activating this intercepts every tool call in your environment.** It fails safe
> (a missing daemon or stick degrades to the normal prompt), but it is your live
> config — install it deliberately, and test the fail-safe below before relying on it.

### Config knobs

Read by `buddy-bridge` (the daemon):

| Env | Default | Meaning |
|-----|---------|---------|
| `BUDDY_OWNER` | `Buddy` | Owner label shown on the glass. |
| `BUDDY_TZ_OFFSET_S` | `0` | Seconds east of UTC for the device clock (e.g. `-14400` for EDT). |
| `BUDDY_BRIDGE_SOCK` | *(see below)* | Explicit socket path. |

Read by `buddy-hook` (the per-call hook):

| Env | Default | Meaning |
|-----|---------|---------|
| `BUDDY_BRIDGE_SOCK` | *(see below)* | Explicit socket path — must match the daemon. |
| `BUDDY_HOOK_DEADLINE_MS` | `25000` | The hook's own deadline; keep it under the settings `timeout` (× 1000). |

**Socket resolution** (both binaries agree, in order): `BUDDY_BRIDGE_SOCK` if set,
else `$XDG_RUNTIME_DIR/buddy-bridge.sock`, else `<tmp>/buddy-bridge.sock`. If you
run the daemon under a different `XDG_RUNTIME_DIR` than Claude Code (services often
do), set `BUDDY_BRIDGE_SOCK` explicitly on **both** so they meet. Set env for the
hook via the settings `env` block or a wrapper script named as the `command`.

## Recovering an out-of-sync bond

A bond is two halves — the host's stored LTK and the stick's. If one is wiped
(reflash, `bluetoothctl remove`, NVS erase) the other offers a stale key and
encryption fails. The daemon detects the stale-LTK trap and does
`remove_device` + re-acquire automatically. If it loops, clear **both** halves and
re-pair cold:

**Host** — forget the bond:

```sh
bluetoothctl devices                 # find the Claude-XXXX address
bluetoothctl remove AA:BB:CC:DD:EE:FF
```

**Stick** — erase its NVS (where NimBLE keeps the bond). The buddy firmware is
flashed with espflash's **default single-app** partition table (partitions.csv is
not yet wired into the runner), so NVS is at **`0x9000`, size `0x6000`**:

```sh
/usr/bin/sg dialout -c 'espflash erase-region -p /dev/ttyUSB0 -c esp32 0x9000 0x6000'
```

> Verify the offset against the table actually flashed — if the OTA partition table
> (`firmware/partitions.csv`, qhw.12) is ever wired in, NVS becomes `0x9000`/`0x4000`
> and this command must change to match, or you erase the wrong region.

Then re-pair cold (above). A full-flash erase + reflash also clears it.

## Proving the fail-safe — by breaking it

Claude Code treats a hook **timeout as a non-blocking error and proceeds**. That is
a fail-*open* platform on the exact path meant to stop dangerous commands, so the
hook is built to fail *safe*: every non-decision prints nothing and exits 0, and the
normal terminal prompt takes over. This is not assumed — it is proven by breaking
each path. A green happy-path proves nothing here.

**Host-level (no device needed), already verified:**

```sh
# Daemon down → silent, exit 0 (normal prompt takes over):
echo '{"session_id":"s1","tool_name":"Bash"}' \
  | BUDDY_BRIDGE_SOCK=/nonexistent.sock ./target/release/buddy-hook ; echo "exit=$?"
# → prints nothing, exit=0

# Malformed payload → silent, exit 0 (never invents a call to approve):
echo 'not json' | ./target/release/buddy-hook ; echo "exit=$?"
# → prints nothing, exit=0
```

A real decision does emit — e.g. a `Deny` answer becomes exactly:

```json
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"denied on the stick"}}
```

`permissionDecision` is only ever `allow` or `deny` — **never `ask`** (upstream
`anthropics/claude-code#39344`: an `ask` from a hook can silently disable
`permissions.deny` rules) and never `defer`.

**On-device (needs the stick, paired, and you at the glass).** With the hook
installed and a live session, trigger a tool call and, for each break, confirm a
**normal terminal permission prompt appears** and **no tool call slips through
un-prompted**:

1. **Happy path** — call runs, glass shows the prompt, press **A** → allowed;
   press **B** → denied. Both honoured.
2. **Kill the daemon** mid-prompt (`Ctrl-C` the `buddy-bridge`) → hook rides its
   deadline, exits silent → normal prompt.
3. **Unplug / power off the stick** → daemon is up but nothing bonded → hook gets
   `Unbonded` → silent → normal prompt.
4. **Unbond** (host `bluetoothctl remove`) with the daemon up → same as (3).
5. **Let the deadline expire** — daemon up, bonded, but nobody presses a button →
   after `BUDDY_HOOK_DEADLINE_MS` the hook exits silent → normal prompt.

The device-side twin hazard (an absent `prompt` field in a snapshot legitimately
means "prompt gone", so a bare keepalive must never clear a live prompt) is closed
in the domain: raise a prompt, then let keepalives and lifecycle traffic flow, and
confirm the prompt **survives on the glass** until a button or a link-drop clears it.
</content>
</invoke>
