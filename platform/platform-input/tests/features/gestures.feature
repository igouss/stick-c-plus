Feature: The board's buttons report gestures

  The boundary this suite guards: a thumb on the glass becomes a stream of
  (button, gesture) events. The fine grain — every debounce edge case, every
  window boundary — lives in the unit and property tests next to the code; here
  we pin the behaviour a person would describe out loud.

  Background:
    Given a board where the front button reports double-clicks

  Scenario: A board nobody touches says nothing
    When one second passes
    Then the events are "nothing"

  Scenario: A quick press on the side button is a click
    When the side button is held for 40 ms
    And the side button is released for 40 ms
    Then the events are "side click"

  Scenario: A prompt button does not wait out the double-click window
    When the side button is held for 40 ms
    And the side button is released for 20 ms
    Then the events are "side click"

  Scenario: A button that reports double-clicks holds its lone click back
    When the front button is held for 40 ms
    And the front button is released for 20 ms
    Then the events are "nothing"

  Scenario: ...and releases it once the window has passed
    When the front button is held for 40 ms
    And the front button is released for 400 ms
    Then the events are "front click"

  Scenario: Two quick presses on the front button are one double-click
    When the front button is held for 40 ms
    And the front button is released for 25 ms
    And the front button is held for 40 ms
    And the front button is released for 25 ms
    Then the events are "front double-click"

  Scenario: Two slow presses are two separate clicks
    When the front button is held for 40 ms
    And the front button is released for 400 ms
    And the front button is held for 40 ms
    And the front button is released for 400 ms
    Then the events are "front click, front click"

  Scenario: A long press is a hold, and never also a click
    When the front button is held for 700 ms
    And the front button is released for 400 ms
    Then the events are "front long-hold"

  Scenario: A hold is reported while the button is still down
    When the front button is held for 700 ms
    Then the events are "front long-hold"

  Scenario: The power button's press arrives already classified
    When the power button records a short press
    And one poll passes
    Then the events are "power click"

  Scenario: The power button's latch drains exactly once
    When the power button records a short press
    And one second passes
    Then the events are "power click"

  Scenario: All three buttons can act at once
    When the front button is held for 40 ms
    And the side button is held for 40 ms
    And both levelled buttons are released for 20 ms
    And the power button records a short press
    And one poll passes
    Then the events are "side click, power click"
