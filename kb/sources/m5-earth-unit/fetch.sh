#!/usr/bin/env bash
# Fetch the M5 Earth Unit (U019) schematic into this directory.
# Idempotent: an existing valid PDF is skipped. See ../m5-earth-unit.md.
set -euo pipefail
cd "$(dirname "$0")"

# name|url
docs=(
  "earth_unit_schematic.pdf|https://m5stack-doc.oss-cn-shenzhen.aliyuncs.com/728/U019_UNIT_EARTH_SCHE.pdf"
)

is_pdf() { [ "$(file -b --mime-type "$1" 2>/dev/null)" = "application/pdf" ]; }

for entry in "${docs[@]}"; do
  name="${entry%%|*}"; url="${entry#*|}"
  if [ -f "$name" ] && is_pdf "$name"; then
    printf "skip  %s\n" "$name"; continue
  fi
  if curl -fsSL --retry 3 --retry-delay 2 --max-time 300 -A "Mozilla/5.0" -o "$name" "$url" && is_pdf "$name"; then
    printf "OK    %-8s %s\n" "$(du -h "$name" | cut -f1)" "$name"
  else
    printf "FAIL  %s <- %s\n" "$name" "$url"; rm -f "$name"
  fi
done
