Feature: The compositor draws one screen and at most one overlay, in a fixed order
  As the buddy's glass
  I want the passkey takeover to outrank everything and the innermost overlay to win
  So that the owner is never shown a menu instead of the question it was asking

  Scenario: The home screen is the resting picture
    Given a busy buddy at home
    When the glass is painted
    Then the glass is not blank
    And nothing escapes the canvas

  Scenario Outline: Every screen paints, and nothing runs off the glass
    Given a busy buddy at home
    And the screen is <screen>
    When the glass is painted
    Then the glass is not blank
    And nothing escapes the canvas

    Examples:
      | screen |
      | home   |
      | pet    |
      | info   |
      | clock  |

  Scenario: A passkey takes over the whole glass
    Given a busy buddy at home
    And the passkey 482913 is active
    When the glass is painted
    And the screen is clock
    And the overlay is reset
    And the glass is painted again
    Then both paintings are identical

  Scenario: An overlay changes the picture it is drawn over
    Given a busy buddy at home
    When the glass is painted
    And the overlay is reset
    And the glass is painted again
    Then the two paintings differ

  Scenario: A quarter turn draws on the taller canvas
    Given a busy buddy at home
    When the glass is painted
    And the board is turned a quarter
    And the glass is painted again
    Then the two paintings have different shapes
    And nothing escapes the canvas
