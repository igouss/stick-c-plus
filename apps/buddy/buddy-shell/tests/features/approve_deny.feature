Feature: A answers a pending tool call and B refuses it
  As the owner of a stick that gates Claude Code
  I want the two buttons to put an answer on the wire naming the prompt they answered
  So that a press on the desk is the decision the hook acts on

  Scenario: A allows the pending prompt
    Given a booted stick
    And a snapshot arrives with prompt "p1" for Bash
    When the front button is clicked
    Then the answer sent is "once" for prompt "p1"

  Scenario: B denies the pending prompt
    Given a booted stick
    And a snapshot arrives with prompt "p1" for Bash
    When the side button is clicked
    Then the answer sent is "deny" for prompt "p1"

  Scenario: A prompt is answered once, not twice
    Given a booted stick
    And a snapshot arrives with prompt "p1" for Bash
    When the front button is clicked
    And the front button is clicked
    Then exactly 1 answer was sent

  Scenario: The approval screen comes down as soon as the owner has decided
    Given a booted stick
    And a snapshot arrives with prompt "p1" for Bash
    When the front button is clicked
    Then no prompt is on the glass

  Scenario: A press with no prompt pending sends nothing
    Given a booted stick
    When the front button is clicked
    Then exactly 0 answers were sent

  Scenario: A new prompt is answerable again
    Given a booted stick
    And a snapshot arrives with prompt "p1" for Bash
    When the front button is clicked
    And a snapshot arrives with prompt "p2" for Bash
    And the side button is clicked
    Then the answer sent is "deny" for prompt "p2"
    And exactly 2 answers were sent
