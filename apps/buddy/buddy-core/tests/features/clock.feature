Feature: The charging clock tells the time of day as a mood
  As the owner of a docked, idle desk pet
  I want the creature's mood to track the wall clock
  So that a glance tells me roughly what time it is

  # Defect (d): upstream ordered `h < 9` above the late-night branch, so midnight (hour 0)
  # was shadowed into the pre-9am flicker as dead code and could never reach Dizzy. The port
  # hoists the late-night branch above `h < 9`. These scenarios pin the fixed order.

  Scenario: Midnight reaches the late-night Dizzy the author intended
    Given a fresh buddy
    When the charging clock is read at hour 0 on day 3 at 0 ms
    Then the persona is Dizzy

  Scenario: The rest of the midnight flicker window is Sleep
    Given a fresh buddy
    When the charging clock is read at hour 0 on day 3 at 7000 ms
    Then the persona is Sleep

  Scenario: Late evening flickers the same Dizzy as midnight
    Given a fresh buddy
    When the charging clock is read at hour 23 on day 3 at 0 ms
    Then the persona is Dizzy

  Scenario: A pre-9am weekday hour flickers Idle against Sleep, never Dizzy
    Given a fresh buddy
    When the charging clock is read at hour 8 on day 3 at 0 ms
    Then the persona is Idle

  Scenario: Early morning is an unconditional Sleep whatever the phase
    Given a fresh buddy
    When the charging clock is read at hour 3 on day 3 at 999999 ms
    Then the persona is Sleep
