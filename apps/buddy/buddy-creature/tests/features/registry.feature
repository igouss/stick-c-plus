Feature: The species registry resolves a persisted index and holds a stable order
  As the buddy display layer
  I want a persisted species index to resolve to a built-in creature in a fixed order
  So that the wire + NVS index stays a stable compatibility surface and never panics

  Scenario: The first registry slot resolves to the first creature
    When species index 0 is resolved
    Then resolution yields the claudepix creature

  Scenario: An out-of-range index resolves to nothing
    When species index 200 is resolved
    Then resolution yields nothing

  Scenario: The GIF sentinel resolves to nothing, not a built-in
    When the GIF sentinel index is resolved
    Then resolution yields nothing

  Scenario: The registry holds one creature, claudepix first
    Then the registry holds 1 creature
    And registry entry 0 is claudepix
