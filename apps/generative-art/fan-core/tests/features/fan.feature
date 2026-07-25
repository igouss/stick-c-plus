Feature: The fan

  The fan is a pure function of an animation phase: a fixed grid of folding triangles that fold
  open and closed as the phase advances and return to their start exactly once every full turn.
  These scenarios guard those domain rules at the boundary; the numeric fidelity of the port lives
  in the property tests next to the code.

  Scenario: the fan is the whole grid of triangles
    Given the fan clock at 0 milliseconds
    Then the fan is made of 302 triangles

  Scenario: the fan folds as the phase advances
    Given the fan clock at 0 milliseconds
    When the fan is captured
    And a quarter period passes
    And the fan is captured
    Then the two captured fans are different pictures

  Scenario: the animation repeats every full turn
    Given the fan clock at 0 milliseconds
    When the phase is captured
    And a full period passes
    And the phase is captured
    Then the two captured phases are equal
