Feature: The transcript HUD shows the newest activity and how much is behind it
  As the owner glancing at a desk pet
  I want the newest line bright, the older ones dim, and a hint of what I am not seeing
  So that a full-looking band is not mistaken for the whole story

  Scenario: A quiet transcript says so rather than leaving the band blank
    Given a busy buddy at home
    And the transcript is empty
    When the glass is painted
    Then the glass is not blank

  Scenario: A new entry changes the picture
    Given a busy buddy at home
    And the transcript holds "read the bead"
    When the glass is painted
    And the transcript holds "read the bead" and "wrote the crate"
    And the glass is painted again
    Then the two paintings differ

  Scenario: A long entry wraps rather than running off the glass
    Given a busy buddy at home
    And the transcript holds a very long entry
    When the glass is painted
    Then nothing escapes the canvas

  Scenario: A deep transcript is distinguishable from a shallow one
    Given a busy buddy at home
    And the transcript holds "older" and "newer"
    When the glass is painted
    And the transcript holds nine more entries behind them
    And the glass is painted again
    Then the two paintings differ
