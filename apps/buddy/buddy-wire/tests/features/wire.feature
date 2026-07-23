Feature: A NUS line is classified into a typed inbound message, and the buddy replies in kind
  As the firmware and the Linux bridge sharing one wire contract
  I want each line classified so only a snapshot can touch prompt state
  So that a transcript event never wipes a pending approval and unknown commands are always acked

  Scenario: A bare object is a snapshot and merges into state
    When the line is parsed:
      """
      {"running":2,"total":3}
      """
    Then it is a snapshot
    And merging it leaves the running count at 2

  Scenario: A two-element time array is a time sync, not a snapshot
    When the line is parsed:
      """
      {"time":[1775731234,-25200]}
      """
    Then it is a time sync with epoch 1775731234 and offset -25200

  Scenario: A one-element time array is an arity error, not a prompt-clearing snapshot
    When the line is parsed:
      """
      {"time":[1775731234]}
      """
    Then parsing fails

  Scenario: A turn event is an event, and never clears a pending prompt
    Given a pending prompt "req_abc" is on the glass
    When the line is parsed:
      """
      {"evt":"turn","role":"assistant"}
      """
    Then it is an event
    And the pending prompt "req_abc" is still on the glass

  Scenario: An absent prompt in a snapshot clears the pending prompt
    Given a pending prompt "req_abc" is on the glass
    When the line is parsed:
      """
      {"running":1}
      """
    Then it is a snapshot
    And after merging, no prompt is on the glass

  Scenario: A known command is dispatched to a positive ack
    When the line is parsed:
      """
      {"cmd":"status"}
      """
    Then it is a command
    And dispatching it acks "status" ok

  Scenario: An unknown command is dispatched to a negative ack, never swallowed
    When the line is parsed:
      """
      {"cmd":"frobnicate"}
      """
    Then it is a command
    And dispatching it nacks "frobnicate"

  # D7: entries are oldest-first — index 0 is the oldest, the wire order is verbatim.
  Scenario: Transcript entries are stored oldest-first
    When the line is parsed:
      """
      {"entries":["oldest","middle","newest"]}
      """
    Then it is a snapshot
    And merging keeps "oldest" oldest and "newest" newest

  # Defect fix (c): truncation is a typed error, never a silent drop.
  Scenario: A full-capacity line frames to exactly one line
    When a full-capacity object line terminated by newline is pushed to the line framer
    Then the framer completes exactly one line

  Scenario: An over-capacity line is reported, not silently truncated
    When a chunk of 1024 object bytes with no terminator is pushed to the line framer
    Then the framer reports the line was too long, not a silent truncation

  Scenario: A within-capacity push to the receive ring is accepted
    When 2047 bytes are pushed to the receive ring
    Then the ring accepts them

  Scenario: An over-capacity push to the receive ring is reported, not dropped mid-line
    When 2048 bytes are pushed to the receive ring
    Then the ring reports an overflow, not a silent mid-line drop
