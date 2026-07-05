//! Gherkin plumbing test: proves the [`step`] sampler folds a batch of raw
//! readings into the calibrated percent the feature file describes, and reports
//! only on change. A few of these guard the domain boundary; the fine grain
//! lives in the unit and property tests next to the code.

use cucumber::{given, then, when, World};
use plant_core::{step, Calibration, Measurement, Moisture, Sample};

#[derive(Debug, Default, World)]
struct SamplerWorld {
    cal: Option<Calibration>,
    prev: Option<Measurement>,
    outcome: Option<Sample>,
}

#[given(regex = r"^a calibration of (\d+) dry and (\d+) wet$")]
fn a_calibration(world: &mut SamplerWorld, dry_raw: u16, wet_raw: u16) {
    world.cal = Some(Calibration::new(dry_raw, wet_raw));
}

#[given(regex = r"^a last reported moisture of (\d+) percent$")]
fn a_last_moisture(world: &mut SamplerWorld, percent: u8) {
    // Report-on-change keys on the percent; the previous measurement's raw count
    // is irrelevant to whether a new reading reports, so pair it with a placeholder.
    let moisture: Moisture = Moisture::new(percent).expect("a percent must be 0..=100");
    world.prev = Some(Measurement::new(0, moisture));
}

#[when(regex = r"^the probe is sampled at (.+)$")]
fn the_probe_is_sampled(world: &mut SamplerWorld, batch: String) {
    let raws: Vec<u16> = parse_batch(&batch);
    let cal: Calibration = world.cal.expect("a scenario must set a calibration");
    world.outcome = Some(step(world.prev, &raws, cal));
}

#[then(regex = r"^the reported moisture is (\d+) percent$")]
fn the_reported_moisture_is(world: &mut SamplerWorld, percent: u8) {
    let expected: Moisture = Moisture::new(percent).expect("a percent must be 0..=100");
    let report: Option<Moisture> = world.outcome.expect("a scenario must sample").report;
    assert_eq!(report, Some(expected));
}

#[then("no moisture is reported")]
fn no_moisture_is_reported(world: &mut SamplerWorld) {
    let report: Option<Moisture> = world.outcome.expect("a scenario must sample").report;
    assert_eq!(report, None);
}

#[then(regex = r"^the sampled raw mean is (\d+)$")]
fn the_sampled_raw_mean_is(world: &mut SamplerWorld, raw: u16) {
    let state: Measurement = world
        .outcome
        .expect("a scenario must sample")
        .state
        .expect("a non-empty batch produces a measurement");
    assert_eq!(
        state.raw(),
        raw,
        "the raw ADC mean rides alongside the percent"
    );
}

/// Parse a Gherkin reading list — `"2000"`, `"10, 20 and 60"`, or `"nothing"`
/// (the empty batch) — into raw counts.
fn parse_batch(text: &str) -> Vec<u16> {
    if text.trim() == "nothing" {
        return Vec::new();
    }
    text.replace(" and ", ", ")
        .split(',')
        .map(|token: &str| {
            token
                .trim()
                .parse::<u16>()
                .expect("a reading must be a number")
        })
        .collect()
}

#[tokio::main]
async fn main() {
    SamplerWorld::run("tests/features").await;
}
