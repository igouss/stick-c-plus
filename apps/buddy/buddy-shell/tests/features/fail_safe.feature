Feature: Only a real snapshot may clear a pending prompt
  As the owner of a stick that gates Claude Code
  I want keepalive traffic to leave a pending decision exactly where it is
  So that a routine heartbeat can never wipe a question I was about to answer

  Scenario: A transcript event does not disturb the prompt
    Given a booted stick
    And a snapshot arrives with prompt "p1" for Bash
    When a transcript event arrives
    Then the prompt for "p1" is still on the glass

  Scenario: A time sync does not disturb the prompt
    Given a booted stick
    And a snapshot arrives with prompt "p1" for Bash
    When a time sync arrives
    Then the prompt for "p1" is still on the glass

  Scenario: A command does not disturb the prompt
    Given a booted stick
    And a snapshot arrives with prompt "p1" for Bash
    When a status command arrives
    Then the prompt for "p1" is still on the glass

  Scenario: A whole run of keepalives does not disturb the prompt
    Given a booted stick
    And a snapshot arrives with prompt "p1" for Bash
    When a transcript event arrives
    And a time sync arrives
    And a status command arrives
    And a transcript event arrives
    Then the prompt for "p1" is still on the glass
    And the front button is clicked
    And the answer sent is "once" for prompt "p1"

  Scenario: A real snapshot with no prompt does clear it
    Given a booted stick
    And a snapshot arrives with prompt "p1" for Bash
    When an empty snapshot arrives
    Then no prompt is on the glass

  Scenario: The link lapses when the host stops talking
    Given a booted stick
    When a transcript event arrives
    Then the link is up
    When 30 seconds pass
    Then the link is down
