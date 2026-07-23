Feature: The buddy's self-sensors carry two load-bearing quirks
  As the owner of the desk pet
  I want the shake and nap sensors to behave exactly as upstream does
  So that the port matches the creature people already know

  # Preserved quirk 1: the shake EMA baseline goes STALE while the menu is open (upstream
  # short-circuits the detector out entirely), so it never advances across those frames. The
  # two scenarios below feed an identical final tilt; only the middle frame's menu state
  # differs, and that alone flips the persona — proof the baseline did not advance under the
  # open menu.

  Scenario: A stale baseline still fires a shake after the menu closes
    Given a fresh buddy
    When the buddy steps at 0 ms
    And the menu is held open under a steady tilt at 13000 ms
    And the buddy is tilted the same amount with the menu closed at 15001 ms
    Then the persona is Dizzy

  Scenario: A baseline that tracked the same tilt no longer fires it
    Given a fresh buddy
    When the buddy steps at 0 ms
    And the buddy is tilted the same amount with the menu closed at 13000 ms
    And the buddy is tilted the same amount with the menu closed at 15001 ms
    Then the persona is Idle

  # Preserved quirk 2: an unanswered prompt FREEZES the nap counter (the IMU is not read), but
  # the enter/leave transition check sits OUTSIDE that freeze. So a prompt stops the counter
  # from latching a new nap, yet a nap already latched stays latched right through the prompt.

  Scenario: The frozen counter cannot latch a nap while a prompt is unanswered
    Given a fresh buddy
    When the buddy lies face-down under an unanswered prompt at 0 ms
    And the buddy lies face-down under an unanswered prompt at 16 ms
    And the buddy lies face-down under an unanswered prompt at 32 ms
    And the buddy lies face-down under an unanswered prompt at 48 ms
    And the buddy lies face-down under an unanswered prompt at 64 ms
    And the buddy lies face-down under an unanswered prompt at 80 ms
    And the buddy lies face-down under an unanswered prompt at 96 ms
    And the buddy lies face-down under an unanswered prompt at 112 ms
    And the buddy lies face-down under an unanswered prompt at 128 ms
    And the buddy lies face-down under an unanswered prompt at 144 ms
    And the buddy lies face-down under an unanswered prompt at 160 ms
    And the buddy lies face-down under an unanswered prompt at 176 ms
    And the buddy lies face-down under an unanswered prompt at 192 ms
    And the buddy lies face-down under an unanswered prompt at 208 ms
    And the buddy lies face-down under an unanswered prompt at 224 ms
    Then the buddy is not napping

  Scenario: A latched nap survives an unanswered prompt because the counter is frozen
    Given a fresh buddy
    When the buddy lies face-down at 0 ms
    And the buddy lies face-down at 16 ms
    And the buddy lies face-down at 32 ms
    And the buddy lies face-down at 48 ms
    And the buddy lies face-down at 64 ms
    And the buddy lies face-down at 80 ms
    And the buddy lies face-down at 96 ms
    And the buddy lies face-down at 112 ms
    And the buddy lies face-down at 128 ms
    And the buddy lies face-down at 144 ms
    And the buddy lies face-down at 160 ms
    And the buddy lies face-down at 176 ms
    And the buddy lies face-down at 192 ms
    And the buddy lies face-down at 208 ms
    And the buddy lies face-down at 224 ms
    Then the buddy is napping
    When the buddy lies face-up under an unanswered prompt at 240 ms
    And the buddy lies face-up under an unanswered prompt at 256 ms
    And the buddy lies face-up under an unanswered prompt at 272 ms
    Then the buddy is still napping
