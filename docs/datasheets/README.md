# M5StickC Plus — reference documents

Datasheets and schematics for the board and its on-board parts. The PDFs
themselves are **not committed** (large, immutable binaries) — run
[`fetch.sh`](./fetch.sh) to download them here. This index carries the useful,
version-controlled parts: what each doc covers, its source, and the pin map.

## Documents

| File | Component | Covers | Source |
|------|-----------|--------|--------|
| `m5stickc_plus_schematic.pdf` | whole board | Wiring, nets, connectors | M5Stack (k016-p) |
| `esp32-pico-d4_datasheet_en.pdf` | ESP32-PICO-D4 | The SiP on this board (ESP32 + 4 MB flash) | Espressif |
| `esp32_datasheet_en.pdf` | ESP32 | SoC electrical/peripheral summary | Espressif |
| `esp32_technical_reference_manual_en.pdf` | ESP32 | **Peripheral registers — RMT (WS2812), GPIO, I2C, timers** | Espressif |
| `axp192_pmu.pdf` | AXP192 | Power management — rails, LDOs, LCD backlight, battery | X-Powers / M5Stack |
| `st7789v2_display.pdf` | ST7789V2 | 1.14" 135×240 TFT controller | Sitronix / M5Stack |
| `mpu6886_imu.pdf` | MPU6886 | 6-axis accel + gyro | TDK InvenSense / M5Stack |
| `bm8563_rtc_cn.pdf` | BM8563 | Real-time clock (PCF8563-compatible), 中文 | M5Stack |
| `spm1423_mic.pdf` | SPM1423 | PDM MEMS microphone | Knowles / M5Stack |

Most relevant to this LED driver: the **ESP32 TRM** (RMT peripheral that clocks
WS2812) and the **schematic** + **AXP192** (the PMU must be alive to power the
board; it also drives the LCD backlight).

## GPIO pin map

| Function | Pin(s) | Notes |
|----------|--------|-------|
| **Grove port (HY2.0-4P)** | **G32, G33** | + 5V, GND. G32 is the firmware's WS2812 data default. |
| LCD ST7789V2 | G15 MOSI, G13 CLK, G23 DC, G18 RST, G5 CS | SPI; backlight via AXP192 |
| Button A | G37 | on the front |
| Button B | G39 | on the side |
| Red LED | G10 | plain GPIO, active-low |
| IR transmitter | G9 | |
| Passive buzzer | G2 | avoid for the strip — reserved |
| Microphone SPM1423 | G0 CLK, G34 DATA | PDM |
| Internal I2C | G22 SCL, G21 SDA | shared bus, below |

## Internal I2C addresses

| Device | Address |
|--------|---------|
| AXP192 (PMU) | `0x34` |
| MPU6886 (IMU) | `0x68` |
| BM8563 (RTC) | `0x51` |

> The strip's data line defaults to **G32** (Grove). G2 carries the buzzer and
> G0/G34 the mic, so prefer the Grove pins (G32/G33) for external LEDs.
