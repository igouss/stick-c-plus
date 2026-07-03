#!/usr/bin/env bash
# Fetch the M5StickC Plus reference PDFs into this directory.
# Idempotent: existing valid PDFs are skipped. See README.md for what each covers.
set -euo pipefail
cd "$(dirname "$0")"

# name|url
docs=(
  "esp32_datasheet_en.pdf|https://www.espressif.com/sites/default/files/documentation/esp32_datasheet_en.pdf"
  "esp32-pico-d4_datasheet_en.pdf|https://www.espressif.com/sites/default/files/documentation/esp32-pico-d4_datasheet_en.pdf"
  "esp32_technical_reference_manual_en.pdf|https://www.espressif.com/sites/default/files/documentation/esp32_technical_reference_manual_en.pdf"
  "m5stickc_plus_schematic.pdf|https://m5stack-doc.oss-cn-shenzhen.aliyuncs.com/669/k016-p-StickC-Plus-sche.pdf"
  "axp192_pmu.pdf|https://m5stack.oss-cn-shenzhen.aliyuncs.com/resource/docs/datasheet/core/AXP192_datasheet_en.pdf"
  "st7789v2_display.pdf|https://m5stack.oss-cn-shenzhen.aliyuncs.com/resource/docs/datasheet/core/ST7789V.pdf"
  "mpu6886_imu.pdf|https://m5stack.oss-cn-shenzhen.aliyuncs.com/resource/docs/datasheet/core/MPU-6886-000193%2Bv1.1_GHIC_en.pdf"
  "bm8563_rtc_cn.pdf|https://m5stack.oss-cn-shenzhen.aliyuncs.com/resource/docs/datasheet/core/BM8563_V1.1_cn.pdf"
  "spm1423_mic.pdf|https://m5stack.oss-cn-shenzhen.aliyuncs.com/resource/docs/datasheet/core/SPM1423HM4H-B_datasheet_en.pdf"
)

# A real PDF, tolerant of a leading UTF-8 BOM (the AXP192 file has one). Trust
# file(1)'s content sniff, not a naive first-4-bytes == "%PDF" check.
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
