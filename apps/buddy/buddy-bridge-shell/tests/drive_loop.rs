//! The reconnect/recovery flow, proven on the host against a fake `Central` — no device.
//!
//! Each scenario scripts the transport's outcomes and asserts the loop's decisions: the happy
//! paths (fresh pair, already-paired subscribe), the stale-LTK recovery (remove + re-acquire +
//! re-pair), a reboot (stream ends → backoff → reconnect), and the fatal missing-agent stop.
//! One straight-line sequence per test (cyclomatic complexity 1).

use std::collections::VecDeque;

use async_trait::async_trait;
use buddy_bridge_core::{Action, State};
use buddy_bridge_shell::{Central, CentralError, Connected, DriveLoop, NoSleep, Step};
use futures::stream::{self, Iter};

/// A finite payload stream — models a subscription that ends when the link drops.
type Payloads = Iter<std::vec::IntoIter<Vec<u8>>>;

/// A scripted `Central`: each method pops its next result and records the call, so a test both
/// drives the loop and asserts the exact sequence of operations it performed.
#[derive(Default)]
struct FakeCentral {
    connect: VecDeque<Result<Connected, CentralError>>,
    pair: VecDeque<Result<(), CentralError>>,
    remove: VecDeque<Result<(), CentralError>>,
    subscribe: VecDeque<Result<Vec<Vec<u8>>, CentralError>>,
    calls: Vec<&'static str>,
}

#[async_trait]
impl Central for FakeCentral {
    type Tx = Payloads;

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

#[tokio::test]
async fn a_fresh_device_pairs_then_subscribes() {
    let mut fake: FakeCentral = FakeCentral::default();
    fake.connect.push_back(Ok(Connected {
        already_paired: false,
    }));
    fake.pair.push_back(Ok(()));
    let mut driver: DriveLoop<FakeCentral, NoSleep> = DriveLoop::new(fake, NoSleep);

    // connect → ConnectedFresh → the loop decides to pair.
    assert_eq!(driver.step().await, Step::Continue);
    assert_eq!(driver.pending(), &Action::Pair);
    // pair → LinkEncrypted → the loop decides to subscribe.
    driver.step().await;
    assert_eq!(driver.pending(), &Action::Subscribe);
}

#[tokio::test]
async fn an_already_paired_device_reaches_the_running_state() {
    let mut fake: FakeCentral = FakeCentral::default();
    fake.connect.push_back(Ok(Connected {
        already_paired: true,
    }));
    fake.subscribe
        .push_back(Ok(vec![b"{\"alive\":1}\n".to_vec()]));
    let mut driver: DriveLoop<FakeCentral, NoSleep> = DriveLoop::new(fake, NoSleep);

    // connect → ConnectedPaired → subscribe (no pairing).
    driver.step().await;
    assert_eq!(driver.pending(), &Action::Subscribe);
    // subscribe → NotifySubscribed → run, and the steady state is reached.
    driver.step().await;
    assert_eq!(driver.pending(), &Action::Run);
    assert_eq!(driver.state(), State::Subscribed);
}

#[tokio::test]
async fn a_stale_bond_is_removed_reacquired_and_repaired() {
    let mut fake: FakeCentral = FakeCentral::default();
    // Already paired at connect, but the link will not encrypt (stale LTK).
    fake.connect.push_back(Ok(Connected {
        already_paired: true,
    }));
    fake.subscribe
        .push_back(Err(CentralError::EncryptionFailed));
    fake.remove.push_back(Ok(()));
    // After remove + re-acquire, a fresh connect is genuinely unpaired.
    fake.connect.push_back(Ok(Connected {
        already_paired: false,
    }));
    fake.pair.push_back(Ok(()));
    let mut driver: DriveLoop<FakeCentral, NoSleep> = DriveLoop::new(fake, NoSleep);

    driver.step().await; // connect → subscribe
    driver.step().await; // subscribe fails encryption → remove + re-acquire
    assert_eq!(driver.pending(), &Action::RemoveDeviceThenReacquire);
    driver.step().await; // remove → reacquired → connect
    assert_eq!(driver.pending(), &Action::Connect);
    driver.step().await; // connect (fresh) → pair
    assert_eq!(driver.pending(), &Action::Pair);
}

#[tokio::test]
async fn a_reboot_ends_the_stream_and_the_loop_reconnects() {
    let mut fake: FakeCentral = FakeCentral::default();
    fake.connect.push_back(Ok(Connected {
        already_paired: true,
    }));
    // One heartbeat, then the finite stream ends — the device rebooted.
    fake.subscribe
        .push_back(Ok(vec![b"{\"alive\":1}\n".to_vec()]));
    fake.connect.push_back(Ok(Connected {
        already_paired: true,
    }));
    let mut driver: DriveLoop<FakeCentral, NoSleep> = DriveLoop::new(fake, NoSleep);

    driver.step().await; // connect → subscribe
    driver.step().await; // subscribe → run
    driver.step().await; // run pumps the stream to its end → backoff
    assert!(matches!(driver.pending(), Action::Backoff(_)));
    driver.step().await; // backoff elapsed → connect again
    assert_eq!(driver.pending(), &Action::Connect);
}

#[tokio::test]
async fn a_missing_agent_stops_the_loop_instead_of_retrying_forever() {
    let mut fake: FakeCentral = FakeCentral::default();
    fake.connect.push_back(Err(CentralError::AgentMissing));
    let mut driver: DriveLoop<FakeCentral, NoSleep> = DriveLoop::new(fake, NoSleep);

    // run() would loop forever on a retryable fault; a missing agent must terminate it.
    let reason: String = driver.run().await;
    assert!(
        reason.contains("agent"),
        "the stop reason names the cause: {reason}"
    );
}
