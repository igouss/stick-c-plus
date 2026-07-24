# The Linux BLE central that carries Claude Code approvals to the stick.
#
# Imported by the root justfile; the variables and `require`d tools it uses are declared there.

# ---- Bridge (the Linux BLE central — needs BlueZ; the device test needs the stick) ----

# THE one command: run the bridge daemon on the socket the Claude Code hook resolves. It finds
# the stick from cold, bonds if it has to, and then reconnects by itself forever — across the
# stick's reboots, yours, and any trip out of range. Bond once; after that this never asks you
# anything. Flash the peer first with `just run-buddy`. RUST_LOG=debug for more; Ctrl-C to stop.
#
# The signal to watch for is `link up: bonded`, NOT the passkey prompt — a passkey prompt on a
# stick you have already bonded means the bond was lost, which `just bridge-pair` re-establishes.
[doc('run the bridge daemon on the socket the Claude Code hook resolves')]
[group('bridge')]
[script]
bridge: bridge-preflight
    sock="{{ bridge_sock }}"
    echo "▶ bridge ↔ claude-buddy, hook socket $sock"
    echo "  waiting for the stick; it is picked up whenever it appears. Ctrl-C to stop."
    RUST_LOG="${RUST_LOG:-info}" BUDDY_BRIDGE_SOCK="$sock" cargo run --release -p buddy-bridge
# Everything that must be true before the daemon can work, asserted rather than assumed. Each
# check exists because its absence produced a silent hang or a mystery failure, not a message.
[doc('assert everything the bridge daemon needs, before it needs it')]
[group('bridge')]
[no-exit-message]
[script]
bridge-preflight:
    if ! ls /sys/class/bluetooth/hci* >/dev/null 2>&1; then
      echo "❌ no BlueZ adapter under /sys/class/bluetooth — is Bluetooth up?"; exit 2
    fi
    # A soft-blocked adapter accepts every D-Bus call and simply never sees an advertisement:
    # the daemon scans forever and BlueZ reports no error at all. rfkill is deliberately NOT a
    # `require`d tool — not every host ships it — so an absent one skips this check instead of
    # failing the recipe.
    if command -v rfkill >/dev/null 2>&1 && rfkill list bluetooth 2>/dev/null | grep -q 'blocked: yes'; then
      echo "❌ bluetooth is rfkill-blocked — 'rfkill unblock bluetooth' first"; exit 2
    fi
    # A daemon already holding the hook socket wins the bind; a second one dies on it, or worse,
    # the two fight over the adapter and neither keeps a link.
    sock="{{ bridge_sock }}"
    if pgrep -x buddy-bridge >/dev/null 2>&1; then
      echo "❌ a buddy-bridge is already running (socket $sock) — stop it first: pkill -x buddy-bridge"; exit 2
    fi
    echo "✅ preflight: adapter present, radio unblocked, no bridge already running"
# Deliberately re-bond: forget BOTH halves of the bond, then bring the daemon up so a fresh
# pairing happens now, while you are looking at the glass. This is the ONLY recipe that should
# ever ask you for six digits — `just bridge` is the everyday command, and a bond survives
# reboots on both sides. Reach for this when a passkey prompt appears where none should.
# Inherits `bridge-forget`'s confirmation; `just --yes bridge-pair` skips it.
[doc('deliberately re-bond: forget both halves, then pair afresh')]
[group('bridge')]
bridge-pair: bridge-forget bridge
# Forget the HOST half of an out-of-sync bond to the Claude-XXXX stick. The DEVICE half lives in
# the stick's NVS and survives a reflash — this prints the erase command to clear it too. See
# kb/guides/buddy-permission-hook.md.
#
# Removing the device from BlueZ is now safe: it evicts the cache entry, and the daemon's
# discovery finds an advertising stick with no cache entry (which is what the old one could not
# do — that is why this recipe used to leave the stick undiscoverable). Confirmed because the
# way back is a trip to the glass to read six digits.
[confirm('drop the host half of the Claude-XXXX bond? re-bonding needs the passkey on the glass.')]
[doc('forget the host half of the Claude-XXXX bond')]
[group('bridge')]
[script]
bridge-forget:
    mapfile -t addrs < <({{ bluetoothctl }} devices 2>/dev/null | awk '/Claude-/{print $2}')
    if [ "${#addrs[@]}" -eq 0 ]; then echo "no Claude- bond on the host to forget"; else
      for a in "${addrs[@]}"; do echo "forgetting $a"; {{ bluetoothctl }} remove "$a" >/dev/null || true; done
      echo "✅ host bond(s) cleared."
    fi
    echo "the DEVICE half persists across reflash — erase the stick's NVS to fully reset the bond:"
    echo "  {{ sg }} 'espflash erase-region -p {{ port }} -c esp32 0x9000 0x6000'"
# The device-in-the-loop proof for the bridge: the #[ignore]d test that drives the REAL
# BluerCentral against a flashed Claude-XXXX stick — bond, heartbeat, chunked round-trip,
# reconnect-across-reboot, and the Just-Works-downgrade regression (the passkey callback must
# fire). Like `oracle`, it is #[ignore]d so a plain `cargo test` shows it *ignored*, never a
# false green, and it is deliberately NOT in `just ci` (a bond needs the physical device).
# Preflight asserts a BlueZ adapter is present; set STICK_PASSKEY to the six digits the glass
# shows (the firmware draws a fresh one per pairing — there is no constant to fall back on).
[doc('device-in-the-loop bridge test against a flashed stick')]
[group('bridge')]
[script]
bridge-device: bridge-preflight
    echo "▶ bridge device test — flash the peer first: just run-buddy"
    echo "  you will be asked to enter the passkey shown on the glass, and to power-cycle the stick."
    cargo test -p buddy-bridge-shell --test device_bridge -- --ignored --nocapture
