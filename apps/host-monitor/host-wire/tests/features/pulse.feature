Feature: A hostpulse /pulse body folds into a per-host CPU/memory frame
  As the firmware monitoring a homelab over the network
  I want one bearer-gated fetch of every host's ready-to-plot CPU/memory series
  So that the glass shows all three hosts at once with no on-device rate math

  Scenario: Every host in the payload becomes a row, in wire order, on the declared grid
    When the pulse payload is parsed:
      """
      { "step_s": 30, "window_s": 900, "hosts": [
        { "name": "fedora",     "cpu": [11,13,9,12,null,10], "mem": [41,42,42,43,43,44] },
        { "name": "oracle-arm", "cpu": [3,4,3,5,4,4],        "mem": [58,58,59,59,60,60] },
        { "name": "oracle-amd", "cpu": [1,2,1,1,2,1],        "mem": [22,22,23,23,23,24] }
      ] }
      """
    Then the frame holds 3 hosts
    And the hosts are named "fedora, oracle-arm, oracle-amd"
    And the grid is step 30 window 900
    And host 1 has cpu latest 10 and mem latest 44

  Scenario: The grid is read from the payload, not assumed
    When the pulse payload is parsed:
      """
      { "step_s": 15, "window_s": 600, "hosts": [] }
      """
    Then the grid is step 15 window 600

  Scenario: An empty hosts array is an empty frame
    When the pulse payload is parsed:
      """
      { "step_s": 30, "window_s": 900, "hosts": [] }
      """
    Then the frame holds 0 hosts

  Scenario: One host is carried with its latest present values
    When the pulse payload is parsed:
      """
      { "step_s": 30, "window_s": 900, "hosts": [
        { "name": "fedora", "cpu": [11,13], "mem": [41,44] }
      ] }
      """
    Then the frame holds 1 hosts
    And host 1 has cpu latest 13 and mem latest 44

  Scenario: A null is a gap, and the label reads past a trailing gap
    When the pulse payload is parsed:
      """
      { "step_s": 30, "window_s": 900, "hosts": [
        { "name": "fedora", "cpu": [11,null,10,null], "mem": [41,42,43,44] }
      ] }
      """
    Then host 1 has cpu latest 10 and mem latest 44

  Scenario: A down host arrives all-null and is kept, not dropped
    When the pulse payload is parsed:
      """
      { "step_s": 30, "window_s": 900, "hosts": [
        { "name": "fedora",     "cpu": [11,10], "mem": [41,44] },
        { "name": "oracle-arm", "cpu": [null,null], "mem": [null,null] },
        { "name": "oracle-amd", "cpu": [1,2],   "mem": [22,24] }
      ] }
      """
    Then the frame holds 3 hosts
    And host 2 is down
    And host 1 is not down

  Scenario: Out-of-range percents are clamped, not rejected
    When the pulse payload is parsed:
      """
      { "step_s": 30, "window_s": 900, "hosts": [
        { "name": "fedora", "cpu": [150], "mem": [-4] }
      ] }
      """
    Then host 1 has cpu latest 100 and mem latest 0

  Scenario: A body that is not a frame fails to parse
    When the pulse payload is parsed:
      """
      { "error": "prometheus_unavailable" }
      """
    Then parsing fails
