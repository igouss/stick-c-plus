Feature: The alarm — given the verdict, should the buzzer be making a noise right now?

  The boundary this suite guards: the alarm chirps on a cadence, an acknowledgement
  silences the sound and nothing else, and — the thing most likely to be wrong — the
  same still-present fault stays silent after acknowledgement even when a higher-ranked
  fault masked it in between. The fine grain lives in the unit and property tests next
  to the code; here we pin the scenarios a person would describe out loud.

  Background:
    Given a fresh alarm and a 5000 ms cadence

  Scenario: A healthy board never sounds
    When the alarm steps against a healthy verdict at tick 0 ms
    Then no chirp sounds
    And the alarm is silent

  Scenario: A new fault chirps immediately, not one cadence later
    When the alarm steps against a stall on "display" at tick 0 ms
    Then a chirp sounds
    And the alarm is sounding

  Scenario: The same fault does not chirp again before its cadence is due
    Given the alarm is sounding a stall on "display" since tick 0 ms
    When the alarm steps against a stall on "display" at tick 2000 ms
    Then no chirp sounds
    And the alarm is sounding

  Scenario: The same fault chirps again once its cadence has passed
    Given the alarm is sounding a stall on "display" since tick 0 ms
    When the alarm steps against a stall on "display" at tick 5000 ms
    Then a chirp sounds

  Scenario: Acknowledging a sounding alarm silences it
    Given the alarm is sounding a stall on "display" since tick 0 ms
    When the alarm is acknowledged
    Then the alarm is acknowledged
    And no chirp sounds

  Scenario: The same still-present fault stays silent after acknowledgement
    Given the alarm is sounding a stall on "display" since tick 0 ms
    And the alarm is acknowledged
    When the alarm steps against a stall on "display" at tick 20000 ms
    Then no chirp sounds
    And the alarm is acknowledged

  Scenario: A different fault sounds again after the first one was acknowledged
    Given the alarm is sounding a stall on "display" since tick 0 ms
    And the alarm is acknowledged
    When the alarm steps against a crash at tick 1000 ms
    Then a chirp sounds
    And the alarm is sounding

  Scenario: Recovery clears the acknowledgement, so a fault that returns sounds again
    Given the alarm is sounding a stall on "display" since tick 0 ms
    And the alarm is acknowledged
    When the alarm steps against a healthy verdict at tick 1000 ms
    And the alarm steps against a stall on "display" at tick 2000 ms
    Then a chirp sounds
    And the alarm is sounding

  Scenario: An acknowledgement survives being outranked, masked, and then unmasked
    Given the alarm is sounding a stall on "display" since tick 0 ms
    And the alarm is acknowledged
    When the alarm steps against a crash at tick 1000 ms
    And the alarm is acknowledged
    And the alarm steps against a stall on "display" at tick 2000 ms
    Then no chirp sounds
    And the alarm is acknowledged

  Scenario: An unacknowledged stall that resurfaces after being masked does chirp
    Given the alarm is sounding a stall on "display" since tick 0 ms
    When the alarm steps against a crash at tick 1000 ms
    And the alarm steps against a stall on "display" at tick 2000 ms
    Then a chirp sounds
    And the alarm is sounding
