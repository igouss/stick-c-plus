//! The Gherkin suite for the button boundary: a thumb on the glass becomes events.
//!
//! This guards the *boundary* — what a person would say out loud about the buttons. The fine
//! grain lives in the unit and property tests next to the code: every debounce edge, every
//! window boundary, every bounce train. Here a scenario reads like the manual.
//!
//! Time is hand-cranked at 1 ms per poll, so nothing sleeps and the whole suite is
//! deterministic.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cucumber::{given, then, when, World};
use platform_core::Tick;
use platform_input::{
    ButtonEvent, ButtonId, ButtonLevel, Buttons, Gesture, Gestures, InputConfig, LatchedGesture,
};

/// A levelled button whose raw level the scenario drives.
struct SharedLevel {
    pressed: Arc<AtomicBool>,
}

impl ButtonLevel for SharedLevel {
    fn pressed(&mut self) -> bool {
        self.pressed.load(Ordering::Relaxed)
    }
}

/// A PMIC latch the scenario arms; draining clears it, exactly like the real register.
struct SharedLatch {
    armed: Arc<Mutex<Option<Gesture>>>,
}

impl LatchedGesture for SharedLatch {
    fn take(&mut self) -> Option<Gesture> {
        self.armed.lock().expect("latch lock").take()
    }
}

/// The board under test: the three levels the scenario drives, the buttons reading them, the
/// hand-cranked clock, and everything they have emitted so far.
#[derive(World)]
#[world(init = Self::new)]
struct BoardWorld {
    front: Arc<AtomicBool>,
    side: Arc<AtomicBool>,
    power: Arc<Mutex<Option<Gesture>>>,
    buttons: Buttons<SharedLevel, SharedLevel, SharedLatch>,
    now: Tick,
    seen: Vec<ButtonEvent>,
}

/// `Buttons` holds port implementations and is deliberately not `Debug`; the scenario only ever
/// needs to see the levels and what has been emitted.
impl fmt::Debug for BoardWorld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoardWorld")
            .field("now", &self.now)
            .field("seen", &self.seen)
            .finish()
    }
}

impl BoardWorld {
    /// A board with every button prompt — the default a scenario overrides in its Background.
    fn new() -> Self {
        Self::with_config(InputConfig::default())
    }

    /// A board wired to `config`, everything released and nothing latched.
    fn with_config(config: InputConfig) -> Self {
        let front: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
        let side: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
        let power: Arc<Mutex<Option<Gesture>>> = Arc::new(Mutex::new(None));
        BoardWorld {
            buttons: Buttons::new(
                SharedLevel {
                    pressed: Arc::clone(&front),
                },
                SharedLevel {
                    pressed: Arc::clone(&side),
                },
                SharedLatch {
                    armed: Arc::clone(&power),
                },
                config,
            ),
            front,
            side,
            power,
            now: 0,
            seen: Vec::new(),
        }
    }

    /// Poll `ms` times at 1 ms per poll, keeping everything that came out.
    fn poll_for(&mut self, ms: Tick) {
        (0..ms).for_each(|_| {
            let events: Vec<ButtonEvent> = self.buttons.poll(self.now).collect();
            self.seen.extend(events);
            self.now += 1;
        });
    }

    /// Hold `level` at `pressed` for `ms`.
    fn hold(&mut self, level: &Arc<AtomicBool>, pressed: bool, ms: Tick) {
        level.store(pressed, Ordering::Relaxed);
        self.poll_for(ms);
    }
}

/// How a scenario names one event: `"front click"`, `"power click"`, `"side long-hold"`.
fn describe(event: &ButtonEvent) -> String {
    let button: &str = match event.button {
        ButtonId::Front => "front",
        ButtonId::Side => "side",
        ButtonId::Power => "power",
    };
    let gesture: &str = match event.gesture {
        Gesture::Click => "click",
        Gesture::DoubleClick => "double-click",
        Gesture::LongHold => "long-hold",
    };
    format!("{button} {gesture}")
}

#[given(regex = r"^a board where the front button reports double-clicks$")]
fn front_reports_double_clicks(world: &mut BoardWorld) {
    *world = BoardWorld::with_config(InputConfig {
        front: Gestures::WithDoubleClick,
        ..InputConfig::default()
    });
}

#[when(regex = r"^the front button is held for (\d+) ms$")]
fn front_held(world: &mut BoardWorld, ms: Tick) {
    let front: Arc<AtomicBool> = Arc::clone(&world.front);
    world.hold(&front, true, ms);
}

#[when(regex = r"^the front button is released for (\d+) ms$")]
fn front_released(world: &mut BoardWorld, ms: Tick) {
    let front: Arc<AtomicBool> = Arc::clone(&world.front);
    world.hold(&front, false, ms);
}

#[when(regex = r"^the side button is held for (\d+) ms$")]
fn side_held(world: &mut BoardWorld, ms: Tick) {
    let side: Arc<AtomicBool> = Arc::clone(&world.side);
    world.hold(&side, true, ms);
}

#[when(regex = r"^the side button is released for (\d+) ms$")]
fn side_released(world: &mut BoardWorld, ms: Tick) {
    let side: Arc<AtomicBool> = Arc::clone(&world.side);
    world.hold(&side, false, ms);
}

#[when(regex = r"^both levelled buttons are released for (\d+) ms$")]
fn both_released(world: &mut BoardWorld, ms: Tick) {
    world.front.store(false, Ordering::Relaxed);
    world.side.store(false, Ordering::Relaxed);
    world.poll_for(ms);
}

#[when(regex = r"^the power button records a short press$")]
fn power_pressed(world: &mut BoardWorld) {
    *world.power.lock().expect("latch lock") = Some(Gesture::Click);
}

#[when(regex = r"^one poll passes$")]
fn one_poll(world: &mut BoardWorld) {
    world.poll_for(1);
}

#[when(regex = r"^one second passes$")]
fn one_second(world: &mut BoardWorld) {
    world.poll_for(1_000);
}

#[then(regex = r#"^the events are "([^"]*)"$"#)]
fn the_events_are(world: &mut BoardWorld, expected: String) {
    let actual: Vec<String> = world.seen.iter().map(describe).collect();
    let rendered: String = match actual.is_empty() {
        true => "nothing".to_string(),
        false => actual.join(", "),
    };
    assert_eq!(rendered, expected);
}

#[tokio::main]
async fn main() {
    BoardWorld::run("tests/features").await;
}
