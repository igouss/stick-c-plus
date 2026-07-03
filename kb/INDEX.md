# kb index

Curated one-liners, grouped by topic. Within a topic they run **raw → derived**:
`source` / `experiment` (raw) first, then the `finding` / `guide` distilled from
them — so each cluster reads as a lineage. Skim here, read the files. See
[README.md](README.md) for how the two-voice system works.

## The board — identity, hardware, bring-up

- `source` [m5stack-m5stickc-plus](sources/m5stack-m5stickc-plus.md) — M5Stack's
  Arduino library + the shipped **FactoryTest** demo (git submodule, MIT, pinned
  `4c87db9`/`0.1.1`). The clearest bring-up reference: `begin()` shows the AXP192 →
  LCD → Beep → RTC power-on order the datasheets only imply.
- `source` [m5stickc-plus-datasheets](sources/m5stickc-plus-datasheets.md) — board
  schematic + ESP32/AXP192/ST7789/MPU6886/BM8563/SPM1423 datasheets (PDFs
  gitignored; `fetch.sh` reproduces them). Most relevant: ESP32 **TRM** (RMT clocks
  WS2812) + schematic + AXP192.
- `source` [m5stickc-plus-factory-image](sources/m5stickc-plus-factory-image.md) —
  the **whole 4 MB flash read off our board** while stock, committed (not
  gitignored) because it's **irreplaceable** — once we flash, the factory demo
  can't be regenerated, only restored from this file (`espflash write-bin 0x0`).
  Verified complete; sha256 in its `SHA256SUMS`.
- `experiment` [2026-07-03 · Is our board running FactoryTest?](experiments/2026-07-03-identify-factory-firmware/README.md)
  — **confirmed**: dumped live `app0`, matched FactoryTest's three random BLE UUIDs
  + private strings. Not a name-match — a fingerprint match.
- `finding` [board-runs-factorytest-demo](findings/board-runs-factorytest-demo.md)
  — `high` · the flashed app *is* FactoryTest (Apr-2022 / `0.0.7`-era build),
  unmodified — until we flash our own, at which point re-verify.
- `finding` [axp192-powers-lcd-backlight](findings/axp192-powers-lcd-backlight.md)
  — `high` · display stays dark until the AXP192 (`0x34`) is programmed; backlight
  is the **LDO2** rail (I2C reg `0x28`), not a GPIO/PWM. Bring the PMU up first.
- `finding` [ws2812-grb-byte-order](findings/ws2812-grb-byte-order.md) — `high` ·
  WS2812 latches **G-R-B**; our in-tree `Ws2812Rmt` adapter emits the swap — keep
  `Rgb` as RGB in the domain and don't double-swap at the boundary.
- `guide` [m5stickc-plus-board-reference](guides/m5stickc-plus-board-reference.md)
  — pin map + I2C addresses; **why the strip defaults to G32** (G2 = buzzer,
  G0/G34 = mic are taken).

## Development — toolchain & flashing

- `guide` [esp-rust-toolchain](guides/esp-rust-toolchain.md) — the `esp` rustc fork
  for Xtensa (`espup`, `xtensa-esp32-none-elf`), why `firmware/` is a detached
  workspace, and why the firmware is now on `esp-hal 1.1` (smartled dropped).
- `finding` [esp-rs-ota-version-matrix](findings/esp-rs-ota-version-matrix.md) —
  `high` · WiFi/OTA needs **esp-hal 1.1**; `esp-hal-smartled`'s `~1.0` pin is the
  wall (so we own the RMT encoder), and `esp-wifi`/`esp-hal-embassy` are superseded
  by **`esp-radio`/`esp-rtos`**. Carries the pinned latest OTA crate set.
- `guide` [flashing-and-serial-access](guides/flashing-and-serial-access.md) — the
  board is `/dev/ttyUSB0` (FT232); the three traps that make a working `espflash`
  look broken (missing `dialout` group → `/usr/bin/sg`; `sg` shadowed by ast-grep;
  FT232 fails baud > 115200); and the read/monitor/flash recipes.
- `finding` [serial-open-resets-esp32](findings/serial-open-resets-esp32.md) —
  `high` · opening the port reboots the ESP32 (FT232 auto-reset) and the port is
  single-owner, so a shared console must fan out from **one** holder — never
  `ttyd espflash monitor` (one child per browser tab = reset + byte race).
- `guide` [sharing-the-serial-console](guides/sharing-the-serial-console.md) — let
  many viewers (or the web) watch the UART at once: tmux-mirror (recommended),
  `ser2net`, or `conserver`, fronted by the host's `ttyd`/`oauth2-proxy` stack.
  Package availability + configs verified on-host; setups not yet run on the board.
