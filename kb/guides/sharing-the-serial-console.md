---
id: sharing-the-serial-console
title: "Sharing the board's serial console to multiple viewers (and the web)"
kind: guide
scope: project:stick-c-plus
reviewed: 2026-07-03
distils: []
---

How to let more than one person (or a browser) watch the board's UART at once,
reusing the host's existing `ttyd` + `oauth2-proxy` stack. Read
[serial-open-resets-esp32](../findings/serial-open-resets-esp32.md) first — it is
the constraint everything here works around.

## The constraint

`/dev/ttyUSB0` is a **single-owner** byte stream, and *opening* it reboots the
ESP32 (FT232 auto-reset). So the naive `ttyd espflash monitor …` is wrong on two
counts: `ttyd` forks one child **per browser client**, so two tabs = two opens =
two processes fighting for the port **and** a double reset. The fix is always the
same shape: **one process owns the port; everyone else mirrors that process, not
the device.** Two ways to do it, cheapest first.

## What's on this host (verified 2026-07-03)

`dnf`/`rpm` on this Fedora 43 box: `socat`, `telnet`, `ttyd` **installed**;
`tio`, `picocom`, `ser2net` **absent** but packaged — `ser2net 4.6.7` (updates).
So either option below is one `dnf install` away; nothing needs building.

## Option A — tmux mirror (recommended; zero new daemons)

Fits the existing stack, whose main `ttyd` already runs over tmux.

```sh
# one process owns the port, inside a tmux session:
/usr/bin/sg dialout -c \
  "tmux new-session -d -s board 'espflash monitor -p /dev/ttyUSB0 -c esp32'"

# ttyd just mirrors that session — it never touches the device itself.
# ttyd here is read-only unless you pass -W (see claude-code-ttyd.sh), so a
# viewer needs no extra flag:
ttyd -p 7683 -i 127.0.0.1 -b /board tmux attach -t board -r
```

- N web viewers **and** a local `tmux attach -t board` all see the *same* stream.
- Only the initial `espflash` start resets the chip; later attaches don't reboot it.
- Drop `-r` (and add ttyd `-W`) if you want the web to type / send `CTRL+R`.

## Option B — ser2net (lean serial-over-TCP, multi-client)

`sudo dnf install ser2net`, then `/etc/ser2net.yaml`:

```yaml
%YAML 1.1
---
connection: &m5stick
  accepter: telnet(rfc2217),tcp,127.0.0.1,3333
  connector: serialdev,/dev/ttyUSB0,115200n81,local,nobreak
  options:
    max-connections: 8      # >1 = broadcast serial output to every client
    kickolduser: false
```

`115200n81` (the FT232 won't go faster — see
[flashing-and-serial-access](flashing-and-serial-access.md)); `local` = ignore
modem-control lines; `nobreak` = no BREAK on connect. Attach with
`tio telnet://host:3333`, `telnet host 3333`, or `socat - TCP:host:3333`. Give the
unit the port with a `SupplementaryGroups=dialout` drop-in. Note: ser2net opens the
device on first client and (by default) releases it when the last leaves, so an
idle→reconnect resets the board once more; keep one client (e.g. a logger)
attached to avoid that.

## Fronting either of these on the web

Reuse the host's `ttyd` → `oauth2-proxy` (Pocket ID OIDC) → Caddy `.homelab`
gateway pattern (the units live in `~/IdeaProjects/infra`, not here). Point `ttyd`
at a **client** of the owner, never at the device:

```sh
ttyd -p 7683 -i 127.0.0.1 -b /board socat - TCP:127.0.0.1:3333        # ser2net
```

Multiple browser tabs are now fine — each becomes a client of the multiplexer,
which owns the one port. Add the `/board` upstream to the existing `ttyd-auth`
oauth2-proxy exactly like the main terminal. Bonus: the console is now on the
tailnet, so you can skip the browser and just `tio`/`telnet`/`console` to it from a
laptop.

## Caveats that survive every option

- **Flashing is exclusive.** `espflash flash` / `cargo run` drive the bootloader
  over the raw port + reset lines — they can't go through a shared mirror. Stop the
  owner (`tmux kill-session -t board`, or `systemctl stop ser2net`) before flashing
  our firmware, restart after.
- **One physical owner, always.** "Concurrent" here means concurrent *observers of
  one owner*, never two programs each holding `/dev/ttyUSB0`.

## Status

Verified on this host: the single-owner/reset constraint, and package
availability. **Not yet run on this board:** the ser2net setup above is the
recommended recipe, not yet exercised end-to-end — prove it before relying on
it (attach two clients, confirm both see the stream and the board resets only at
owner start).
