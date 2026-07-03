#!/usr/bin/env bash
# Dump app0 off the live M5StickC Plus and verify it is M5Stack's FactoryTest demo
# by matching FactoryTest-only strings (esp. its three random BLE UUIDs).
# Read-only / non-destructive. See ./README.md for the recorded run.
#
# Serial traps (2026-07-03): the agent shell lacks the `dialout` group, `sg` is
# shadowed by ast-grep, and this FT232 fails baud > 115200. Hence /usr/bin/sg +
# default baud. Details: ../../guides/flashing-and-serial-access.md
set -euo pipefail
cd "$(dirname "$0")"

PORT="${PORT:-/dev/ttyUSB0}"
DUMP="${DUMP:-app0.bin}"     # gitignored (3 MB, re-fetchable)

echo "== reading app0 (0x10000, 3 MB) off $PORT — ~4-5 min at 115200 =="
/usr/bin/sg dialout -c "espflash read-flash -p '$PORT' -c esp32 0x10000 0x300000 '$DUMP'"

echo "== FactoryTest-only signatures in the live flash =="
strings_to_check=(
  "1bc68b2a-f3e3-11e9-81b4-2a2ae2dbcce4"   # BLE service UUID — the decisive one
  "1bc68da0-f3e3-11e9" "1bc68efe-f3e3-11e9"
  "check Hardware" "Test Mode" "BMP8563 RTC Time" "Bat Vol error"
  "Drawdisplay" "MicRecordfft" "M5StickCPlus initializing"
)
for s in "${strings_to_check[@]}"; do
  printf "  %2d  %s\n" "$(grep -a -c "$s" "$DUMP" || true)" "$s"
done

echo "== build fingerprint =="
strings -n 5 "$DUMP" | grep -iE "arduino-lib-builder|v4\.4\.1|Apr 20 2022" | sort -u || true

if grep -a -q "1bc68b2a-f3e3-11e9-81b4-2a2ae2dbcce4" "$DUMP"; then
  echo "RESULT: CONFIRMED — this board runs FactoryTest."
else
  echo "RESULT: UUID NOT found — NOT the stock FactoryTest (or already reflashed)."
fi
