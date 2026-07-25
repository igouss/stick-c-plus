Feature: The plume field

  The frond is a pure function of an animation phase: a fixed cloud of points in canvas
  space that breathes as the phase advances and repeats exactly once every full turn. These
  scenarios guard those domain rules at the boundary; the numeric fidelity of the port lives
  in the property tests next to the code.

  Scenario: the frond is the subsampled point budget
    Given the plume clock at 0 milliseconds
    Then the frond is made of 6000 points

  Scenario: a well-behaved point sits on the canvas
    Given the plume clock at 0 milliseconds
    Then the point at index 5000 is finite and within the 400 by 400 canvas

  Scenario: the frond breathes as the phase advances
    Given the plume clock at 0 milliseconds
    When the frond is captured
    And a frame passes
    And the frond is captured
    Then the two captured fronds are different pictures

  Scenario: the animation repeats every full turn
    Given the plume clock at 0 milliseconds
    When the phase is captured
    And a full period passes
    And the phase is captured
    Then the two captured phases are equal
