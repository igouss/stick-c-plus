Feature: The claudepix creature binds each persona state onto exact vendored presets
  As the buddy display layer
  I want each persona state to name the exact sprite set that plays for it
  So that a wrong preset, a wrong order, or an empty set fails a test rather than the glass

  Scenario Outline: Each state maps to its exact sprite set, in order
    Given the claudepix creature
    When the sprite set for <state> is queried
    Then the set is exactly <slugs>

    Examples:
      | state     | slugs                                      |
      | Sleep     | expression_sleep                           |
      | Idle      | idle_breathe, idle_blink, idle_look_around |
      | Busy      | work_coding, work_think                    |
      | Attention | expression_surprise                        |
      | Celebrate | dance_bounce, dance_sway                   |
      | Dizzy     | dance_djmix                                |
      | Heart     | expression_wink                            |

  Scenario Outline: Every state offers a non-empty set
    Given the claudepix creature
    When the sprite set for <state> is queried
    Then the set is not empty

    Examples:
      | state     |
      | Sleep     |
      | Idle      |
      | Busy      |
      | Attention |
      | Celebrate |
      | Dizzy     |
      | Heart     |
