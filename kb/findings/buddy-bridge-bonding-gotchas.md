---
id: buddy-bridge-bonding-gotchas
title: "Establishing the buddy BLE bond: what looks like a failure, and what actually was one"
kind: finding
scope: project:stick-c-plus
reviewed: 2026-07-23
check: ! grep -q 'uuids:' apps/buddy/buddy-bridge-shell/src/bluer_central.rs && grep -q 'discover_devices_with_changes' apps/buddy/buddy-bridge-shell/src/bluer_central.rs   # trap 2: a `uuids:` discovery filter hides the stick outright (the measured cause); `_with_changes` is the documented-race insurance
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

**The actual cause, measured.** The daemon set a discovery filter on the NUS service UUID.
BlueZ matches a UUID filter against what the peripheral *advertises*, and the stick cannot
advertise both name and UUID: flags (3) + `Claude-0292` as a complete local name (13) + a
128-bit UUID (18) is 34 bytes against a 31-byte limit, so one is displaced into the scan
response. The filter therefore excluded the device **outright** — BlueZ never reported it at
all, however long the scan ran. An unfiltered `bluetoothctl scan` had no such problem, which
is exactly why it saw the stick in two seconds and the daemon did not see it in six minutes.

Proven by direct A/B on the metal, cache and stick unchanged, filter the only variable:

| discovery filter | result |
| --- | --- |
| `uuids: {NUS}` + LE transport | two 20 s windows, nothing found |
| LE transport only | `found C8:85:41:4E:02:92` in the same second |

**The fix** is to filter on transport only and match on the advertised **name**, which is the
real identity predicate. The NUS UUID is still verified where the answer is reliable — on the
connected link, in `resolve_gatt`, which requires the service and both characteristics.

A second, *unproven* change rides along: `discover_devices_with_changes()` instead of
`discover_devices()`. The name arrives in the scan response, after the `DeviceAdded` that
creates the device, and bluer warns properties "may not be available yet" at that point; the
plain variant never re-emits a device, so a late name would be missed forever. But once the
UUID filter was gone the plain variant *also* found the stick — with a cache warmed seconds
before, so not a cold test. It is kept as documented-correct insurance, **not** as something
observed to fix anything. Do not cite it as the cause; the filter was the cause.

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
