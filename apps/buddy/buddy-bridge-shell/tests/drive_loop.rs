//! The reconnect/recovery flow, proven on the host against a fake `Central` — no device.
//!
//! Each scenario scripts the transport's outcomes and asserts the loop's decisions: the happy
//! paths (fresh pair, already-paired subscribe), the stale-LTK recovery (remove + re-acquire +
//! re-pair), a reboot (stream ends → backoff → reconnect), and the fatal missing-agent stop.
//! One straight-line sequence per test (cyclomatic complexity 1).

use std::collections::VecDeque;

use async_trait::async_trait;
use buddy_bridge_core::{Action, State};
use buddy_bridge_shell::{Central, CentralError, Connected, DriveLoop, LinkPeer, NoSleep, Step};
use futures::stream::{self, Iter};
use tokio::sync::mpsc::{self, UnboundedReceiver};

/// A finite payload stream — models a subscription that ends when the link drops.
type Payloads = Iter<std::vec::IntoIter<Vec<u8>>>;

/// A do-nothing [`LinkPeer`] for the FSM tests: it consumes device lines and has nothing to send,
/// so its outbound channel is closed from the start (the loop pumps TX only). These tests assert the
/// reconnect DECISIONS, not the link bridge — that is `link_bridge.rs`.
struct NullPeer;

impl LinkPeer for NullPeer {
    fn on_up(&self) -> UnboundedReceiver<String> {
        // A closed receiver: no lines to write, the outbound branch disables itself at once.
        let (_tx, rx): (mpsc::UnboundedSender<String>, UnboundedReceiver<String>) =
            mpsc::unbounded_channel();
        rx
    }
    fn on_line(&self, _line: Vec<u8>) {}
    fn on_down(&self) {}
}

/// A scripted `Central`: each method pops its next result and records the call, so a test both
/// drives the loop and asserts the exact sequence of operations it performed.
#[derive(Default)]
struct FakeCentral {
    locate: VecDeque<Result<(), CentralError>>,
    connect: VecDeque<Result<Connected, CentralError>>,
    pair: VecDeque<Result<(), CentralError>>,
    remove: VecDeque<Result<(), CentralError>>,
    subscribe: VecDeque<Result<Vec<Vec<u8>>, CentralError>>,
    calls: Vec<&'static str>,
}

#[async_trait]
impl Central for FakeCentral {
    type Tx = Payloads;

    /// An unscripted locate SUCCEEDS: every attempt now begins with one, and the tests that
    /// predate it are about what happens once the device has been found.
    async fn locate(&mut self) -> Result<(), CentralError> {
        self.calls.push("locate");
        self.locate.pop_front().unwrap_or(Ok(()))
    }

    async fn connect(&mut self) -> Result<Connected, CentralError> {
        self.calls.push("connect");
        self.connect
            .pop_front()
            .unwrap_or(Err(CentralError::NotConnected))
    }

    async fn pair(&mut self) -> Result<(), CentralError> {
        self.calls.push("pair");
        self.pair
            .pop_front()
            .unwrap_or(Err(CentralError::NotConnected))
    }

    async fn remove_and_reacquire(&mut self) -> Result<(), CentralError> {
        self.calls.push("remove");
        self.remove
            .pop_front()
            .unwrap_or(Err(CentralError::NotConnected))
    }

    async fn mtu(&self) -> Result<u16, CentralError> {
        Ok(23)
    }

    async fn subscribe_tx(&mut self) -> Result<Self::Tx, CentralError> {
        self.calls.push("subscribe");
        match self
            .subscribe
            .pop_front()
            .unwrap_or(Err(CentralError::NotConnected))
        {
            Ok(payloads) => Ok(stream::iter(payloads)),
            Err(err) => Err(err),
        }
    }

    async fn write_rx(&self, _chunk: &[u8]) -> Result<(), CentralError> {
        Ok(())
    }
}

/// A connect outcome for a device BlueZ already holds a bond for.
fn paired() -> Result<Connected, CentralError> {
    Ok(Connected {
        already_paired: true,
    })
}

/// A connect outcome for a device with no bond — the one that leads to pairing.
fn unpaired() -> Result<Connected, CentralError> {
    Ok(Connected {
        already_paired: false,
    })
}

/// Play one round that connects an already-bonded device and then fails to encrypt, leaving the
/// loop at whatever the FSM decided. Used to walk up to the re-bond threshold without a loop.
async fn an_encryption_failure(driver: &mut DriveLoop<FakeCentral, NoSleep, NullPeer>) {
    driver.step().await; // locate → connect
    driver.step().await; // connect (paired) → subscribe
    driver.step().await; // subscribe fails to encrypt
}

#[tokio::test]
async fn a_fresh_device_pairs_then_subscribes() {
    let mut fake: FakeCentral = FakeCentral::default();
    fake.connect.push_back(unpaired());
    fake.pair.push_back(Ok(()));
    let mut driver: DriveLoop<FakeCentral, NoSleep, NullPeer> =
        DriveLoop::new(fake, NoSleep, NullPeer);

    // locate → Located → the loop decides to connect.
    assert_eq!(driver.step().await, Step::Continue);
    assert_eq!(driver.pending(), &Action::Connect);
    // connect → ConnectedFresh → the loop decides to pair.
    driver.step().await;
    assert_eq!(driver.pending(), &Action::Pair);
    // pair → LinkEncrypted → the loop decides to subscribe.
    driver.step().await;
    assert_eq!(driver.pending(), &Action::Subscribe);
}

