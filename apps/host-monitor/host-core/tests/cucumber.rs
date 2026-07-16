//! Gherkin plumbing test: proves the [`step`] meter folds two node_exporter scrapes
//! into the busy percentage the feature file describes, reads memory straight from one
//! scrape, and withholds a sample until a CPU rate exists. A few of these guard the
//! domain boundary; the fine grain lives in the unit and property tests next to the code.

use cucumber::{given, then, when, World};
use host_core::{step, PollState, RawScrape, Sample};

#[derive(Debug, Default, World)]
struct MeterWorld {
    /// The carried CPU-counter baseline — primed by a `given`, advanced by the `when`.
    state: PollState,
    /// The most recent step's output: `Some(sample)` once a rate exists, else `None`.
    sample: Option<Sample>,
}

/// A scrape carrying the given CPU counters and placeholder memory — used to prime the
/// baseline, where memory is irrelevant because the first scrape yields no sample.
fn priming_scrape(idle: f64, total: f64) -> RawScrape {
    RawScrape {
        cpu_idle_secs: idle,
        cpu_total_secs: total,
        mem_total: 1.0,
        mem_avail: 1.0,
    }
}

#[given(regex = r"^a prior scrape of (\d+) idle of (\d+) total cpu-seconds$")]
fn a_prior_scrape(world: &mut MeterWorld, idle: u64, total: u64) {
    // Priming the baseline is one `step` from the empty state; it reports nothing.
    let (state, _): (PollState, Option<Sample>) =
        step(PollState::new(), priming_scrape(idle as f64, total as f64));
    world.state = state;
}

#[when(
    regex = r"^the host is scraped with (\d+) idle of (\d+) total cpu-seconds and (\d+) of (\d+) bytes free$"
)]
fn the_host_is_scraped(world: &mut MeterWorld, idle: u64, total: u64, avail: u64, mem_total: u64) {
    let scrape: RawScrape = RawScrape {
        cpu_idle_secs: idle as f64,
        cpu_total_secs: total as f64,
        mem_total: mem_total as f64,
        mem_avail: avail as f64,
    };
    let (state, sample): (PollState, Option<Sample>) = step(world.state, scrape);
    world.state = state;
    world.sample = sample;
}

#[then("no sample is reported")]
fn no_sample_is_reported(world: &mut MeterWorld) {
    assert_eq!(world.sample, None, "a single scrape has no rate to report");
}

#[then(regex = r"^the reported cpu is (\d+) percent$")]
fn the_reported_cpu_is(world: &mut MeterWorld, percent: u8) {
    let sample: Sample = world.sample.expect("a scenario must produce a sample");
    assert_eq!(sample.cpu().value(), percent);
}

#[then(regex = r"^the reported memory is (\d+) percent$")]
fn the_reported_memory_is(world: &mut MeterWorld, percent: u8) {
    let sample: Sample = world.sample.expect("a scenario must produce a sample");
    assert_eq!(sample.mem().value(), percent);
}

#[tokio::main]
async fn main() {
    MeterWorld::run("tests/features").await;
}
