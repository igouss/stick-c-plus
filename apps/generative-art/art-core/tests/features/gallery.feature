Feature: The gallery running order

  The gallery shows one sketch at a time and a button press advances to the next, wrapping
  after the last piece back to the first. These scenarios guard that running order at the
  boundary; which pixels each sketch draws lives in the display crate and its screenshots.

  Scenario: the gallery opens on the plume
    Given a fresh gallery
    Then the sketch on the glass is the plume

  Scenario: a press advances to the next sketch
    Given a fresh gallery
    When the button is pressed 1 time
    Then the sketch on the glass is the squares

  Scenario: the running order wraps after the last piece
    Given a fresh gallery
    When the button is pressed 5 times
    Then the sketch on the glass is the plume

  Scenario: every piece is reachable by pressing the button
    Given a fresh gallery
    When the button is pressed 3 times
    Then the sketch on the glass is the orbits
