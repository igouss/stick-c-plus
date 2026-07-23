Feature: The buddy wears a persona derived from its sessions, sensors, and clock
  As the owner of the M5StickC Plus desk pet
  I want the creature's mood to follow my Claude Code sessions and my handling of it
  So that a glance at the glass tells me what is happening

  Scenario: A fresh buddy sleeps through its wake window, then wakes idle
    Given a fresh buddy
    When the buddy steps at 0 ms
    Then the persona is Sleep
    When the buddy steps at 20000 ms
    Then the persona is Idle

  Scenario: A single waiting session shows attention and passes the wake window
    Given a fresh buddy
    And 1 sessions are waiting
    When the buddy steps at 0 ms
    Then the persona is Attention

  Scenario: Two running sessions are not yet busy
    Given a fresh buddy
    And 2 sessions are running
    When the buddy steps at 0 ms
    And the buddy steps at 20000 ms
    Then the persona is Idle

  Scenario: Three running sessions are busy
    Given a fresh buddy
    And 3 sessions are running
    When the buddy steps at 0 ms
    Then the persona is Busy

  Scenario: A completed session celebrates even while others run
    Given a fresh buddy
    And 5 sessions are running
    And a session just completed
    When the buddy steps at 0 ms
    Then the persona is Celebrate

  Scenario: A disconnected bridge is idle, never busy
    Given a fresh buddy
    And the bridge is disconnected
    And 5 sessions are running
    When the buddy steps at 0 ms
    And the buddy steps at 20000 ms
    Then the persona is Idle

  Scenario: A level-up celebrates over the idle base
    Given a fresh buddy
    When the buddy steps at 0 ms
    And a level-up occurs at 20000 ms
    Then the persona is Celebrate

  Scenario: A shake makes the buddy dizzy
    Given a fresh buddy
    When the buddy steps at 0 ms
    And the buddy is shaken at 100 ms
    Then the persona is Dizzy

  Scenario: A fast approval fills the buddy with a heart
    Given a fresh buddy
    When the buddy steps at 0 ms
    And an approval is answered in 2 s at 20000 ms
    Then the persona is Heart

  Scenario: A slow approval fires no heart
    Given a fresh buddy
    When the buddy steps at 0 ms
    And an approval is answered in 5 s at 20000 ms
    Then the persona is Idle

  Scenario: The charging clock writes the persona directly, suppressing attention
    Given a fresh buddy
    And 1 sessions are waiting
    When the charging clock says Sleep at 0 ms
    Then the persona is Sleep

  Scenario: A front long-hold opens the settings menu
    Given a fresh buddy
    When the front button is long-held at 0 ms
    Then the menu is open
