Feature: The squares grid

  The grid is a pure function of an animation phase: a fixed set of cells that breathe as the
  phase advances and return to their start exactly once every full turn. These scenarios guard
  those domain rules at the boundary; the numeric fidelity of the port lives in the property
  tests next to the code.

  Scenario: the grid is the whole set of cells
    Given the squares clock at 0 milliseconds
    Then the grid is made of 45 cells

  Scenario: the grid breathes as the phase advances
    Given the squares clock at 0 milliseconds
    When the grid is captured
    And a quarter period passes
    And the grid is captured
    Then the two captured grids are different pictures

  Scenario: the animation repeats every full turn
    Given the squares clock at 0 milliseconds
    When the phase is captured
    And a full period passes
    And the phase is captured
    Then the two captured phases are equal
