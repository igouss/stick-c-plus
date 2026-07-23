Feature: current is a pure selection over injected elapsed time
  As the buddy display layer
  I want the current sprite and frame chosen purely from an injected elapsed_ms
  So that variety varies deterministically and frame timing stays the sprite's own job

  Scenario: At the start of a state the first sprite in its set plays
    Given the claudepix creature
    When current is selected for Idle at 0 ms
    Then the chosen sprite is idle_breathe

  Scenario: The selection is deterministic in elapsed time
    Given the claudepix creature
    When current is selected twice for Idle at 5000 ms
    Then both selections are identical

  Scenario: Frame timing is delegated to the chosen sprite's own clock
    Given the claudepix creature
    When current is selected for Sleep at 1234 ms
    Then the reported frame index equals the chosen sprite's own frame_at
