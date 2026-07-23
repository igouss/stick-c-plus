//! The generic loop that runs the pure decision against a real (or fake) `Central`.
//!
//! This is the imperative shell's one job: perform the [`Action`] the FSM last chose, turn the
//! transport's outcome into the [`Event`] the FSM expects, and step again. Because it is
//! written against the [`Central`] and [`Sleeper`] ports — never bluer — the entire
//! reconnect/recovery flow (happy path, stale-LTK recovery, reboot, missing agent) is proven
//! on the host against a fake; only the concrete `BluerCentral` needs the device.

use buddy_bridge_core::{Action, Event, Fsm, State};
use futures::StreamExt;
use log::{info, warn};

use crate::central::{Central, CentralError, Connected};
use crate::reassembler::RxReassembler;
use crate::sleeper::Sleeper;

/// The result of one [`DriveLoop::step`]: keep going, or stop with the reason a
/// [`Action::FailFast`] gave.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Step {
    /// The step completed; call [`DriveLoop::step`] again.
    Continue,
    /// The loop hit an unrecoverable condition and will not retry.
    Stopped(String),
}

/// Runs a [`Fsm`] against a [`Central`] + [`Sleeper`], carrying the one bit of state the FSM
/// does not: the live TX stream between a successful subscribe and the pump that consumes it.
pub struct DriveLoop<C: Central, S: Sleeper> {
    fsm: Fsm,
    central: C,
    sleeper: S,
    tx: Option<C::Tx>,
    action: Action,
}

impl<C: Central, S: Sleeper> DriveLoop<C, S> {
    /// A loop primed with the first [`Action::Connect`].
    pub fn new(central: C, sleeper: S) -> Self {
        let mut fsm: Fsm = Fsm::new();
        let action: Action = fsm.start();
        DriveLoop {
            fsm,
            central,
            sleeper,
            tx: None,
            action,
        }
    }

    /// The FSM's current state (for observability/tests).
    pub fn state(&self) -> State {
        self.fsm.state()
    }

    /// The action the loop will perform next (for observability/tests).
    pub fn pending(&self) -> &Action {
        &self.action
    }

    /// Run until an [`Action::FailFast`], returning its reason. A well-behaved link never
    /// stops; only a fatal misconfiguration (a missing pairing agent) ends the loop.
    pub async fn run(&mut self) -> String {
        loop {
            match self.step().await {
                Step::Continue => {}
                Step::Stopped(reason) => return reason,
            }
        }
    }

    /// Perform the pending action, feed its outcome to the FSM, and adopt the next action.
    pub async fn step(&mut self) -> Step {
        let event: Event = match self.action.clone() {
            Action::FailFast(reason) => return Step::Stopped(reason.to_string()),
            Action::Connect => self.on_connect().await,
            Action::Pair => self.on_pair().await,
            Action::RemoveDeviceThenReacquire => self.on_remove_and_reacquire().await,
            Action::Subscribe => self.on_subscribe().await,
            Action::Run => self.on_run().await,
            Action::Backoff(dur) => {
                self.sleeper.sleep(dur).await;
                Event::BackoffElapsed
            }
        };
        self.action = self.fsm.on(event);
        Step::Continue
    }

    async fn on_connect(&mut self) -> Event {
        match self.central.connect().await {
            Ok(Connected {
                already_paired: false,
            }) => Event::ConnectedFresh,
            Ok(Connected {
                already_paired: true,
            }) => Event::ConnectedPaired,
            Err(CentralError::EncryptionFailed) => Event::EncryptionFailed,
            Err(CentralError::AgentMissing) => Event::AgentMissing,
            Err(other) => {
                warn!("connect failed: {other}");
                Event::Disconnected
            }
        }
    }

    async fn on_pair(&mut self) -> Event {
        match self.central.pair().await {
            Ok(()) => Event::LinkEncrypted,
            Err(CentralError::AlreadyPaired) => Event::PairRejectedAlreadyPaired,
            Err(CentralError::EncryptionFailed) => Event::EncryptionFailed,
            Err(CentralError::AgentMissing) => Event::AgentMissing,
            Err(other) => {
                warn!("pair failed: {other}");
                Event::Disconnected
            }
        }
    }

    async fn on_remove_and_reacquire(&mut self) -> Event {
        match self.central.remove_and_reacquire().await {
            Ok(()) => Event::Reacquired,
            Err(other) => {
                warn!("stale-bond recovery failed: {other}");
                Event::Disconnected
            }
        }
    }

    async fn on_subscribe(&mut self) -> Event {
        match self.central.subscribe_tx().await {
            Ok(stream) => {
                self.tx = Some(stream);
                Event::NotifySubscribed
            }
            Err(CentralError::EncryptionFailed) => Event::EncryptionFailed,
            Err(other) => {
                warn!("subscribe failed: {other}");
                Event::Disconnected
            }
        }
    }

    /// Pump the TX stream, reassembling notifications into whole lines, until the link ends.
    async fn on_run(&mut self) -> Event {
        let mut stream: C::Tx = self
            .tx
            .take()
            .expect("Run only follows a successful Subscribe");
        let mut reassembler: RxReassembler = RxReassembler::new();
        while let Some(payload) = stream.next().await {
            match reassembler.accept(&payload) {
                Ok(lines) => {
                    for line in lines {
                        info!("rx: {}", String::from_utf8_lossy(&line));
                    }
                }
                Err(truncation) => warn!("reassembly: {truncation}"),
            }
        }
        // The notify stream ends when the link drops (device reboot, out of range).
        Event::Disconnected
    }
}