#[tokio::test]
async fn an_already_paired_device_reaches_the_running_state() {
    let mut fake: FakeCentral = FakeCentral::default();
    fake.connect.push_back(paired());
    fake.subscribe
        .push_back(Ok(vec![b"{\"alive\":1}\n".to_vec()]));
    let mut driver: DriveLoop<FakeCentral, NoSleep, NullPeer> =
        DriveLoop::new(fake, NoSleep, NullPeer);

    driver.step().await; // locate → connect
                         // connect → ConnectedPaired → subscribe (no pairing).
    driver.step().await;
    assert_eq!(driver.pending(), &Action::Subscribe);
    // subscribe → NotifySubscribed → run, and the steady state is reached.
    driver.step().await;
    assert_eq!(driver.pending(), &Action::Run);
    assert_eq!(driver.state(), State::Subscribed);
}

/// The stick is switched off, so no scan ever finds it. The loop must keep looking — and must
/// NOT reach for the bond-destroying recovery, because absence is not evidence of a stale bond.
/// This is the regression that made "bond once, and forever" impossible: the old daemon read a
/// failed connect on a bonded device as a stale LTK and removed the bond every time.
#[tokio::test]
async fn a_device_that_is_away_is_waited_for_and_keeps_its_bond() {
    let mut fake: FakeCentral = FakeCentral::default();
    fake.locate.push_back(Err(CentralError::NotFound));
    fake.locate.push_back(Err(CentralError::NotFound));
    fake.locate.push_back(Err(CentralError::NotFound));
    let mut driver: DriveLoop<FakeCentral, NoSleep, NullPeer> =
        DriveLoop::new(fake, NoSleep, NullPeer);

    driver.step().await; // locate finds nothing → backoff
    assert!(matches!(driver.pending(), Action::Backoff(_)));
    driver.step().await; // backoff elapsed → locate again
    assert_eq!(driver.pending(), &Action::Locate);
    driver.step().await; // still nothing → backoff
    driver.step().await; // backoff elapsed → locate again
    assert_eq!(
        driver.pending(),
        &Action::Locate,
        "an absent stick is waited for indefinitely, never re-bonded"
    );
}

/// Three consecutive encryption failures ARE conclusive, so the bond is given up — and the
/// re-acquire must then locate afresh, because `remove_device` evicted the device from the stack.
#[tokio::test]
async fn a_persistent_stale_bond_is_given_up_then_located_and_repaired() {
    let mut fake: FakeCentral = FakeCentral::default();
    // Three rounds: already paired at connect, but the link will not encrypt (stale LTK).
    fake.connect.push_back(paired());
    fake.connect.push_back(paired());
    fake.connect.push_back(paired());
    fake.subscribe
        .push_back(Err(CentralError::EncryptionFailed));
    fake.subscribe
        .push_back(Err(CentralError::EncryptionFailed));
    fake.subscribe
        .push_back(Err(CentralError::EncryptionFailed));
    fake.remove.push_back(Ok(()));
    // After the bond is dropped, a fresh connect is genuinely unpaired.
    fake.connect.push_back(unpaired());
    fake.pair.push_back(Ok(()));
    let mut driver: DriveLoop<FakeCentral, NoSleep, NullPeer> =
        DriveLoop::new(fake, NoSleep, NullPeer);

    an_encryption_failure(&mut driver).await;
    assert!(
        matches!(driver.pending(), Action::Backoff(_)),
        "one failure keeps the bond"
    );
    driver.step().await; // backoff elapsed → locate
    an_encryption_failure(&mut driver).await;
    assert!(
        matches!(driver.pending(), Action::Backoff(_)),
        "two failures still keep the bond"
    );
    driver.step().await; // backoff elapsed → locate
    an_encryption_failure(&mut driver).await;
    assert_eq!(driver.pending(), &Action::RemoveDeviceThenReacquire);

    driver.step().await; // remove → Reacquired → locate (the device was evicted)
    assert_eq!(
        driver.pending(),
        &Action::Locate,
        "remove_device evicted the device; it must be discovered again before connecting"
    );
    driver.step().await; // locate → connect
    driver.step().await; // connect (fresh) → pair
    assert_eq!(driver.pending(), &Action::Pair);
}

#[tokio::test]
async fn a_reboot_ends_the_stream_and_the_loop_reconnects() {
    let mut fake: FakeCentral = FakeCentral::default();
    fake.connect.push_back(paired());
    // One heartbeat, then the finite stream ends — the device rebooted.
    fake.subscribe
        .push_back(Ok(vec![b"{\"alive\":1}\n".to_vec()]));
    fake.connect.push_back(paired());
    let mut driver: DriveLoop<FakeCentral, NoSleep, NullPeer> =
        DriveLoop::new(fake, NoSleep, NullPeer);

    driver.step().await; // locate → connect
    driver.step().await; // connect → subscribe
    driver.step().await; // subscribe → run
    driver.step().await; // run pumps the stream to its end → backoff
    assert!(matches!(driver.pending(), Action::Backoff(_)));
    driver.step().await; // backoff elapsed → locate again
    assert_eq!(driver.pending(), &Action::Locate);
    driver.step().await; // locate → connect, and the bond is reused
    assert_eq!(driver.pending(), &Action::Connect);
}

#[tokio::test]
async fn a_missing_agent_stops_the_loop_instead_of_retrying_forever() {
    let mut fake: FakeCentral = FakeCentral::default();
    fake.connect.push_back(Err(CentralError::AgentMissing));
    let mut driver: DriveLoop<FakeCentral, NoSleep, NullPeer> =
        DriveLoop::new(fake, NoSleep, NullPeer);

    // run() would loop forever on a retryable fault; a missing agent must terminate it.
    let reason: String = driver.run().await;
    assert!(
        reason.contains("agent"),
        "the stop reason names the cause: {reason}"
    );
}
