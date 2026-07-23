Feature: Session to snapshot synthesis
  There is no status IPC surface to query the agent; the heartbeat is reconstructed from
  the stream of already-typed session events, keyed by session_id, folded into the
  SnapshotPacket the device expects: total = distinct sessions, running = started-not-stopped,
  waiting = sessions with an outstanding prompt. Tokens are credited through buddy_core's
  TokenLatch so a daemon restart / device reboot never double-counts.

  # Invariant 4 — reconstruction over zero / one / many sessions.

  Scenario: No sessions synthesize to zero counts
    Given a fresh ledger
    When the ledger synthesizes a snapshot
    Then the snapshot reports 0 total, 0 running, 0 waiting

  Scenario: One started session is one running session
    Given a fresh ledger
    When session "s1" starts
    And the ledger synthesizes a snapshot
    Then the snapshot reports 1 total, 1 running, 0 waiting

  Scenario: A stopped session stays in the total but not the running count
    Given a fresh ledger
    When session "s1" starts
    And session "s2" starts
    And session "s2" stops
    And the ledger synthesizes a snapshot
    Then the snapshot reports 2 total, 1 running, 0 waiting

  Scenario: A session awaiting approval is a waiting session
    Given a fresh ledger
    When session "s1" starts
    And session "s1" awaits approval "req_1"
    And the ledger synthesizes a snapshot
    Then the snapshot reports 1 total, 1 running, 1 waiting

  # Invariant 4 — totality: running and waiting partition the total over any event sequence.
  Scenario: Running and waiting never exceed the total, over any event sequence
    Then folding any session events keeps running and waiting within the total

  # Invariant 5 — token latch reuse: first credits 0, forward credits the delta, a backwards
  # total (a restart) credits 0 and never double-counts.

  Scenario: The first token total credits nothing
    Given a fresh ledger
    When the daemon observes a token total of 1000
    And the ledger synthesizes a snapshot
    Then the snapshot credits 0 tokens

  Scenario: A forward token total credits the delta
    Given a fresh ledger
    When the daemon observes a token total of 1000
    And the daemon observes a token total of 1500
    And the ledger synthesizes a snapshot
    Then the snapshot credits 500 tokens

  Scenario: A backwards token total is a restart that never double-counts
    Given a fresh ledger
    When the daemon observes a token total of 1000
    And the daemon observes a token total of 1500
    And the daemon observes a token total of 300
    And the ledger synthesizes a snapshot
    Then the snapshot credits 500 tokens
