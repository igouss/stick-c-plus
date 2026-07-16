//! Gherkin plumbing test: proves [`parse_pulse`] folds a `/pulse` body into the frame the
//! contract describes — every host in order on the declared grid, gaps kept as gaps, a down
//! host retained, out-of-range percents clamped, and a non-frame body rejected. A few of
//! these guard the wire boundary; the fine grain lives in the unit tests next to the code.

use cucumber::gherkin::Step;
use cucumber::{then, when, World};
use host_core::{HostSeries, Percent, Pulse};
use host_wire::{parse_pulse, WireError};

#[derive(Debug, Default, World)]
struct PulseWorld {
    /// The most recent parse: `Some(Ok)` on a frame, `Some(Err)` on a non-frame body.
    result: Option<Result<Pulse, WireError>>,
}

impl PulseWorld {
    /// The parsed frame, or a panic — for the `then` steps that assume a successful parse.
    fn frame(&self) -> &Pulse {
        self.result
            .as_ref()
            .expect("a scenario must parse a payload first")
            .as_ref()
            .expect("this scenario expects a valid frame")
    }
}

#[when("the pulse payload is parsed:")]
fn the_payload_is_parsed(world: &mut PulseWorld, step: &Step) {
    let body: &str = step
        .docstring
        .as_deref()
        .expect("the step must carry a docstring payload");
    world.result = Some(parse_pulse(body.as_bytes()));
}

#[then(regex = r"^the frame holds (\d+) hosts$")]
fn the_frame_holds_n_hosts(world: &mut PulseWorld, count: usize) {
    assert_eq!(world.frame().len(), count);
}

#[then(regex = r#"^the hosts are named "([^"]*)"$"#)]
fn the_hosts_are_named(world: &mut PulseWorld, names: String) {
    let expected: Vec<&str> = names.split(", ").collect();
    let got: Vec<&str> = world.frame().hosts().iter().map(HostSeries::name).collect();
    assert_eq!(got, expected);
}

#[then(regex = r"^the grid is step (\d+) window (\d+)$")]
fn the_grid_is(world: &mut PulseWorld, step_s: u32, window_s: u32) {
    assert_eq!(world.frame().step_s(), step_s);
    assert_eq!(world.frame().window_s(), window_s);
}

#[then(regex = r"^host (\d+) has cpu latest (\d+) and mem latest (\d+)$")]
fn host_n_latest(world: &mut PulseWorld, one_based: usize, cpu: u8, mem: u8) {
    let host: &HostSeries = &world.frame().hosts()[one_based - 1];
    assert_eq!(host.cpu().latest().map(Percent::value), Some(cpu));
    assert_eq!(host.mem().latest().map(Percent::value), Some(mem));
}

#[then(regex = r"^host (\d+) is down$")]
fn host_n_is_down(world: &mut PulseWorld, one_based: usize) {
    assert!(world.frame().hosts()[one_based - 1].is_down());
}

#[then(regex = r"^host (\d+) is not down$")]
fn host_n_is_not_down(world: &mut PulseWorld, one_based: usize) {
    assert!(!world.frame().hosts()[one_based - 1].is_down());
}

#[then("parsing fails")]
fn parsing_fails(world: &mut PulseWorld) {
    let result: &Result<Pulse, WireError> = world
        .result
        .as_ref()
        .expect("a scenario must parse a payload first");
    assert!(
        result.is_err(),
        "a non-frame body must not parse to a frame"
    );
}

#[tokio::main]
async fn main() {
    PulseWorld::run("tests/features").await;
}
