Feature: Which way up the picture is drawn

  The glass is the same glass for every app on this board, and it turns the same way for all
  of them — so which quarter turn puts the picture's top toward the sky is the board's
  question, not any one app's.

  A different question from which face the board is resting on: a board lying flat rests on a
  perfectly nameable face and has no readable rotation at all. Naming the face stays with the
  app that cares (see orientation-core); these scenarios are only about the turn.

  Rotation 0 is the panel's native landscape, which is legible with the stick held
  horizontally, USB-C to the right. That is a measured fact about how the panel is mounted,
  not a convention — see the rotation table's own note.

  Scenario: The picture starts in the panel's native landscape
    Then the picture is drawn at 0 degrees

  Scenario: Holding the stick in its natural reading position asks for no rotation
    When the accelerometer reads 0, -1000, 0 milli-g
    And 250 milliseconds pass
    Then the picture is drawn at 0 degrees

  Scenario: Standing the stick on its USB port turns the picture to stay readable
    When the accelerometer reads 1000, 0, 0 milli-g
    And 250 milliseconds pass
    Then the picture is drawn at 270 degrees

  Scenario: Turning the stick the other way turns the picture the other way
    When the accelerometer reads -1000, 0, 0 milli-g
    And 250 milliseconds pass
    Then the picture is drawn at 90 degrees

  Scenario: A new position is not obeyed until it has been held
    When the accelerometer reads 1000, 0, 0 milli-g
    And 100 milliseconds pass
    Then the picture is drawn at 0 degrees

  Scenario: A board resting on a corner keeps its rotation rather than flipping about
    When the accelerometer reads 707, 707, 0 milli-g
    And 5000 milliseconds pass
    Then the picture is drawn at 0 degrees
