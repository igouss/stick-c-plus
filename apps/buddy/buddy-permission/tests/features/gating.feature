Feature: Emission gating keeps a live prompt on the glass
  An absent prompt field in a snapshot legitimately CLEARS the approval on the device, so
  while a prompt is outstanding every synthesized snapshot MUST carry it. The prompt clears
  ONLY on an explicit resolve — never as a side effect of a keepalive. The GatedSnapshot gate
  re-attaches the outstanding prompt so the daemon can never hand a bare, prompt-less snapshot
  to the link while a prompt is live.

  # Invariant 6 — the device-side anti-hazard.

  Scenario: A live prompt rides every heartbeat
    Given a fresh ledger
    When session "s1" starts
    And session "s1" awaits approval "req_1"
    And the ledger synthesizes a snapshot
    Then the snapshot carries a prompt
    When the ledger synthesizes a snapshot again
    Then the snapshot carries a prompt

  Scenario: An explicit resolve clears the prompt
    Given a fresh ledger
    When session "s1" starts
    And session "s1" awaits approval "req_1"
    And session "s1" resolves
    And the ledger synthesizes a snapshot
    Then the snapshot carries no prompt

  # Invariant 6 — totality: a live prompt survives any keepalive event sequence that is not a resolve.
  Scenario: A live prompt survives any keepalive sequence
    Then any synthesis while a prompt is live carries a prompt

  # Invariant 6 — the cross-session hazard: a resolve on an UNRELATED session must not clear the glass.
  Scenario: A resolve on another session keeps a still-waiting prompt
    Given a fresh ledger
    When session "s1" awaits approval "req_1"
    And session "s2" resolves
    And the ledger synthesizes a snapshot
    Then the snapshot carries a prompt
    And the snapshot reports 2 total, 0 running, 1 waiting

  # Invariant 6 — completeness: over ANY event sequence, a waiting session never rides an absent prompt.
  Scenario: A waiting session always rides a prompt
    Then any waiting session always rides a prompt

  Scenario: Gating restores an outstanding prompt onto a bare snapshot
    When a bare snapshot is gated against the outstanding prompt "req_9"
    Then the gated snapshot carries a prompt

  Scenario: Gating with no outstanding prompt passes the snapshot through
    When a bare snapshot is gated against no outstanding prompt
    Then the gated snapshot carries no prompt
