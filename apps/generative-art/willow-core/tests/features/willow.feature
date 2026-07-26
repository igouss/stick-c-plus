Feature: The willow curtain

  The curtain is a pure function of a sway phase: a fixed set of hanging tendrils that sway as the
  wind advances and return to their start exactly once every full breath. These scenarios guard
  those domain rules at the boundary; the numeric fidelity of the sway — that a point tracks its
  reference — lives in the property tests next to the code.

  Scenario: the curtain hangs a whole set of tendrils
    Given the willow clock at 0 milliseconds
    Then the curtain hangs 24 tendrils

  Scenario: the curtain sways as the wind blows
    Given the willow clock at 0 milliseconds
    When the curtain is captured
    And the wind blows on
    And the curtain is captured
    Then the two captured curtains are different pictures

  Scenario: a full breath returns the curtain to its start
    Given the willow clock at 0 milliseconds
    When the curtain is captured
    And a full breath passes
    And the curtain is captured
    Then the two captured curtains are the same picture
