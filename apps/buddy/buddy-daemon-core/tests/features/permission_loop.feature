Feature: The permission loop, coordinated and fail-safe
  Press A on the stick to approve a tool call, B to deny, and have Claude Code honour it. The
  coordinator sequences that loop between the per-tool-call hooks and the single bonded BLE link,
  owning no effects — it folds each input into an ordered list of effects the async daemon performs.
  Time is injected: the link-up handshake carries the epoch and tz the caller supplies, never a
  clock this crate reads.

  # ── Invariant: the loop end-to-end — hook-in -> snapshot-out -> device-response -> reply ──────
  Scenario: A bonded hook rides a gated snapshot and is answered allow by the device
    Given a bonded coordinator
    When a hook arrives for session "s1" running tool "bash"
    Then a gated snapshot carrying the prompt is sent down the link
    And the hook is not answered yet
    When the device answers session "s1" with an allow
    Then the waiting hook for session "s1" is answered allow
    And a fresh gated snapshot clearing the prompt is sent down the link

  # ── Invariant 3: a device deny replies Deny to the raising hook and clears the prompt ─────────
  Scenario: A bonded hook is answered deny by the device
    Given a bonded coordinator
    When a hook arrives for session "s1" running tool "bash"
    When the device answers session "s1" with a deny
    Then the waiting hook for session "s1" is answered deny
    And a fresh gated snapshot clearing the prompt is sent down the link

  # ── Invariant 2: an unbonded hook degrades to the normal terminal prompt (HOST fail-open) ─────
  Scenario: An unbonded hook is answered exactly Unbonded with no snapshot
    Given an unbonded coordinator
    When a hook arrives for session "s1" running tool "bash"
    Then the effects are exactly one Unbonded reply to the hook
    And no snapshot is sent down the link

  # ── Invariant 2: an unbonded hook raises NO prompt — none resurfaces when the link comes up ───
  Scenario: An unbonded hook raises no prompt that resurfaces on link-up
    Given an unbonded coordinator
    When a hook arrives for session "s1" running tool "bash"
    Then the effects are exactly one Unbonded reply to the hook
    When the link comes up
    Then the snapshot sent for the handshake carries no prompt

  # ── Invariant 4: a device line that answers no live prompt is ignored — never a guessed allow ─
  # Zero / one / many: four distinct malformed or unknown shapes, each yielding no effect at all,
  # and the live prompt survives untouched on the glass.
  Scenario: Every malformed or unknown device line is ignored and never clears the prompt
    Given a bonded coordinator
    When a hook arrives for session "s1" running tool "bash"
    When the device sends a line for an unknown prompt id
    Then no effects are produced
    When the device sends a non-permission line
    Then no effects are produced
    When the device sends an unparseable line
    Then no effects are produced
    When the device sends a line with an unknown decision token
    Then no effects are produced
    When a keepalive tick fires
    Then the snapshot sent for the keepalive still carries the prompt

  # ── Invariant 5: concurrent prompts route with no cross-talk; resolving one keeps the other ───
  Scenario: With two pending prompts, resolving one leaves the other on the glass
    Given a bonded coordinator
    When a hook arrives for session "s1" running tool "bash"
    And a hook arrives for session "s2" running tool "edit"
    When the device answers session "s2" with an allow
    Then the waiting hook for session "s2" is answered allow
    And the waiting hook for session "s1" is still waiting
    And the fresh snapshot still carries a prompt

  # ── Invariant 5, totality: every device answer routes to the hook that raised THAT id ─────────
  Scenario: Answering pending prompts in any order routes each reply to its own hook
    Then answering pending prompts in any order routes each reply to its own hook

  # ── Invariant 1, concrete: a keepalive while a prompt is outstanding still carries the prompt ─
  Scenario: A keepalive while a prompt is outstanding still carries the prompt
    Given a bonded coordinator
    When a hook arrives for session "s1" running tool "bash"
    And a keepalive tick fires
    Then the snapshot sent for the keepalive still carries the prompt

  # ── Invariant 1, totality: no keepalive or unrelated event ever clears a live prompt ──────────
  Scenario: No keepalive or unrelated event ever clears a live prompt
    Then any keepalive or unrelated event while a prompt is outstanding still carries the prompt

  # ── Invariant 6: a link-down drains every waiting hook (zero / one / many) ────────────────────
  Scenario: A link-down with nothing pending strands no hook
    Given a bonded coordinator
    When the link goes down
    Then no hook is answered
    And no snapshot is sent down the link

  Scenario: A link-down drains a single waiting hook with Unbonded
    Given a bonded coordinator
    When a hook arrives for session "s1" running tool "bash"
    And the link goes down
    Then the waiting hook for session "s1" is answered Unbonded
    And no snapshot is sent down the link

  Scenario: A link-down drains every waiting hook with Unbonded
    Given a bonded coordinator
    When a hook arrives for session "s1" running tool "bash"
    And a hook arrives for session "s2" running tool "edit"
    And the link goes down
    Then the waiting hook for session "s1" is answered Unbonded
    And the waiting hook for session "s2" is answered Unbonded

  # ── Invariant 7: the link-up handshake is time-sync, then owner, then snapshot — in order ─────
  # The time-sync line is exactly a two-element array, so it can never fall through to the
  # device's snapshot branch and clear a pending prompt.
  Scenario: Link-up emits the handshake in order — time, owner, snapshot
    Given an unbonded coordinator
    When the link comes up
    Then the first line sent is exactly the two-element time sync
    And the second line sent is the owner name
    And the third line sent is a snapshot
