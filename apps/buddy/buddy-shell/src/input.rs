//! The input thread: poll the buttons, fold each press into the state, carry out the effect.
//!
//! The one loop that turns a finger on a button into a line on the wire. It owns no policy —
//! which button answers what is [`DeviceState::press`](crate::DeviceState::press)'s business, and
//! the effect it hands back is *data* — so this file is a straight line: poll, fold, act.
//!
//! ## Fast, because someone is waiting
//!
//! A permission prompt blocks a tool call on the owner's machine. The poll period is therefore
//! the board's floor rather than a comfortable tick: at `CONFIG_FREERTOS_HZ = 100` a sleep under
//! ten milliseconds busy-waits instead of yielding, so ten is as fast as this loop may go, and it
//! goes exactly that fast.
//!
//! ## A refused notify is logged, not retried
//!
//! If the link drops between the press and the send, the answer is gone. That is *correct*: the
//! hook host-side holds its own deadline and falls through to the owner's normal permissions when
//! it expires, so a device that queued the answer and resent it on reconnect would be answering a
//! question that has already been decided — the one failure mode worse than not answering.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use buddy_wire::PermissionResponse;
use log::{info, warn};
use platform_core::{Clock, Tick};
use platform_input::{
    ButtonLevel, Buttons, GestureConfig, Gestures, InputConfig, LatchedGesture, DEBOUNCE_MS,
    DOUBLE_CLICK_MS, HOLD_MS,
};

use crate::ports::{Bond, Notifier, NotifyError, SpeciesStore};
use crate::shared::SharedDevice;
use crate::state::{DeviceState, Effect};

/// How often the buttons are polled.
///
/// The board's tick floor, and deliberately at it — see the module docs.
pub const POLL_PERIOD: Duration = Duration::from_millis(10);

/// The input thread's stack, in bytes.
///
/// It folds a press through the domain and serialises one short JSON line; nothing here
/// recurses or buffers. Four kibibytes, like the other input loops on this board, and validated
/// against the high-water mark on the metal before it is trusted.
pub const INPUT_STACK_SIZE: usize = 4 * 1024;

/// The gesture timings the buddy recognises.
///
/// Neither button asks for double-clicks, and that is a decision about the *headline feature*
/// rather than a default taken by accident: a double-click window holds a lone click back until
/// it has expired, and A is the button that answers a tool call somebody's machine is blocked
/// on. A prompt-reporting button answers the instant it is released.
pub const INPUT_CONFIG: InputConfig = InputConfig {
    front: Gestures::Prompt,
    side: Gestures::Prompt,
    timing: GestureConfig {
        debounce_ms: DEBOUNCE_MS,
        hold_ms: HOLD_MS,
        double_click_ms: DOUBLE_CLICK_MS,
    },
};

/// A running input thread — a handle to stop and join it.
pub struct InputTask {
    handle: JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

impl InputTask {
    /// Ask the loop to finish after its current cycle.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Block until the thread has exited, propagating a panic it carried.
    pub fn join(self) -> thread::Result<()> {
        self.handle.join()
    }
}

/// Spawn the input thread.
///
/// `buttons` is the assembled front/side/power aggregate; `shared` the one device state;
/// `notifier`, `bond` and `species` the driven ports the effects are carried out through.
pub fn spawn_input<F, S, P, C, N, B, T>(
    buttons: Buttons<F, S, P>,
    shared: SharedDevice,
    clock: C,
    notifier: N,
    bond: B,
    species: T,
) -> io::Result<InputTask>
where
    F: ButtonLevel + Send + 'static,
    S: ButtonLevel + Send + 'static,
    P: LatchedGesture + Send + 'static,
    C: Clock + Send + 'static,
    N: Notifier + Send + 'static,
    B: Bond + Send + 'static,
    T: SpeciesStore + Send + 'static,
{
    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let stop_in_thread: Arc<AtomicBool> = Arc::clone(&stop);
    let handle: JoinHandle<()> = thread::Builder::new()
        .name("buddy-input".to_string())
        .stack_size(INPUT_STACK_SIZE)
        .spawn(move || {
            input_loop(
                buttons,
                shared,
                clock,
                notifier,
                bond,
                species,
                stop_in_thread,
            )
        })?;
    Ok(InputTask { handle, stop })
}

/// The thread body: poll, fold, act — until asked to stop.
fn input_loop<F, S, P, C, N, B, T>(
    mut buttons: Buttons<F, S, P>,
    shared: SharedDevice,
    clock: C,
    notifier: N,
    bond: B,
    species: T,
    stop: Arc<AtomicBool>,
) where
    F: ButtonLevel,
    S: ButtonLevel,
    P: LatchedGesture,
    C: Clock,
    N: Notifier,
    B: Bond,
    T: SpeciesStore,
{
    while !stop.load(Ordering::Relaxed) {
        let now: Tick = clock.now();
        for event in buttons.poll(now) {
            let effect: Effect = shared.with(|state: &mut DeviceState| state.press(event, now));
            perform(effect, &notifier, &bond, &species);
        }
        thread::sleep(POLL_PERIOD);
    }
}

/// Perform one effect. The only place in the shell that touches the outside world.
///
/// Public because it is the *whole* of what the thread does with an [`Effect`], and a test that
/// hand-rolled the same match would be proving its own copy rather than this one.
pub fn perform<N, B, T>(effect: Effect, notifier: &N, bond: &B, species: &T)
where
    N: Notifier,
    B: Bond,
    T: SpeciesStore,
{
    match effect {
        Effect::None => {}
        Effect::Answer(response) => answer(response, notifier),
        Effect::ForgetBond => {
            info!("buddy-input: forgetting the bond on the owner's confirmation");
            bond.forget();
        }
        Effect::PersistSpecies(index) => {
            info!("buddy-input: species now #{}", index.get());
            species.store(index.get());
        }
    }
}

/// Put one permission response on the wire, and say so either way.
///
/// The log line is the observable half of the headline feature: a press that produced no line is
/// otherwise indistinguishable, from the outside, from a press that was never noticed.
fn answer<N: Notifier>(response: PermissionResponse, notifier: &N) {
    let line: String = response.to_json();
    match notifier.notify(&line) {
        Ok(()) => info!(
            "buddy-input: answered prompt {} -> {}",
            response.id,
            response.decision.as_wire()
        ),
        Err(NotifyError { reason }) => warn!(
            "buddy-input: prompt {} answered {} but the line did not leave: {reason} — the \
             host's own deadline decides now",
            response.id,
            response.decision.as_wire()
        ),
    }
}
