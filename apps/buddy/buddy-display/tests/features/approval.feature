Feature: A pending prompt replaces the transcript band and counts up
  As the owner of a stick that gates tool calls
  I want the pending question, its age, and which button answers it
  So that I can answer without reading anything else on the glass

  Scenario: A pending prompt replaces the band
    Given a busy buddy at home
    When the glass is painted
    And a prompt for Bash arrives
    And the glass is painted again
    Then the two paintings differ

  Scenario: Each second reaches the glass
    Given a busy buddy at home
    And a prompt for Bash arrives
    And the prompt has waited 3 seconds
    When the glass is painted
    And the prompt has waited 4 seconds
    And the glass is painted again
    Then the two paintings differ

  Scenario: A ticking counter does not restart the creature's animation
    Given a busy buddy at home
    And a prompt for Bash arrives
    And the prompt has waited 3 seconds
    When the animation anchor is remembered
    And the prompt has waited 4 seconds
    Then the animation anchor is unchanged

  Scenario: A persona change does restart the creature's animation
    Given a busy buddy at home
    When the animation anchor is remembered
    And the persona becomes Attention
    Then the animation anchor has changed
