Feature: The fail-safe PreToolUse hook decision
  Claude Code treats a PreToolUse hook timeout as a non-blocking error and PROCEEDS
  with the tool call, so the ONLY safe emit is a real device decision. Every other
  outcome must print NOTHING and hand control to Claude Code's normal terminal prompt.
  permissionDecision is only ever "allow" or "deny" here — never "ask" (upstream #39344
  can silently disable deny rules) and never "defer".

  # Invariant 1 — the fail-open guard: only Decided(Once/Deny) emits; every other outcome is Silent.
  # Invariant 3 — the emitted JSON is exactly the PreToolUse shape with allow/deny.

  Scenario: A device approval emits an allow
    When the device outcome is a decision to allow
    Then the hook emits an allow decision
    And the printed line is exactly the PreToolUse allow JSON

  Scenario: A device denial emits a deny
    When the device outcome is a decision to deny
    Then the hook emits a deny decision
    And the printed line is exactly the PreToolUse deny JSON

  Scenario: A timeout stays silent
    When the device outcome is a timeout
    Then the hook stays silent
    And the printed line is empty

  Scenario: A dead daemon stays silent
    When the device outcome is a dead daemon
    Then the hook stays silent
    And the printed line is empty

  Scenario: Nothing bonded stays silent
    When the device outcome is nothing bonded
    Then the hook stays silent
    And the printed line is empty

  # Invariant 1 — totality: enumerate the whole non-decided failure space, not one example.
  Scenario: Every non-decided outcome prints nothing
    Then every non-decided outcome is silent

  # Invariant 2 — totality: no rendered output ever mentions ask or defer, over the whole output space.
  Scenario: No hook output ever mentions ask or defer
    Then no hook output ever contains ask or defer
