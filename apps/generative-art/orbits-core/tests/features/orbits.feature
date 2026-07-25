Feature: The orbits field

  The field is a pure function of a virtual frame: thirty diamond blooms that sweep the canvas as a
  comet, each cell as bright as the nearest bloom reaches it, grained by a fixed texture. These
  scenarios guard those domain rules at the boundary; the numeric fidelity of the port — that the
  triangle wave is the transcendental and the bloom is the source's — lives in the property tests
  next to the code.

  Scenario: the field is thirty orbits deep
    Given the orbits clock at 0 milliseconds
    Then the field has 30 orbits

  Scenario: a bloom peaks on an orbit centre
    Given the orbits clock at 700 milliseconds
    Then a bloom peaks on an orbit centre

  Scenario: the comet drifts as the frame advances
    Given the orbits clock at 0 milliseconds
    When the field is captured
    And the comet drifts on
    And the field is captured
    Then the two captured fields are different pictures
