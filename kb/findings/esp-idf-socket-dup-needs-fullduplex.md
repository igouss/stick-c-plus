---
id: esp-idf-socket-dup-needs-fullduplex
title: "A socket dup() (TcpStream::try_clone) silently fails on ESP-IDF unless LWIP_NETCONN_FULLDUPLEX is set"
confidence: high
scope: project:stick-c-plus
derived-from: []
supersedes: []
reviewed: 2026-07-04
check: manual   # `just oracle` is green (host), yet a dup-based reader/writer split EOFs against the live board; a &TcpStream borrow adopts. Reproduced on-device (qhw.9).
---

**Claim:** On ESP-IDF, `dup()` on an lwIP socket — which is what Rust's
`TcpStream::try_clone()` calls — fails unless `CONFIG_LWIP_NETCONN_FULLDUPLEX`
is enabled. It is **off** in our build (the generated `sdkconfig` has
`LWIP_MAX_SOCKETS` and `LWIP_SO_REUSE` but no `FULLDUPLEX`). On Linux `dup()`
always succeeds, so any code that clones a socket to hold an owned reader **and**
an owned writer passes every host test and then breaks only on the board.

**Symptom (how it presents):** the TCP listener accepts — the port is open, not
refused — but the connection is **dropped before the first byte of reply**, and
the peer sees a bare `EOF`. There is **no panic, no reboot, no serial output**:
the `try_clone()?` returns `Err` early, the handler thread unwinds cleanly, and
any "connection ended" log sits at `debug`, below the INFO console level. For the
native-API server this reads to Home Assistant as *"Unable to connect… make sure
the device's YAML includes an `api` section"* — a message that misdirects toward
config when the real fault is the socket clone.

**Why it fooled the gates:** the host `aioesphomeapi` oracle (the exact HA client)
was **green** — Linux `dup()` works. This is the textbook host-green / device-red
divergence: a socket-layer capability the host has and the target does not. The
lesson pairs with [serial-open-resets-esp32](serial-open-resets-esp32.md): the
board's I/O layer has sharp edges the host hides.

**Fix (the one we took):** don't dup. `std` implements `Read` for `&TcpStream`
and `Write` for `&TcpStream`, so a single-threaded read-then-write pump can hold
two **shared borrows** of one stream — an owned reader and writer are unnecessary.
This is portable (no IDF Kconfig dependency), needs no `unsafe`, and removes the
device-hostile call outright rather than masking it. See
`esphome-server/src/server.rs::pump`.

**The other fix (rejected):** setting `CONFIG_LWIP_NETCONN_FULLDUPLEX=y` in
`sdkconfig.defaults` makes `dup()` work. It is one line, but it keeps an
unnecessary socket clone in the hot path and buys a per-socket lwIP cost only to
enable something the code should not need. Prefer removing the dup.

**Holds when:** any code path clones a TCP socket on ESP-IDF (`try_clone`,
`dup`/`dup2`, or a library that splits a stream into owned halves). Re-check when
Noise lands (qhw.10) — its framing must not reintroduce a clone.
