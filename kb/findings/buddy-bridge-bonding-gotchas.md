---
id: buddy-bridge-bonding-gotchas
title: "Establishing the buddy BLE bond: what looks like a failure, and what actually was one"
kind: finding
scope: project:stick-c-plus
reviewed: 2026-07-23
check: grep -q 'discover_devices_with_changes' apps/buddy/buddy-bridge-shell/src/bluer_central.rs && ! grep -Eq '\.discover_devices\(\)' apps/buddy/buddy-bridge-shell/src/bluer_central.rs   # trap 2: the plain variant never re-offers a device once its name resolves, so a cold scan misses a stick that is advertising
---

Bringing `buddy-bridge` up against a flashed `Claude-XXXX` stick had four traps that each
*look* like a broken link. All hit on the Fedora host (BlueZ 5.87, adapter `hci0`) on
2026-07-23 while proving the `es6` fail-open loop on-device.

> **Revised the same day.** Trap 2 below was diagnosed wrongly the first time. What was
> blamed on `bluetoothctl` stealing the adapter was a defect in our own discovery code, and
> the recovery this finding prescribed — `just bridge-forget`, which evicts BlueZ's cache —
> made the stick **permanently** undiscoverable to the daemon while also forbidding the one
> thing that put it back. Followed literally, the procedure was a dead end. That is why the
> bond took several attempts even with the stick advertising on the desk. Both the code and
> the advice are fixed; the corrected account is kept because the wrong one cost hours.

## 1 — A successful bond used to print NOTHING (fixed)

The daemon logged `found …; connecting`, the passkey prompt appeared on stderr, and then
**silence** — identical output whether it bonded or hung. It had actually bonded; there was
just no log line for it. Fixed in `buddy-bridge`'s `DaemonPeer`: `on_up` now logs
`link up: bonded …` and `on_down` logs `link down: …`. **The success signal is the
`link up: bonded` line, not the passkey prompt.** If in doubt, ask BlueZ, not the daemon
log: `bluetoothctl info <addr>` → `Bonded: yes / Connected: yes`.

## 2 — The daemon could only find a stick somebody else had already found (fixed)

**Symptom.** From cold, with the host bond removed, the daemon logged
`scanning for a 'Claude-' peripheral…` and sat there for **six minutes** finding nothing,
while `bluetoothctl show` reported `Discovering: no`. A plain `bluetoothctl --timeout 25
scan on` found it in about two seconds. Restarting the daemon straight afterwards, it
logged `found …; connecting` **instantly**.

**The wrong conclusion** (recorded here on the first pass) was that the concurrent
`bluetoothctl` session had stolen the daemon's discovery, and the advice was "never scan
alongside the daemon". That got cause and cure backwards: the external scan was the only
thing making the daemon work at all.

**The actual cause.** A 128-bit service UUID and a name do not both fit in a 31-byte
advertisement, so the firmware puts the name in the **scan response**, which arrives
strictly after the advertisement. The daemon used bluer's `discover_devices()` and read
`device.name()` on `DeviceAdded` — where the name is still `None`. bluer's own docs say so:
*"Device properties are queried asynchronously and may not be available yet when a
DeviceAdded event occurs. Use `discover_devices_with_changes` when you want to be notified
when the device properties change."* With the plain variant the unnamed device is skipped
and **never re-emitted**, so the scan runs forever past a stick two feet away. It appeared
to work only once something else had cached the name, because `discover_devices()` replays
already-known addresses first and a cached name resolves immediately.

**The fix** is `discover_devices_with_changes()` in `bluer_central.rs`, which re-offers a
device on every property change, so the name is re-checked when the scan response lands.
Scanning with `bluetoothctl` alongside the daemon is no longer forbidden — BlueZ multiplexes
discovery sessions — but it is no longer of any use either.

## 3 — A bond is two halves; clearing one leaves a stale-LTK loop

Host `bluetoothctl remove` clears only the host's LTK. The stick keeps its half in NVS
**across a reflash** (a plain `espflash flash` does not touch NVS), so on the next connect
it offers a stale key and encryption fails. The daemon auto-recovers (`remove_device` +
rediscover); if it loops, clear BOTH halves: `just bridge-forget` (host) **and** erase the
stick's NVS. The device's real partition table — confirmed from its own boot log — is
`nvs 0x9000, 0x6000` (the espflash default single-app table; `firmware/partitions.csv` is
not wired into the runner), so:
`espflash erase-region -p /dev/ttyUSB0 -c esp32 0x9000 0x6000`.

**The daemon no longer recovers on the first failure**, and that matters: a stick that is
merely switched off fails in exactly the same way BlueZ reports a stale LTK, so the old
hair trigger destroyed a perfectly good bond every time the stick went out of range. It now
takes three consecutive encryption failures (`REBOND_AFTER_ENCRYPTION_FAILURES`) before the
bond is given up, and it says so in the log when it does. A genuine stale LTK fails
instantly and reaches that count in seconds; an absent stick just waits, bond intact.

## 4 — A `timeout`-killed serial monitor orphans an espflash holding the port

`timeout … sg dialout -c '… espflash monitor …'` kills the `sg` wrapper but the inner
`espflash` survives, keeping `/dev/ttyUSB0` open, so the next flash/monitor fails with
`Failed to open serial port`. Check `fuser /dev/ttyUSB0`; clear with `pkill -9 -f espflash`.
(Serial and BLE are independent — the daemon needs only the BT adapter, never the port —
but a stuck monitor still blocks reflashing the stick.)

## The clean bring-up, distilled

```sh
just run-buddy      # flash the peer; then Ctrl-C the monitor (frees the port)
just bridge         # the everyday command: finds the stick, bonds if it must, reconnects forever
```

Bond once. After the first pairing, `just bridge` never asks you anything again — the bond
survives reboots on both sides, and the daemon rediscovers and reconnects on its own. A
passkey prompt on a stick you have already bonded means the bond was lost; `just bridge-pair`
clears both halves and re-establishes it deliberately, while you are looking at the glass.

> **The passkey is not a constant to pipe in.** These traps were found against the `ble-spike`
> bin, which used a fixed `123456` so pairing was reproducible across flashes. That bin is gone
> and so is the constant: the product firmware draws a **fresh random passkey per pairing** from
> the hardware RNG and shows it full-screen on the glass, because a published constant defeats
> the MITM protection LE Secure Connections bonding exists to provide. You type what the stick
> shows — exactly once, for the life of the bond.

See [buddy-permission-hook](../guides/buddy-permission-hook.md) for the full loop and the
fail-safe proof.
