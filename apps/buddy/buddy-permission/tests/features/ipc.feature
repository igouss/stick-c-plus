Feature: Hook to daemon IPC round-trip
  A per-tool-call hook process cannot own a bonded BLE connection, so it forwards the tool-call
  context to the long-lived daemon and blocks on the reply. The two DTOs that cross that socket
  must serialize and deserialize back to the identical value — the hook and the daemon agree on
  the contract.

  # Invariant 7 — faithful serde round-trip over the DTO space.

  Scenario: A hook request round-trips through serde
    Then any hook request serializes and deserializes back to itself

  Scenario: A daemon reply round-trips through serde
    Then every daemon reply serializes and deserializes back to itself
