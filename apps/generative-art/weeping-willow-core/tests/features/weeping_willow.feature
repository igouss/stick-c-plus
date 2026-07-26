Feature: The weeping willow tree

  The tree is a pure function of a sway phase: a fixed frame of wood carrying a canopy of fronds that
  stream as the wind advances and return to their start exactly once every full breath. These
  scenarios guard those domain rules at the boundary; the numeric fidelity of the sway — that a frond
  point tracks its reference — lives in the property tests next to the code.

  Scenario: the tree stands on wood
    Given the willow clock at 0 milliseconds
    Then the tree stands on some wood

  Scenario: the tree hangs a whole canopy of fronds
    Given the willow clock at 0 milliseconds
    Then the canopy hangs 35 fronds

  Scenario: the canopy sways as the wind blows
    Given the willow clock at 0 milliseconds
    When the canopy is captured
    And the wind blows on
    And the canopy is captured
    Then the two captured canopies are different pictures

  Scenario: a full breath returns the canopy to its start
    Given the willow clock at 0 milliseconds
    When the canopy is captured
    And a full breath passes
    And the canopy is captured
    Then the two captured canopies are the same picture

  Scenario: fireflies drift through the scene
    Given the willow clock at 0 milliseconds
    When the swarm is captured
    And the wind blows on
    And the swarm is captured
    Then the two captured swarms are different pictures

  Scenario: a full breath returns the swarm to its start
    Given the willow clock at 0 milliseconds
    When the swarm is captured
    And a full breath passes
    And the swarm is captured
    Then the two captured swarms are the same picture
