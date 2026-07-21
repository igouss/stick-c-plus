Feature: The board reads its own orientation from gravity
  As someone holding the M5StickC Plus
  I want the screen to show which way the controller is pointing
  So that I can see its orientation in space without guessing from the numbers

  Scenario: A board lying on its back reads level and screen up
    When the accelerometer reads 0, 0, 1000 milli-g
    Then the facing is ScreenUp
    And the pitch is 0 degrees
    And the roll is 0 degrees

  Scenario: Standing the stick on its USB port pitches it a quarter turn
    When the accelerometer reads -1000, 0, 0 milli-g
    Then the facing is Upright
    And the pitch is 90 degrees

  Scenario: Turning the board face down is named, not confused with face up
    When the accelerometer reads 0, 0, -1000 milli-g
    Then the facing is ScreenDown

  Scenario: Rolling onto an edge is a signed quarter turn
    When the accelerometer reads 0, 1000, 0 milli-g
    Then the facing is LeftEdge
    And the roll is 90 degrees

  Scenario: Rolling onto the other edge rolls the other way
    When the accelerometer reads 0, -1000, 0 milli-g
    Then the facing is RightEdge
    And the roll is -90 degrees

  Scenario: A board held on a corner names no face rather than guessing one
    When the accelerometer reads 0, 707, 707 milli-g
    Then the facing is Tilted
    And the roll is 45 degrees

  Scenario: A board being moved reports that, instead of a pose it cannot trust
    When the accelerometer reads 0, 0, 2000 milli-g
    Then the facing is Moving

  Scenario: The raw reading is kept beside the verdict, for the axis bars to draw
    When the accelerometer reads -342, 0, 940 milli-g
    Then the facing is ScreenUp
    And the reading is still -342, 0, 940 milli-g

  Scenario: The very first sample is shown whole, so boot shows the truth at once
    Given a smoothed readout
    When the accelerometer reads 0, 0, 1000 milli-g
    Then the facing is ScreenUp
    And the reading is still 0, 0, 1000 milli-g

  Scenario: A smoothed readout catches up with a turn within its responsiveness budget
    Given a smoothed readout
    When the accelerometer reads 0, 0, 1000 milli-g
    And the accelerometer reads -1000, 0, 0 milli-g 7 times
    Then the facing is Upright
    And the pitch is 90 degrees

  Scenario: A board being shaken says so, rather than naming a pose it cannot trust
    Given a smoothed readout
    When the accelerometer reads 0, 0, 1000 milli-g
    And the accelerometer reads 0, 0, 4000 milli-g
    Then the facing is Moving

  Scenario: A spike is damped on the way in, so a knock never swings the readout wildly
    Given a smoothed readout
    When the accelerometer reads 0, 0, 1000 milli-g
    And the accelerometer reads 0, 0, 4000 milli-g
    Then the reading is still 0, 0, 2500 milli-g

  Scenario: Once the shaking stops the readout returns to naming the pose
    Given a smoothed readout
    When the accelerometer reads 0, 0, 1000 milli-g
    And the accelerometer reads 0, 0, 4000 milli-g
    And the accelerometer reads 0, 0, 1000 milli-g 7 times
    Then the facing is ScreenUp
    And the pitch is 0 degrees
