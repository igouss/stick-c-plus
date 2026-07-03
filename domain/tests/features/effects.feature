Feature: LED strip effects paint pixels
  As the firmware driving a NightDriver-style strip
  I want effects to render frames deterministically
  So that what reaches the LEDs is predictable and testable off-device

  Scenario: A solid effect fills every pixel with one color
    Given a strip of 3 pixels
    And a solid red effect
    When the strip is rendered at 0 ms
    Then every pixel is red

  Scenario: An empty strip renders without touching a pixel
    Given a strip of 0 pixels
    And a solid red effect
    When the strip is rendered at 0 ms
    Then the strip stays empty

  Scenario: A rainbow spreads distinct hues along the strip
    Given a strip of 3 pixels
    And a rainbow effect
    When the strip is rendered at 0 ms
    Then pixel 0 differs from pixel 1
