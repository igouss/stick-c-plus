//! Device-in-the-loop proof for the bridge — `#[ignore]`d, needs a real `Claude-XXXX` stick.
//!
//! Like the aioesphomeapi oracle, this is `#[ignore]`d on purpose: a plain `cargo test` shows
//! it *ignored*, never a false green, because a bond cannot be faked. It runs the concrete
//! [`BluerCentral`] against a live NUS peripheral over BlueZ (5.87 on this box) and proves the
//! five things the spike must do. Run it with a flashed stick via `just bridge-device`; flash
//! the peer with `just run-bin-buddy ble-spike` first. The spike's fixed passkey is `123456`
//! (override with `STICK_PASSKEY`).
//!
//! The load-bearing security assertion is (5): the passkey callback MUST fire during a fresh
//! pairing. If pairing completes without it, BlueZ silently downgraded to Just Works (no MITM)
//! — a false green this test turns red.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bluer::{Adapter, Address, Session};
use buddy_bridge_shell::{
    build_agent, chunk, discover, BluerCentral, Central, IoCapability, PasskeyProvider,
    RxReassembler,
};
use futures::StreamExt;
use tokio::time::timeout;

/// The advertised-name prefix the firmware uses.
const NAME_PREFIX: &str = "Claude-";

/// A passkey provider that records whether it was asked — the Just-Works-downgrade detector.
struct RecordingPasskey {
    code: u32,
    fired: Arc<AtomicBool>,
}

#[async_trait]
impl PasskeyProvider for RecordingPasskey {
    async fn passkey(&self) -> Option<u32> {
        self.fired.store(true, Ordering::SeqCst);
        Some(self.code)
    }
}

/// The passkey to enter: `STICK_PASSKEY`, or the spike's fixed `123456`.
fn stick_passkey() -> u32 {
    std::env::var("STICK_PASSKEY")
        .ok()
        .and_then(|raw: String| raw.trim().parse::<u32>().ok())
        .unwrap_or(123_456)
}

/// True if `needle` occurs in `haystack`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w: &[u8]| w == needle)
}

/// Pump a subscription until a line containing `needle` arrives (or the timeout elapses),
/// returning that line. Also used to observe the heartbeat resuming after a reboot.
async fn wait_for_line<S>(stream: &mut S, needle: &[u8], within: Duration) -> Option<Vec<u8>>
where
    S: futures::Stream<Item = Vec<u8>> + Unpin,
{
    timeout(within, async {
        let mut reassembler: RxReassembler = RxReassembler::new();
        loop {
            let payload: Vec<u8> = stream.next().await?;
            let lines: Vec<Vec<u8>> = reassembler.accept(&payload).ok()?;
            for line in lines {
                if contains(&line, needle) {
                    return Some(line);
                }
            }
        }
    })
    .await
    .ok()
    .flatten()
}

#[tokio::test]
#[ignore = "device-in-the-loop: needs a real Claude-XXXX stick on BlueZ; run via `just bridge-device`"]
async fn bonds_subscribes_round_trips_and_reconnects() {
    let fired: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let provider: Arc<RecordingPasskey> = Arc::new(RecordingPasskey {
        code: stick_passkey(),
        fired: Arc::clone(&fired),
    });

    let session: Session = Session::new().await.expect("a BlueZ session");
    let adapter: Adapter = session.default_adapter().await.expect("a default adapter");
    adapter
        .set_powered(true)
        .await
        .expect("power the adapter on");

    let (agent, capability): (bluer::agent::Agent, IoCapability) = build_agent(provider);
    let _handle = session
        .register_agent(agent)
        .await
        .expect("register the agent");
    assert_eq!(
        capability,
        IoCapability::KeyboardOnly,
        "the agent must pair as KeyboardOnly, never NoInputNoOutput"
    );

    let address: Address = discover(&adapter, NAME_PREFIX)
        .await
        .expect("find the Claude- stick");
    let mut central: BluerCentral = BluerCentral::new(adapter, address).expect("bind the device");

    // (1) Bond.
    let connected = central.connect().await.expect("connect + resolve GATT");
    if !connected.already_paired {
        central.pair().await.expect("pair as initiator");
        // (5) The security assertion: a fresh pairing MUST have gone through the passkey
        // callback. No callback ⇒ BlueZ downgraded to Just Works ⇒ no MITM protection.
        assert!(
            fired.load(Ordering::SeqCst),
            "pairing completed WITHOUT the passkey callback — silent Just-Works downgrade"
        );
    }

    // (2) Subscribe, and see an unsolicited heartbeat (proves notify + reassembly, including
    // the device's separate-`\n` notification).
    let mut tx = central.subscribe_tx().await.expect("subscribe to TX");
    let heartbeat: Vec<u8> = wait_for_line(&mut tx, b"alive", Duration::from_secs(12))
        .await
        .expect("an {\"alive\":N} heartbeat within 12s");
    assert!(contains(&heartbeat, b"alive"));

    // (3) Round-trip: write a line long enough to force >mtu-3 fragmentation; the echoing
    // spike returns it, and it must reassemble byte-for-byte.
    let mtu: u16 = central.mtu().await.expect("negotiated MTU");
    let line: String = format!("{{\"echo\":\"{}\"}}", "x".repeat(64));
    let mut framed: Vec<u8> = line.clone().into_bytes();
    framed.push(b'\n');
    for piece in chunk(&framed, mtu) {
        central.write_rx(piece).await.expect("write an RX chunk");
    }
    let echoed: Vec<u8> = wait_for_line(&mut tx, line.as_bytes(), Duration::from_secs(8))
        .await
        .expect("the echoed line within 8s");
    assert_eq!(echoed, line.as_bytes(), "the echo must reassemble exactly");

    // (4) Reconnect: the operator power-cycles the stick. The LTK survives a reboot, so the
    // link recovers with NO passkey re-entry.
    eprintln!("\n>>> POWER-CYCLE THE STICK NOW — it should reconnect with no passkey <<<\n");
    fired.store(false, Ordering::SeqCst);
    // Drain until the stream ends (disconnect). BlueZ caches GATT objects for a bonded device,
    // so the raw notify stream would never end on a reboot; the fixed `subscribe_tx` closes it on
    // the `Connected(false)` event. The timeout is a backstop so a regression can never hang CI.
    let drained: Result<(), _> = timeout(Duration::from_secs(30), async {
        while tx.next().await.is_some() {}
    })
    .await;
    assert!(
        drained.is_ok(),
        "TX stream did not end within 30s of the device rebooting — subscribe_tx is not \
         closing the stream on disconnect (BlueZ GATT-cache hang)"
    );
    drop(tx);

    let mut resumed: bool = false;
    for _attempt in 0..30u32 {
        if central.connect().await.is_ok() {
            if let Ok(mut tx2) = central.subscribe_tx().await {
                if wait_for_line(&mut tx2, b"alive", Duration::from_secs(15))
                    .await
                    .is_some()
                {
                    resumed = true;
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    assert!(resumed, "the bridge did not reconnect after a reboot");
    assert!(
        !fired.load(Ordering::SeqCst),
        "reconnect re-entered the passkey — the bonded LTK should survive a reboot"
    );
}
