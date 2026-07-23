//! Device-in-the-loop proof for the bridge — `#[ignore]`d, needs a real `Claude-XXXX` stick.
//!
//! Like the aioesphomeapi oracle, this is `#[ignore]`d on purpose: a plain `cargo test` shows
//! it *ignored*, never a false green, because a bond cannot be faked. It runs the concrete
//! [`BluerCentral`] against a live NUS peripheral over BlueZ (5.87 on this box) and proves the
//! five things the link must do. Run it with a flashed stick via `just bridge-device`; flash the
//! peer with `just run-buddy` first, and set `STICK_PASSKEY` to the six digits that stick's glass
//! shows for this pairing — the firmware draws a fresh one each time, so there is no constant.
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
    build_agent, chunk, BluerCentral, Central, IoCapability, PasskeyProvider, RxReassembler,
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

/// The passkey to enter, from `STICK_PASSKEY` — read off the stick's glass for this pairing.
///
/// There is deliberately **no fallback**. The firmware draws a fresh passkey per pairing, so any
/// constant this test could guess is wrong, and a wrong guess fails as a *pairing* error — which
/// reads like a broken bond rather than an unset variable and sends the operator hunting the wrong
/// fault. Missing input is a missing input: say so, by name, before touching the adapter.
fn stick_passkey() -> u32 {
    let raw: String = std::env::var("STICK_PASSKEY").unwrap_or_else(|_| {
        panic!(
            "STICK_PASSKEY is unset. The firmware shows a FRESH random passkey on the glass for \
             each pairing — there is no constant to fall back on. Read the six digits off the \
             stick and re-run with STICK_PASSKEY=<digits>."
        )
    });
    raw.trim()
        .parse::<u32>()
        .unwrap_or_else(|err| panic!("STICK_PASSKEY={raw:?} is not a six-digit number: {err}"))
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

    let mut central: BluerCentral = BluerCentral::new(adapter, NAME_PREFIX);

    // (0) Find it. This is the step that must work from COLD — with no prior `bluetoothctl scan`
    // and nothing in BlueZ's cache — which is exactly what the old discovery could not do.
    central.locate().await.expect("find the Claude- stick");
    let address: Address = central.address().expect("a located device has an address");
    eprintln!("located {address}");

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
