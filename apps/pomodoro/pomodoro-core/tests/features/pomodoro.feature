Feature: The pomodoro timer counts focus and break intervals
  As someone using the M5StickC Plus as a pomodoro timer
  I want focus and break phases driven by the two buttons and a clock
  So that I work in focused intervals and rest between them

  Background:
    Given durations of 1000 ms focus, 500 ms short break, 1500 ms long break, long every 4

  Scenario: A fresh timer shows a full focus and does not count down
    Then the phase is Focus
    And the status is Idle
    And 1000 ms remain at 900 ms

  Scenario: Starting runs the countdown and sounds the focus jingle
    When the front button is tapped at 0 ms
    Then the status is Running
    And a FocusStart jingle sounds
    And 600 ms remain at 400 ms

  Scenario: Pausing then resuming preserves the remaining time
    When the front button is tapped at 0 ms
    And the front button is tapped at 300 ms
    Then the status is Paused
    And 700 ms remain at 5000 ms
    When the front button is tapped at 5000 ms
    Then the status is Running
    And 700 ms remain at 5000 ms

  Scenario: A focus reaching zero finishes and sounds the completion jingle
    When the front button is tapped at 0 ms
    And the clock ticks at 1000 ms
    Then the status is Finished
    And a PhaseComplete jingle sounds
    And 0 ms remain at 1000 ms

  Scenario: After a finished focus, starting begins a short break
    When the front button is tapped at 0 ms
    And the clock ticks at 1000 ms
    And the front button is tapped at 2000 ms
    Then the phase is ShortBreak
    And a BreakStart jingle sounds

  Scenario: Skipping jumps to the next phase at once
    When the front button is tapped at 0 ms
    And the side button is tapped at 300 ms
    Then the phase is ShortBreak
    And a BreakStart jingle sounds

  Scenario: Skipping an unstarted first focus is a short break, not a long one
    When the side button is tapped at 0 ms
    Then the phase is ShortBreak
    And a BreakStart jingle sounds

  Scenario: Holding resets the current phase to full
    When the front button is tapped at 0 ms
    And the front button is held at 400 ms
    Then the phase is Focus
    And the status is Paused
    And 1000 ms remain at 400 ms
