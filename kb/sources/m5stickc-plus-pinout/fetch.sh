#!/usr/bin/env bash
# Re-fetch M5Stack's official M5StickC PLUS doc page (the source of pinmap.md) so
# the committed capture can be diffed against upstream. Writes the raw canonical
# markdown next to this script; pinmap.md is the hand-transcribed PinMap extract.
# See ../m5stickc-plus-pinout.md.
set -euo pipefail
cd "$(dirname "$0")"

url="https://raw.githubusercontent.com/m5stack/m5-docs/master/docs/en/core/m5stickc_plus.md"
out="m5stickc_plus.upstream.md"

if curl -fsSL --retry 3 --retry-delay 2 --max-time 60 -A "Mozilla/5.0" -o "$out" "$url"; then
  printf "OK    %-6s %s\n" "$(du -h "$out" | cut -f1)" "$out"
  printf "diff the PinMap section against pinmap.md:\n"
  printf "  awk '/^## PinMap/,/^## /' %s\n" "$out"
else
  printf "FAIL  %s <- %s\n" "$out" "$url"; rm -f "$out"; exit 1
fi
