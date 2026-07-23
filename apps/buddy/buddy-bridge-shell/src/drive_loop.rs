//! The generic loop that runs the pure decision against a real (or fake) `Central`.
//!
//! This is the imperative shell's one job: perform the [`Action`] the FSM last chose, turn the
//! transport's outcome into the [`Event`] the FSM expects, and step again. Because it is
//! written against the [`Central`] and [`Sleeper`] ports — never bluer — the entire
//! reconnect/recovery flow (happy path, stale-LTK recovery, reboot, missing agent) is proven
//! on the host against a fake; only the concrete `BluerCentral` needs the device.

use buddy_bridge_core::{chunk, Action, Event, Fsm, State};
use futures::StreamExt;
use log::warn;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::central::{Central, CentralError, Connected};
use crate::link_peer::LinkPeer;
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

/// Runs a [`Fsm`] against a [`Central`] + [`Sleeper`], bridging the live link to a [`LinkPeer`],
/// and carrying the one bit of state the FSM does not: the live TX stream between a successful
/// subscribe and the pump that consumes it.
pub struct DriveLoop<C: Central, S: Sleeper, P: LinkPeer> {
    fsm: Fsm,
    central: C,
    sleeper: S,
    peer: P,
    tx: Option<C::Tx>,
    action: Action,
}

impl<C: Central, S: Sleeper, P: LinkPeer> DriveLoop<C, S, P> {
    /// A loop primed with the first [`Action::Connect`], bridging each running link to `peer`.
    pub fn new(central: C, sleeper: S, peer: P) -> Self {
        let mut fsm: Fsm = Fsm::new();
        let action: Action = fsm.start();
        DriveLoop {
            fsm,
            central,
            sleeper,
            peer,
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
            Action::Locate => self.on_locate().await,
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

    /// A device that is simply away is the routine case, not a fault: `NotFound` is reported at
    /// debug level and retried, while a real stack failure during the scan is warned about.
    async fn on_locate(&mut self) -> Event {
        match self.central.locate().await {
            Ok(()) => Event::Located,
            Err(CentralError::NotFound) => Event::NotFound,
            Err(other) => {
                warn!("locate failed: {other}");
                Event::NotFound
            }
        }
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

    /// Bridge the running link to the [`LinkPeer`]: reassemble each TX notification into whole lines
    /// and hand them up (`on_line`), while writing every line the peer emits down the link (chunked
    /// to the negotiated MTU), until the TX stream ends. Announces `on_up` before pumping and
    /// `on_down` after the link drops.
    async fn on_run(&mut self) -> Event {
        let mut stream: C::Tx = self
            .tx
            .take()
            .expect("Run only follows a successful Subscribe");
        // The negotiated MTU sizes the RX chunks; a probe failure degrades to the 23-byte floor
        // rather than dropping the session — a small MTU only writes more, smaller pieces.
        let mtu: u16 = self.central.mtu().await.unwrap_or(MTU_FLOOR);
        let mut reassembler: RxReassembler = RxReassembler::new();
        let mut outbound: UnboundedReceiver<String> = self.peer.on_up();
        // Once the peer's outbound channel closes (the daemon is gone), disable that branch and pump
        // TX only — never busy-loop on a closed receiver.
        let mut outbound_open: bool = true;
        loop {
            tokio::select! {
                payload = stream.next() => match payload {
                    Some(payload) => self.absorb(&mut reassembler, &payload),
                    // The notify stream ends when the link drops (device reboot, out of range).
                    None => break,
                },
                line = outbound.recv(), if outbound_open => match line {
                    Some(line) => self.emit(&line, mtu).await,
                    None => outbound_open = false,
                },
            }
        }
        self.peer.on_down();
        Event::Disconnected
    }

    /// Reassemble one raw TX notification and hand every whole line it completed to the peer. A
    /// typed truncation is logged, never a silent drop.
    fn absorb(&self, reassembler: &mut RxReassembler, payload: &[u8]) {
        match reassembler.accept(payload) {
            Ok(lines) => lines
                .into_iter()
                .for_each(|line: Vec<u8>| self.peer.on_line(line)),
            Err(truncation) => warn!("reassembly: {truncation}"),
        }
    }

    /// Write one outbound line down the link, framed with the device's `\n` terminator and split
    /// into MTU-sized pieces. A failed write abandons this line; the FSM learns of a dropped link
    /// when the TX stream ends.
    async fn emit(&self, line: &str, mtu: u16) {
        let mut framed: Vec<u8> = line.as_bytes().to_vec();
        framed.push(b'\n');
        for piece in chunk(&framed, mtu) {
            if let Err(err) = self.central.write_rx(piece).await {
                warn!("rx write failed: {err}");
                return;
            }
        }
    }
}

/// The ATT MTU floor (payload sizing is `mtu - 3`), used when the MTU probe fails so a session can
/// still make progress with smaller writes.
const MTU_FLOOR: u16 = 23;
