Feature: The daemon serializes a line the device parses back to the same message
  As the Linux bridge holding the central half of the wire contract
  I want every central->device line to round-trip through the device's own parse
  So that the bridge and the firmware share ONE contract and no absent-field hazard is reintroduced

  # Invariant 2 (anti-hazard): a prompt-bearing snapshot keeps its prompt key and round-trips.
  Scenario: A snapshot with a pending prompt serializes with a prompt key and round-trips
    When the daemon serializes a snapshot carrying the prompt "req_abc"
    Then the serialized line has a prompt key
    And the device parses it back as the same snapshot

  # Invariant 2 (anti-hazard): prompt:None omits the key entirely, so the device CLEARS the prompt.
  # A bare, prompt-less snapshot must be representable and distinct from a prompt-bearing one.
  Scenario: A prompt-less snapshot serializes with NO prompt key and round-trips
    When the daemon serializes a prompt-less snapshot
    Then the serialized line has no prompt key
    And the serialized line has no null
    And the device parses it back as the same snapshot

  # Invariant 3: the time sync is ALWAYS a strict two-element array — never the wrong arity the
  # device rejects (which would fall through and clear a pending prompt).
  Scenario: A time sync serializes to a strict two-element array and round-trips
    When the daemon serializes a time sync with epoch 1775731234 and offset -25200
    Then the serialized time array has exactly two elements
    And the device parses it as a time sync with epoch 1775731234 and offset -25200

  # Invariant 4: the owner connect message round-trips to Command::Owner (zero-length name).
  Scenario: An empty owner name round-trips to an owner command
    When the daemon serializes the owner name ""
    Then the device parses it as an owner command ""

  # Invariant 4: the owner connect message round-trips to Command::Owner (one name).
  Scenario: An owner name round-trips to an owner command
    When the daemon serializes the owner name "Felix"
    Then the device parses it as an owner command "Felix"

  # Invariant 4: the buddy-name connect message round-trips to Command::Name (a many-word name).
  Scenario: A buddy name round-trips to a name command
    When the daemon serializes the buddy name "Rex the Destroyer"
    Then the device parses it as a name command "Rex the Destroyer"

  # Invariant 5: the central-side parse round-trips with the device-side to_json (once).
  Scenario: A once decision round-trips central-side
    Given a device permission response for "req_abc" decided "once"
    When the central parses that permission line
    Then the central reads back decision "once" for "req_abc"

  # Invariant 5: the central-side parse round-trips with the device-side to_json (deny).
  Scenario: A deny decision round-trips central-side
    Given a device permission response for "req_abc" decided "deny"
    When the central parses that permission line
    Then the central reads back decision "deny" for "req_abc"

  # Invariant 6: an unknown decision token is a TYPED error, NEVER a silent Decision::Once.
  # A wrong default here would approve a tool call the owner denied.
  Scenario: An unknown decision token is a typed error, not a silent approval
    When the central parses the permission line:
      """
      {"cmd":"permission","id":"req_abc","decision":"maybe"}
      """
    Then the central parse is a typed error, not a silent approval
    And the central parse reports a bad decision

  # Invariant 6: a missing decision is a bad-decision error, never defaulted to Once.
  Scenario: A missing decision is a typed error, not a silent approval
    When the central parses the permission line:
      """
      {"cmd":"permission","id":"req_abc"}
      """
    Then the central parse is a typed error, not a silent approval
    And the central parse reports a bad decision

  # Invariant 6: a missing id is a typed error.
  Scenario: A missing id is a typed error
    When the central parses the permission line:
      """
      {"cmd":"permission","decision":"once"}
      """
    Then the central parse is a typed error, not a silent approval
    And the central parse reports a missing id

  # Invariant 6: a non-permission line is a typed error, never read as an approval.
  Scenario: A non-permission line is a typed error
    When the central parses the permission line:
      """
      {"cmd":"status"}
      """
    Then the central parse is a typed error, not a silent approval
