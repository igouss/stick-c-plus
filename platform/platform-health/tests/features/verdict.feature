Feature: The board's verdict — is anything broken, and what?

  The boundary this suite guards: given what every registered component last did and
  when, plus how the board booted and what its heap looks like, `assess` names at most
  one thing wrong. The fine grain — every rank, every tie-break, every boundary — lives
  in the unit and property tests next to the code; here we pin the behaviour a person
  would describe out loud.

  Scenario: A board nobody registered anything on says nothing is wrong
    Given the board booted cleanly
    When the board is assessed at tick 1000 ms
    Then the verdict is healthy

  Scenario: One component beating on time is healthy
    Given the board booted cleanly
    And a component "display" with a 2000 ms deadline that last beat 900 ms ago
    When the board is assessed at tick 1000 ms
    Then the verdict is healthy

  Scenario: Three components, all beating on time, are healthy
    Given the board booted cleanly
    And a component "display" with a 2000 ms deadline that last beat 900 ms ago
    And a component "sensors" with a 5000 ms deadline that last beat 500 ms ago
    And a component "network" with a 10000 ms deadline that last beat 1000 ms ago
    When the board is assessed at tick 1000 ms
    Then the verdict is healthy

  Scenario: A component silent past its deadline is named as stalled
    Given the board booted cleanly
    And a component "display" with a 2000 ms deadline that last beat 5000 ms ago
    When the board is assessed at tick 5000 ms
    Then the verdict names "display" as stalled for 5000 ms

  Scenario: A component that never once beat is stalled from the moment it was assessed
    Given the board booted cleanly
    And a component "display" with a 2000 ms deadline that has never beaten
    When the board is assessed at tick 5000 ms
    Then the verdict names "display" as stalled for 5000 ms

  Scenario: A component that concluded its own fault is reported, not stalled
    Given the board booted cleanly
    And a component "sensors" reported giving up 4000 ms ago
    When the board is assessed at tick 5000 ms
    Then the verdict names "sensors" as reported for 4000 ms

  Scenario: A crashed boot is the verdict, naming the cause
    Given the board booted crashed by a task watchdog
    When the board is assessed at tick 7000 ms
    Then the verdict names the board as crashed by a task watchdog

  Scenario: Free heap under the floor is starvation, naming the level
    Given the board booted cleanly
    And the board reports 4000 free bytes against a 12000 byte floor
    When the board is assessed at tick 1000 ms
    Then the verdict names the board as starved with 4000 free bytes against a 12000 byte floor

  Scenario: A board that has not read its heap yet is not starving
    Given the board booted cleanly
    And the board has not read its heap
    When the board is assessed at tick 1000 ms
    Then the verdict is healthy

  Scenario: A crash outranks a stall it probably caused
    Given the board booted crashed by a panic
    And a component "display" with a 2000 ms deadline that has never beaten
    When the board is assessed at tick 5000 ms
    Then the verdict names the board as crashed by a panic

  Scenario: Starvation outranks a component reporting it gave up
    Given the board booted cleanly
    And the board reports 4000 free bytes against a 12000 byte floor
    And a component "sensors" reported giving up 1000 ms ago
    When the board is assessed at tick 5000 ms
    Then the verdict names the board as starved with 4000 free bytes against a 12000 byte floor

  Scenario: A component reporting it gave up outranks an inferred stall
    Given the board booted cleanly
    And a component "sensors" reported giving up 1000 ms ago
    And a component "display" with a 2000 ms deadline that has never beaten
    When the board is assessed at tick 5000 ms
    Then the verdict names "sensors" as reported for 1000 ms

  Scenario: Between two equally-stalled components, the earlier registration wins the tie
    Given the board booted cleanly
    And a component "first" with a 2000 ms deadline that has never beaten
    And a component "second" with a 2000 ms deadline that has never beaten
    When the board is assessed at tick 5000 ms
    Then the verdict names "first" as stalled for 5000 ms
