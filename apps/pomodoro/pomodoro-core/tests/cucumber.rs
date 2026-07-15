//! Gherkin plumbing test: proves the pomodoro [`step`] drives phases, the pause-correct
//! countdown, and the jingles the way the feature file describes. A few of these guard the
//! domain boundary; the fine grain lives in the unit and property tests next to the code.

use cucumber::{given, then, when, World};
use pomodoro_core::{step, Durations, Event, Jingle, Phase, Status, Timer};

/// The scenario's timer, the durations it runs under, and the last jingle a step produced.
#[derive(Debug, World)]
struct TimerWorld {
    durations: Durations,
    timer: Timer,
    last_jingle: Option<Jingle>,
}

impl Default for TimerWorld {
    fn default() -> Self {
        // A placeholder duration set; every scenario's Background replaces it before any step.
        TimerWorld {
            durations: Durations {
                focus_ms: 0,
                short_break_ms: 0,
                long_break_ms: 0,
                long_break_every: 0,
            },
            timer: Timer::new(),
            last_jingle: None,
        }
    }
}

/// Fold one event into the world's timer, recording the jingle it produced.
fn apply(world: &mut TimerWorld, event: Event, now: u64) {
    let stepped: pomodoro_core::Stepped = step(world.timer, event, now, world.durations);
    world.timer = stepped.timer;
    world.last_jingle = stepped.jingle;
}

fn parse_phase(name: &str) -> Phase {
    match name {
        "Focus" => Phase::Focus,
        "ShortBreak" => Phase::ShortBreak,
        "LongBreak" => Phase::LongBreak,
        other => panic!("unknown phase {other:?}"),
    }
}

fn parse_status(name: &str) -> Status {
    match name {
        "Idle" => Status::Idle,
        "Running" => Status::Running,
        "Paused" => Status::Paused,
        "Finished" => Status::Finished,
        other => panic!("unknown status {other:?}"),
    }
}

fn parse_jingle(name: &str) -> Jingle {
    match name {
        "FocusStart" => Jingle::FocusStart,
        "BreakStart" => Jingle::BreakStart,
        "LongBreakStart" => Jingle::LongBreakStart,
        "PhaseComplete" => Jingle::PhaseComplete,
        "SessionRestart" => Jingle::SessionRestart,
        other => panic!("unknown jingle {other:?}"),
    }
}

#[given(
    regex = r"^durations of (\d+) ms focus, (\d+) ms short break, (\d+) ms long break, long every (\d+)$"
)]
fn a_durations(world: &mut TimerWorld, focus: u64, short: u64, long: u64, every: u32) {
    world.durations = Durations {
        focus_ms: focus,
        short_break_ms: short,
        long_break_ms: long,
        long_break_every: every,
    };
    world.timer = Timer::new();
    world.last_jingle = None;
}

#[when(regex = r"^the front button is tapped at (\d+) ms$")]
fn tap_front(world: &mut TimerWorld, now: u64) {
    apply(world, Event::StartPause, now);
}

#[when(regex = r"^the front button is held at (\d+) ms$")]
fn hold_front(world: &mut TimerWorld, now: u64) {
    apply(world, Event::Reset, now);
}

#[when(regex = r"^the front button is double-tapped at (\d+) ms$")]
fn double_tap_front(world: &mut TimerWorld, now: u64) {
    apply(world, Event::RestartSession, now);
}

#[when(regex = r"^the side button is tapped at (\d+) ms$")]
fn tap_side(world: &mut TimerWorld, now: u64) {
    apply(world, Event::Skip, now);
}

#[when(regex = r"^the clock ticks at (\d+) ms$")]
fn clock_ticks(world: &mut TimerWorld, now: u64) {
    apply(world, Event::Tick, now);
}

#[then(regex = r"^the phase is (\w+)$")]
fn phase_is(world: &mut TimerWorld, name: String) {
    assert_eq!(world.timer.phase(), parse_phase(&name));
}

#[then(regex = r"^the status is (\w+)$")]
fn status_is(world: &mut TimerWorld, name: String) {
    assert_eq!(world.timer.status(), parse_status(&name));
}

#[then(regex = r"^(\d+) ms remain at (\d+) ms$")]
fn remain_at(world: &mut TimerWorld, expected: u64, at: u64) {
    assert_eq!(world.timer.remaining(at, world.durations), expected);
}

#[then(regex = r"^a (\w+) jingle sounds$")]
fn jingle_sounds(world: &mut TimerWorld, name: String) {
    assert_eq!(world.last_jingle, Some(parse_jingle(&name)));
}

#[tokio::main]
async fn main() {
    TimerWorld::run("tests/features").await;
}
