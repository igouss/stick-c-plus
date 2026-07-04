Feature: The sampler calibrates soil readings into a moisture percent
  As the firmware monitoring a plant through the M5 Earth Unit
  I want a batch of raw ADC readings folded into one calibrated percent
  So that Home Assistant sees a stable moisture value and only on change

  Scenario: A midpoint reading is half of the calibrated span
    Given a calibration of 1000 dry and 3000 wet
    When the probe is sampled at 2000
    Then the reported moisture is 50 percent

  Scenario: The dry endpoint reads as bone dry
    Given a calibration of 1000 dry and 3000 wet
    When the probe is sampled at 1000
    Then the reported moisture is 0 percent

  Scenario: Inverted wiring reads the same percent
    Given a calibration of 3000 dry and 1000 wet
    When the probe is sampled at 2000
    Then the reported moisture is 50 percent

  Scenario: A batch of readings is averaged, not sampled
    Given a calibration of 0 dry and 100 wet
    When the probe is sampled at 10, 20 and 60
    Then the reported moisture is 30 percent

  Scenario: An empty batch reports nothing
    Given a calibration of 1000 dry and 3000 wet
    When the probe is sampled at nothing
    Then no moisture is reported

  Scenario: An unchanged reading is not reported
    Given a calibration of 0 dry and 100 wet
    And a last reported moisture of 40 percent
    When the probe is sampled at 40
    Then no moisture is reported
