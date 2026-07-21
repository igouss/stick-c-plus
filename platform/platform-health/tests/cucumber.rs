//! The Gherkin suite for the two boundaries this crate guards: the verdict fold and the
//! alarm FSM. This is the boundary a person would describe out loud; every rank, every
//! tie-break and every FSM transition's fine grain lives in the unit and property tests next
//! to the code.
//!
//! Time is always handed in explicitly — nothing here sleeps, and the whole suite is
//! deterministic.

use cucumber::{given, then, when, World};
use platform_core::Tick;
use platform_health::{
    assess, Alarm, AlarmState, BootVerdict, Component, CrashCause, Fault, Observed, Resources,
    Siren, Verdict,
};

/// One component's evidence as a scenario states it: an "ago" relative to whatever tick the
/// board is later assessed at, since the Given step runs before the When step picks `now`.
#[derive(Debug)]
struct ComponentSpec {
    name: &'static str,
    deadline_ms: u32,
    last_beat_ago: Option<Tick>,
    gave_up_ago: Option<Tick>,
}

/// The board under test: the verdict side (registered components, boot, resources) and the
/// alarm side (the FSM and its cadence), plus what came out of the last action taken.
#[derive(Debug, World)]
#[world(init = Self::new)]
struct HealthWorld {
    pending: Vec<ComponentSpec>,
    boot: BootVerdict,
    resources: Option<Resources>,
    heap_floor_bytes: u32,
    verdict: Option<Verdict>,
    assessed_at: Tick,

    alarm: Alarm,
    cadence_ms: Tick,
    last_chirp: Option<Siren>,
}

impl HealthWorld {
    fn new() -> Self {
        HealthWorld {
            pending: Vec::new(),
            boot: BootVerdict::Clean,
            resources: None,
            heap_floor_bytes: 0,
            verdict: None,
            assessed_at: 0,
            alarm: Alarm::new(),
            cadence_ms: 5_000,
            last_chirp: None,
        }
    }
}

/// A scenario names a component with a `String` capture; the domain wants `&'static str`. A
/// per-scenario leak is the cheapest correct answer in a short-lived test binary.
fn leak(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

// --- verdict.feature -------------------------------------------------------------------

#[given(regex = r"^the board booted cleanly$")]
fn boot_clean(world: &mut HealthWorld) {
    world.boot = BootVerdict::Clean;
}

#[given(regex = r"^the board booted crashed by a task watchdog$")]
fn boot_crashed_watchdog(world: &mut HealthWorld) {
    world.boot = BootVerdict::Crashed(CrashCause::TaskWatchdog);
}

#[given(regex = r"^the board booted crashed by a panic$")]
fn boot_crashed_panic(world: &mut HealthWorld) {
    world.boot = BootVerdict::Crashed(CrashCause::Panic);
}

#[given(regex = r#"^a component "([^"]+)" with a (\d+) ms deadline that last beat (\d+) ms ago$"#)]
fn component_last_beat(world: &mut HealthWorld, name: String, deadline_ms: u32, ago: Tick) {
    world.pending.push(ComponentSpec {
        name: leak(name),
        deadline_ms,
        last_beat_ago: Some(ago),
        gave_up_ago: None,
    });
}

#[given(regex = r#"^a component "([^"]+)" with a (\d+) ms deadline that has never beaten$"#)]
fn component_never_beaten(world: &mut HealthWorld, name: String, deadline_ms: u32) {
    world.pending.push(ComponentSpec {
        name: leak(name),
        deadline_ms,
        last_beat_ago: None,
        gave_up_ago: None,
    });
}

#[given(regex = r#"^a component "([^"]+)" reported giving up (\d+) ms ago$"#)]
fn component_reported(world: &mut HealthWorld, name: String, ago: Tick) {
    world.pending.push(ComponentSpec {
        name: leak(name),
        deadline_ms: 2_000,
        last_beat_ago: None,
        gave_up_ago: Some(ago),
    });
}

#[given(regex = r"^the board reports (\d+) free bytes against a (\d+) byte floor$")]
fn board_resources(world: &mut HealthWorld, free_heap_bytes: u32, floor_bytes: u32) {
    world.resources = Some(Resources { free_heap_bytes });
    world.heap_floor_bytes = floor_bytes;
}

#[given(regex = r"^the board has not read its heap$")]
fn board_no_reading(world: &mut HealthWorld) {
    world.resources = None;
}

#[when(regex = r"^the board is assessed at tick (\d+) ms$")]
fn assess_at(world: &mut HealthWorld, now: Tick) {
    let observed: Vec<Observed> = world
        .pending
        .iter()
        .map(|spec: &ComponentSpec| Observed {
            component: Component(spec.name),
            deadline_ms: spec.deadline_ms,
            last_beat: spec.last_beat_ago.map(|ago: Tick| now.saturating_sub(ago)),
            gave_up_at: spec.gave_up_ago.map(|ago: Tick| now.saturating_sub(ago)),
        })
        .collect();
    world.assessed_at = now;
    world.verdict = Some(assess(
        &observed,
        world.boot,
        world.resources,
        world.heap_floor_bytes,
        now,
    ));
}

#[then(regex = r"^the verdict is healthy$")]
fn verdict_is_healthy(world: &mut HealthWorld) {
    assert_eq!(world.verdict, Some(Verdict::Healthy));
}

#[then(regex = r#"^the verdict names "([^"]+)" as stalled for (\d+) ms$"#)]
fn verdict_names_stalled(world: &mut HealthWorld, name: String, silent_for_ms: Tick) {
    assert_eq!(
        world.verdict,
        Some(Verdict::Faulted(Fault::Stalled {
            component: Component(leak(name)),
            silent_for_ms,
        }))
    );
}

#[then(regex = r#"^the verdict names "([^"]+)" as reported for (\d+) ms$"#)]
fn verdict_names_reported(world: &mut HealthWorld, name: String, for_ms: Tick) {
    assert_eq!(
        world.verdict,
        Some(Verdict::Faulted(Fault::Reported {
            component: Component(leak(name)),
            for_ms,
        }))
    );
}

#[then(regex = r"^the verdict names the board as crashed by a task watchdog$")]
fn verdict_names_crashed_watchdog(world: &mut HealthWorld) {
    assert_eq!(
        world.verdict,
        Some(Verdict::Faulted(Fault::Crashed {
            cause: CrashCause::TaskWatchdog,
            since_ms: world.assessed_at,
        }))
    );
}

#[then(regex = r"^the verdict names the board as crashed by a panic$")]
fn verdict_names_crashed_panic(world: &mut HealthWorld) {
    assert_eq!(
        world.verdict,
        Some(Verdict::Faulted(Fault::Crashed {
            cause: CrashCause::Panic,
            since_ms: world.assessed_at,
        }))
    );
}

#[then(
    regex = r"^the verdict names the board as starved with (\d+) free bytes against a (\d+) byte floor$"
)]
fn verdict_names_starved(world: &mut HealthWorld, free_heap_bytes: u32, floor_bytes: u32) {
    assert_eq!(
        world.verdict,
        Some(Verdict::Faulted(Fault::Starved {
            free_heap_bytes,
            floor_bytes,
        }))
    );
}

