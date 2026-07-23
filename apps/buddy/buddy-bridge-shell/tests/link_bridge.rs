//! The link bridge in `on_run`, proven on the host — no device. Two directions, each deterministic:
//! a device notification reassembles and reaches the [`LinkPeer`] (`on_line`), and a line the peer
//! emits is framed with `\n`, chunked, and written down RX. The FSM decisions are `drive_loop.rs`;
//! here we prove only that the running link carries bytes both ways and announces `on_down`.

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use buddy_bridge_shell::{Central, CentralError, Connected, DriveLoop, LinkPeer, NoSleep};
use futures::stream::{self, Stream, StreamExt};
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::sync::oneshot;

/// A boxed TX stream — models a subscription's raw notifications, ending when the link drops.
type BoxTx = Pin<Box<dyn Stream<Item = Vec<u8>> + Send>>;

/// A `Central` scripted for the bridge: it hands out one prepared TX stream and records every RX
/// write. To make the write-direction test deterministic, a write that completes a line (`\n`) fires
/// the optional `end_on_write` signal — the stream awaits it, so the stream ends ONLY after the peer's
/// line has been written, never racing the select.
struct ScriptCentral {
    stream: Mutex<Option<BoxTx>>,
    written: Arc<Mutex<Vec<u8>>>,
    end_on_write: Mutex<Option<oneshot::Sender<()>>>,
}

#[async_trait]
impl Central for ScriptCentral {
    type Tx = BoxTx;

    /// Always found: these scenarios are about what a RUNNING link carries, not how it is found.
    async fn locate(&mut self) -> Result<(), CentralError> {
        Ok(())
    }

    async fn connect(&mut self) -> Result<Connected, CentralError> {
        Ok(Connected {
            already_paired: true,
        })
    }

    async fn pair(&mut self) -> Result<(), CentralError> {
        Ok(())
    }

    async fn remove_and_reacquire(&mut self) -> Result<(), CentralError> {
        Ok(())
    }

    async fn mtu(&self) -> Result<u16, CentralError> {
        Ok(23)
    }

    async fn subscribe_tx(&mut self) -> Result<Self::Tx, CentralError> {
        Ok(self.stream.lock().unwrap().take().expect("subscribe once"))
    }

    async fn write_rx(&self, chunk: &[u8]) -> Result<(), CentralError> {
        let mut written: std::sync::MutexGuard<'_, Vec<u8>> = self.written.lock().unwrap();
        written.extend_from_slice(chunk);
        // A completed line ends the stream, so the write is guaranteed to precede the link drop.
        if written.contains(&b'\n') {
            if let Some(end) = self.end_on_write.lock().unwrap().take() {
                let _ = end.send(());
            }
        }
        Ok(())
    }
}

/// A recording [`LinkPeer`]: it offers a fixed set of lines to write (its outbound channel, closed
/// once they are queued), records every device line handed up, and flags the link-down.
struct RecordingPeer {
    outbound: Mutex<Option<UnboundedReceiver<String>>>,
    received: Arc<Mutex<Vec<Vec<u8>>>>,
    down: Arc<AtomicBool>,
}

impl RecordingPeer {
    /// A peer that will send `to_send` (then close its outbound) and record what it receives.
    fn new(
        to_send: Vec<String>,
        received: Arc<Mutex<Vec<Vec<u8>>>>,
        down: Arc<AtomicBool>,
    ) -> Self {
        let (tx, rx): (mpsc::UnboundedSender<String>, UnboundedReceiver<String>) =
            mpsc::unbounded_channel();
        to_send.into_iter().for_each(|line: String| {
            let _ = tx.send(line);
        });
        // `tx` drops here: the queued lines remain, then the channel closes.
        RecordingPeer {
            outbound: Mutex::new(Some(rx)),
            received,
            down,
        }
    }
}

impl LinkPeer for RecordingPeer {
    fn on_up(&self) -> UnboundedReceiver<String> {
        self.outbound.lock().unwrap().take().expect("on_up once")
    }
    fn on_line(&self, line: Vec<u8>) {
        self.received.lock().unwrap().push(line);
    }
    fn on_down(&self) {
        self.down.store(true, Ordering::SeqCst);
    }
}

/// Drive an already-paired link to its running state, then run the bridge to the link's end.
async fn run_to_link_end(central: ScriptCentral, peer: RecordingPeer) {
    let mut driver: DriveLoop<ScriptCentral, NoSleep, RecordingPeer> =
        DriveLoop::new(central, NoSleep, peer);
    driver.step().await; // locate → connect
    driver.step().await; // connect (paired) → subscribe
    driver.step().await; // subscribe → run
    driver.step().await; // on_run: pump the link until its stream ends
}

/// A device notification, fragmented and then terminated by a lone newline (the real quirk),
/// reassembles into one whole line and reaches the peer; the link-down is announced.
#[tokio::test]
async fn a_device_notification_reassembles_and_reaches_the_peer() {
    let received: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let down: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    // The device sends the line in three notifications; a plain finite stream ends after them.
    let payloads: Vec<Vec<u8>> = vec![b"{\"hi\":".to_vec(), b"1}".to_vec(), b"\n".to_vec()];
    let central: ScriptCentral = ScriptCentral {
        stream: Mutex::new(Some(stream::iter(payloads).boxed())),
        written: Arc::new(Mutex::new(Vec::new())),
        end_on_write: Mutex::new(None),
    };
    // The peer has nothing to send: its outbound closes at once, so only the TX stream drives.
    let peer: RecordingPeer = RecordingPeer::new(Vec::new(), received.clone(), down.clone());
    run_to_link_end(central, peer).await;
    assert_eq!(*received.lock().unwrap(), vec![b"{\"hi\":1}".to_vec()]);
    assert!(down.load(Ordering::SeqCst), "the link-down was announced");
}

/// A line the peer emits is framed with `\n` and written down RX; the stream is held open until that
/// write lands, so the assertion is deterministic (no select race with the link drop).
#[tokio::test]
async fn a_peer_line_is_framed_and_written_down_the_link() {
    let written: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let received: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let down: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    // The stream yields no device items, then awaits the write signal before ending — so the peer's
    // line is forced to be written before the link drops.
    let (end_tx, end_rx): (oneshot::Sender<()>, oneshot::Receiver<()>) = oneshot::channel();
    let stream: BoxTx = stream::once(async move {
        let _ = end_rx.await;
    })
    .filter_map(|_unit: ()| async { None })
    .boxed();
    let central: ScriptCentral = ScriptCentral {
        stream: Mutex::new(Some(stream)),
        written: written.clone(),
        end_on_write: Mutex::new(Some(end_tx)),
    };
    let peer: RecordingPeer = RecordingPeer::new(
        vec!["{\"snapshot\":1}".to_string()],
        received.clone(),
        down.clone(),
    );
    run_to_link_end(central, peer).await;
    assert_eq!(*written.lock().unwrap(), b"{\"snapshot\":1}\n".to_vec());
    assert!(down.load(Ordering::SeqCst), "the link-down was announced");
}
