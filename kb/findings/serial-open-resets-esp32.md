---
id: serial-open-resets-esp32
title: "Opening /dev/ttyUSB0 resets the ESP32, and the port is single-owner — shared consoles must fan out from one holder"
confidence: high
scope: board:m5stickc-plus
derived-from: [2026-07-03-identify-factory-firmware]
supersedes: []
reviewed: 2026-07-03
check: manual   # connect espflash monitor twice; each connect reboots the board — recipe below
---

**Claim:** Any process that opens `/dev/ttyUSB0` asserts the FT232's DTR/RTS
auto-reset lines and **reboots the ESP32**; and because a raw serial port is a
single-owner byte stream, a *second* concurrent opener cannot cleanly share it
(the two readers race for each byte, and `espflash` additionally holds the port).
Therefore serving this console to more than one viewer requires **one persistent
owner that fans the stream out** — never a parallel or per-viewer `open()`.

**Evidence:** During
[2026-07-03-identify-factory-firmware](../experiments/2026-07-03-identify-factory-firmware/README.md),
`espflash` connected via the flash **stub** on every `board-info` / `read-flash` /
`monitor` call — and that handshake *is* a `DTR→EN`, `RTS→IO0` reset (it is how the
stub gets loaded), so a connect demonstrably reboots the chip. The auto-reset
circuit is thus present and functional (default-reset never had to be disabled).
The "second opener can't share" half is standard POSIX tty semantics, not board
magic. See [flashing-and-serial-access](../guides/flashing-and-serial-access.md)
for the connect recipes this builds on. *(The fan-out remedies — ser2net or
a tmux-mirrored monitor — are written up in
[sharing-the-serial-console](../guides/sharing-the-serial-console.md) but not yet
exercised on this board.)*

**Holds when:** tools open the port the normal way (asserting DTR/RTS) — `espflash`,
`espflash monitor`, `cat`, `tio`, `screen`. The reset fires once per fresh open.

**Breaks when:** you front the port with a single long-lived owner (a console
server or a tmux session running the monitor) — then many viewers share the one
stream cleanly and the reset happens **once**, at the owner's start. Also softened
by `--before no-reset`, but that can't sync to already-running firmware anyway.

**How to apply:** Do **not** wire a web terminal as `ttyd espflash monitor …` — ttyd
forks one child per browser client, so every tab is another `open()` = another
reset **and** a byte race over the port. Put exactly one process on `/dev/ttyUSB0`
and mirror *that* to viewers. Recipe to confirm the reset: run
`/usr/bin/sg dialout -c 'espflash monitor -p /dev/ttyUSB0 -c esp32 --non-interactive'`
twice — each connect reprints the boot banner (now our firmware's
`plant-monitor: std/ESP-IDF boot skeleton up …`, formerly FactoryTest's
`@M5StickCPlus initializing…`), proving open ⇒ reboot. (`--non-interactive`
streams serial without a controlling TTY, replacing the old `script` pty shim.)