// --- alarm.feature -----------------------------------------------------------------------

fn stall(name: &'static str) -> Fault {
    Fault::Stalled {
        component: Component(name),
        silent_for_ms: 1,
    }
}

fn crash() -> Fault {
    Fault::Crashed {
        cause: CrashCause::Panic,
        since_ms: 0,
    }
}

#[given(regex = r"^a fresh alarm and a (\d+) ms cadence$")]
fn fresh_alarm(world: &mut HealthWorld, cadence_ms: Tick) {
    world.alarm = Alarm::new();
    world.cadence_ms = cadence_ms;
    world.last_chirp = None;
}

#[given(regex = r#"^the alarm is sounding a stall on "([^"]+)" since tick (\d+) ms$"#)]
fn given_alarm_sounding(world: &mut HealthWorld, name: String, since: Tick) {
    let (alarm, chirp): (Alarm, Option<Siren>) = world.alarm.step(
        &Verdict::Faulted(stall(leak(name))),
        world.cadence_ms,
        since,
    );
    world.alarm = alarm;
    world.last_chirp = chirp;
}

#[given(regex = r"^the alarm is acknowledged$")]
fn given_alarm_acknowledged(world: &mut HealthWorld) {
    world.alarm = world.alarm.acknowledge();
}

#[when(regex = r"^the alarm steps against a healthy verdict at tick (\d+) ms$")]
fn step_healthy(world: &mut HealthWorld, now: Tick) {
    let (alarm, chirp): (Alarm, Option<Siren>) =
        world.alarm.step(&Verdict::Healthy, world.cadence_ms, now);
    world.alarm = alarm;
    world.last_chirp = chirp;
}

#[when(regex = r#"^the alarm steps against a stall on "([^"]+)" at tick (\d+) ms$"#)]
fn step_stall(world: &mut HealthWorld, name: String, now: Tick) {
    let (alarm, chirp): (Alarm, Option<Siren>) =
        world
            .alarm
            .step(&Verdict::Faulted(stall(leak(name))), world.cadence_ms, now);
    world.alarm = alarm;
    world.last_chirp = chirp;
}

#[when(regex = r"^the alarm steps against a crash at tick (\d+) ms$")]
fn step_crash(world: &mut HealthWorld, now: Tick) {
    let (alarm, chirp): (Alarm, Option<Siren>) =
        world
            .alarm
            .step(&Verdict::Faulted(crash()), world.cadence_ms, now);
    world.alarm = alarm;
    world.last_chirp = chirp;
}

#[when(regex = r"^the alarm is acknowledged$")]
fn when_alarm_acknowledged(world: &mut HealthWorld) {
    world.alarm = world.alarm.acknowledge();
    // Acknowledging is an action that produced no chirp; record that, so a later
    // "no chirp sounds" reads this step's outcome rather than the previous step's.
    world.last_chirp = None;
}

#[then(regex = r"^a chirp sounds$")]
fn chirp_sounds(world: &mut HealthWorld) {
    assert!(world.last_chirp.is_some(), "expected a chirp, got none");
}

#[then(regex = r"^no chirp sounds$")]
fn no_chirp_sounds(world: &mut HealthWorld) {
    assert_eq!(world.last_chirp, None, "expected silence");
}

#[then(regex = r"^the alarm is silent$")]
fn alarm_is_silent(world: &mut HealthWorld) {
    assert!(matches!(world.alarm.state, AlarmState::Silent));
}

#[then(regex = r"^the alarm is sounding$")]
fn alarm_is_sounding(world: &mut HealthWorld) {
    assert!(matches!(world.alarm.state, AlarmState::Sounding { .. }));
}

#[then(regex = r"^the alarm is acknowledged$")]
fn alarm_is_acknowledged(world: &mut HealthWorld) {
    assert!(matches!(world.alarm.state, AlarmState::Acknowledged { .. }));
}

#[tokio::main]
async fn main() {
    HealthWorld::run("tests/features").await;
}
