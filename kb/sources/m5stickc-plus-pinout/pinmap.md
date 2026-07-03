<!--
  RAW CAPTURE — do not distil here. Verbatim transcription of the "PinMap" and
  "PIN port" sections of M5Stack's official M5StickC PLUS documentation.
  Source: https://docs.m5stack.com/en/core/m5stickc_plus
  Canonical markdown: m5stack/m5-docs @ docs/en/core/m5stickc_plus.md (master)
  Re-fetch / re-diff with ./fetch.sh. See ../m5stickc-plus-pinout.md for the note.
-->

# M5StickC PLUS — official PinMap (verbatim)

## RED LED & IR Transmitter & BUTTON A & BUTTON B & Buzzer

| ESP32   | GPIO10  | GPIO9            | GPIO37     | GPIO39     | GPIO2      |
|---------|---------|------------------|------------|------------|------------|
| RED LED | LED Pin |                  |            |            |            |
| IR Transmitter | | Transmitter Pin  |            |            |            |
| BUTTON A |        |                  | Button Pin |            |            |
| BUTTON B |        |                  |            | Button Pin |            |
| Buzzer   |        |                  |            |            | Buzzer Pin |

## TFT LCD  (Driver IC: ST7789v2, resolution 135 × 240)

| ESP32   | GPIO15   | GPIO13  | GPIO23 | GPIO18  | GPIO5  |
|---------|----------|---------|--------|---------|--------|
| TFT LCD | TFT_MOSI | TFT_CLK | TFT_DC | TFT_RST | TFT_CS |

## GROVE PORT  (HY2.0-4P)

| ESP32      | GPIO33 | GPIO32 | 5V | GND |
|------------|--------|--------|----|-----|
| GROVE port | SCL    | SDA    | 5V | GND |

## MIC (SPM1423)

| ESP32    | GPIO0 | GPIO34 |
|----------|-------|--------|
| MICPHONE | CLK   | DATA   |

## 6-Axis posture sensor (MPU6886) & power management IC (AXP192)

| ESP32               | GPIO22 | GPIO21 |
|---------------------|--------|--------|
| 6-Axis IMU sensor   | SCL    | SDA    |
| Power management IC | SCL    | SDA    |

## AXP192 power rails

| Microphone | RTC  | TFT backlight | TFT IC | ESP32 / 3.3V MPU6886 | 5V GROVE |
|------------|------|---------------|--------|----------------------|----------|
| LDOio0     | LDO1 | LDO2          | LDO3   | DC-DC1               | IPSOUT   |

## PIN port (externally exposed GPIO)

    G0, G25/G36, G26, G32, G33

Notice (verbatim):
- G36/G25 share the same port; when one of the pins is used, the other pin
  should be set as a floating input. For example, to use G36 as the ADC input,
  configure G25 as FLOATING:

```clike
setup()
{
   M5.begin();
   pinMode(36, INPUT);
   gpio_pulldown_dis(GPIO_NUM_25);
   gpio_pullup_dis(GPIO_NUM_25);
}
```
