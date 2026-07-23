Feature: The bridge decides how to find, connect, pair, recover, and split writes

  The pure policy the driving-adapter runs: the pairing/connection FSM turns each
  transport outcome into the next action, and the MTU chunker splits a framed line to
  fit the negotiated link. These scenarios guard the decision seam end to end; the fine
  grain lives in the unit and property tests beside the code.

  The governing rule is "bond once, and forever": pairing is the only step needing a
  human at the glass inside a thirty-second window, so the machine gives a bond up only
  on conclusive evidence and never merely because the stick is switched off.

  Scenario: Every attempt begins by looking for the stick
    Given a bridge that has found the stick
    Then the bridge decides to connect

  Scenario: A stick that is not advertising is waited for, not given up on
    Given a bridge that has started looking for the stick
    When the scan window closes with nothing found
    Then the bridge decides to wait and try again
    And the bridge keeps the bond

  Scenario: A fresh connection pairs
    Given a bridge that has found the stick
    When the transport reports a fresh (unpaired) connection
    Then the bridge decides to pair

  Scenario: An already-paired connection skips pairing
    Given a bridge that has found the stick
    When the transport reports an already-paired connection
    Then the bridge decides to subscribe

  Scenario: One encryption failure is not enough to give up a bond
    Given a bridge that has connected to a bonded stick
    When the transport reports an encryption failure
    Then the bridge decides to wait and try again
    And the bridge keeps the bond

  Scenario: A persistently stale bond is recovered, not retried forever
    Given a bridge that has connected to a bonded stick
    When the transport reports an encryption failure
    And the link fails to encrypt
    And the link fails to encrypt
    Then the bridge decides to remove the device and re-acquire

  Scenario: Recovering a bond means finding the device again
    Given a bridge that has connected to a bonded stick
    When the transport reports an encryption failure
    And the link fails to encrypt
    And the link fails to encrypt
    And the stale bond has been removed
    Then the bridge decides to look for the stick

  Scenario: A missing pairing agent is fatal
    Given a bridge that has started looking for the stick
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
