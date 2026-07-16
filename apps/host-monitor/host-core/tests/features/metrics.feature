Feature: The meter folds node_exporter scrapes into CPU and memory percentages
  As the firmware monitoring a Fedora host over the network
  I want two cumulative CPU counter reads folded into one busy percentage
  So that the graph shows real load, not a meaningless snapshot of a counter

  Scenario: The first scrape primes the rate and reports nothing
    When the host is scraped with 1000 idle of 2000 total cpu-seconds and 8000000000 of 16000000000 bytes free
    Then no sample is reported

  Scenario: A second scrape reports the busy fraction over the interval
    Given a prior scrape of 1000 idle of 2000 total cpu-seconds
    When the host is scraped with 1025 idle of 2100 total cpu-seconds and 8000000000 of 16000000000 bytes free
    Then the reported cpu is 75 percent
    And the reported memory is 50 percent

  Scenario: A fully idle interval is zero percent busy
    Given a prior scrape of 1000 idle of 2000 total cpu-seconds
    When the host is scraped with 1100 idle of 2100 total cpu-seconds and 8000000000 of 16000000000 bytes free
    Then the reported cpu is 0 percent

  Scenario: A fully busy interval is one hundred percent
    Given a prior scrape of 1000 idle of 2000 total cpu-seconds
    When the host is scraped with 1000 idle of 2100 total cpu-seconds and 8000000000 of 16000000000 bytes free
    Then the reported cpu is 100 percent

  Scenario: Memory usage is read straight from one scrape
    Given a prior scrape of 1000 idle of 2000 total cpu-seconds
    When the host is scraped with 1010 idle of 2020 total cpu-seconds and 4000000000 of 16000000000 bytes free
    Then the reported memory is 75 percent

  Scenario: A counter reset reports no sample rather than a glitch
    Given a prior scrape of 9000 idle of 18000 total cpu-seconds
    When the host is scraped with 10 idle of 20 total cpu-seconds and 8000000000 of 16000000000 bytes free
    Then no sample is reported
