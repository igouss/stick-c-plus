---
id: buddy-bridge-bonding-gotchas
title: "Establishing the buddy BLE bond: four things that look like failures but aren't"
kind: finding
scope: project:stick-c-plus
reviewed: 2026-07-23
---

Bringing `buddy-bridge` up against a flashed `Claude-XXXX` stick has four traps that
each *look* like a broken link and each cost real time on rediscovery. All hit and
resolved on the Fedora host (BlueZ 5.87, adapter `hci0`) on 2026-07-23 while proving
the `es6` fail-open loop on-device.

## 1 — A successful bond used to print NOTHING (fixed)

The daemon logged `found …; connecting`, the passkey prompt appeared on stderr, and
then **silence** — identical output whether it bonded or hung. It had actually
bonded; there was just no log line for it. Fixed in `buddy-bridge`'s `DaemonPeer`:
`on_up` now logs `link up: bonded …` and `on_down` logs `link down: …`. **The
success signal is the `link up: bonded` line, not the passkey prompt.** If in doubt,
ask BlueZ, not the daemon log: `bluetoothctl info <addr>` → `Bonded: yes / Connected: yes`.

## 2 — A concurrent `bluetoothctl scan` steals the daemon's discovery

The daemon's `discover()` opens a BlueZ discovery session. Run `bluetoothctl scan on`
at the same time and the two sessions' filters intersect; the daemon then sits in
`scanning for a 'Claude-' peripheral…` forever with the adapter showing
`Discovering: no`. **Do not scan while the daemon runs** — it owns discovery. (Using
a separate `bluetoothctl scan` to *check* the stick advertises is fine only when the
daemon is stopped.)

## 3 — A bond is two halves; clearing one leaves a stale-LTK loop

Host `bluetoothctl remove` clears only the host's LTK. The stick keeps its half in
NVS **across a reflash** (a plain `espflash flash` does not touch NVS), so on the
next connect it offers a stale key and encryption fails. The daemon auto-recovers
(`remove_device` + re-acquire) once; if it loops, clear BOTH halves:
`just bridge-forget` (host) **and** erase the stick's NVS. The device's real
partition table — confirmed from its own boot log — is `nvs 0x9000, 0x6000` (the
espflash default single-app table; `firmware/partitions.csv` is not wired into the
runner), so: `espflash erase-region -p /dev/ttyUSB0 -c esp32 0x9000 0x6000`.

## 4 — A `timeout`-killed serial monitor orphans an espflash holding the port

`timeout … sg dialout -c '… espflash monitor …'` kills the `sg` wrapper but the
inner `espflash` survives, keeping `/dev/ttyUSB0` open, so the next flash/monitor
fails with `Failed to open serial port`. Check `fuser /dev/ttyUSB0`; clear with
`pkill -9 -f espflash`. (Serial and BLE are independent — the daemon needs only the
BT adapter, never the port — but a stuck monitor still blocks reflashing the stick.)

## The clean bring-up, distilled

```sh
just bridge-forget                       # if a prior bond is looping (host + prints NVS erase)
just run-buddy                           # flash the peer; then Ctrl-C the monitor (frees the port)
just bridge-pair                         # daemon on the hook's socket; wait for 'link up: bonded'
# operational check, from another shell:
bluetoothctl info <addr> | grep -E 'Bonded|Connected'
```

> **The passkey is no longer a constant to pipe in.** These four traps were found against the
> `ble-spike` bin, which used a fixed `123456` so pairing was reproducible across flashes. That
> bin is gone and so is the constant: the product firmware draws a **fresh random passkey per
> pairing** from the hardware RNG and shows it full-screen on the glass, because a published
> constant defeats the MITM protection LE Secure Connections bonding exists to provide. So
> `bridge-pair` prompts and you type what the stick shows — there is nothing to automate, and
> `just bridge-spike-pair` no longer exists. The four traps themselves are unchanged; they are
> properties of BlueZ and of the two-sided bond, not of the peer firmware.

See [buddy-permission-hook](../guides/buddy-permission-hook.md) for the full loop and
the fail-safe proof.
</content>
</invoke>
