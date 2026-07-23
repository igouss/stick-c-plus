Feature: The bridge decides how to connect, pair, recover, and split writes

  The pure policy the driving-adapter runs: the pairing/connection FSM turns each
  transport outcome into the next action, and the MTU chunker splits a framed line to
  fit the negotiated link. These scenarios guard the decision seam end to end; the fine
  grain lives in the unit and property tests beside the code.

  Scenario: A fresh connection pairs
    Given a bridge that has started connecting
    When the transport reports a fresh (unpaired) connection
    Then the bridge decides to pair

  Scenario: An already-paired connection skips pairing
    Given a bridge that has started connecting
    When the transport reports an already-paired connection
    Then the bridge decides to subscribe

  Scenario: A stale bond is recovered, not retried in place
    Given a bridge that has started connecting
    When the transport reports an encryption failure
    Then the bridge decides to remove the device and re-acquire

  Scenario: A missing pairing agent is fatal
    Given a bridge that has started connecting
    When the transport reports a missing agent
    Then the bridge decides to fail fast

  Scenario: An empty payload splits into no writes
    When a payload of 0 bytes is chunked for an MTU of 517
    Then it splits into 0 pieces

  Scenario: A payload within one piece is a single write
    When a payload of 20 bytes is chunked for an MTU of 23
    Then it splits into 1 piece

  Scenario: A payload over the piece size splits into many
    When a payload of 41 bytes is chunked for an MTU of 23
    Then it splits into 3 pieces
