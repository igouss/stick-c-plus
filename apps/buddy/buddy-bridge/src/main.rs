#![forbid(unsafe_code)]
//! `buddy-bridge` — the daemon that is the BLE central Claude Code does not ship.
//!
//! It registers the pairing agent (KeyboardOnly, holding its handle for the run), scans for the
//! `Claude-XXXX` stick, and runs the pure decision (`buddy-bridge-core`) against a live
//! [`BluerCentral`]. This is the composition root: the sole place bluer, stdin, the logger, and
//! the real sleeper are wired together, so every inner crate stays framework-free and tested.
//!
//! Transport-only (bead `bluer-bridge-spike-3zt`): it bonds, subscribes, reassembles, writes,
//! and reconnects. The permission SEMANTICS (mapping snapshots ↔ the glass, approving a real
//! tool call) land in `buddy-permission-flow-es6`.

use std::sync::Arc;

use async_trait::async_trait;
use bluer::agent::Agent;
use bluer::{Adapter, Address, Session};
use buddy_bridge_shell::{
    build_agent, discover, BluerCentral, DriveLoop, IoCapability, PasskeyProvider, TokioSleeper,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines, Stdin};
use tokio::sync::Mutex;

/// The advertised-name prefix the firmware uses (`Claude-XXXX` from the BT-MAC).
const NAME_PREFIX: &str = "Claude-";

/// Reads the 6-digit passkey the device shows on its glass from stdin, one pairing at a time.
///
/// stdin is serialized behind a mutex so a second pairing request cannot race the reader; the
/// prompt goes to stderr so it is visible even when stdout is redirected.
struct StdinPasskey {
    lines: Mutex<Lines<BufReader<Stdin>>>,
}

impl StdinPasskey {
    fn new() -> Self {
        StdinPasskey {
            lines: Mutex::new(BufReader::new(tokio::io::stdin()).lines()),
        }
    }
}

#[async_trait]
impl PasskeyProvider for StdinPasskey {
    async fn passkey(&self) -> Option<u32> {
        let mut stderr = tokio::io::stderr();
        let _ = stderr
            .write_all(b"Enter the 6-digit passkey shown on the stick: ")
            .await;
        let _ = stderr.flush().await;
        let mut lines = self.lines.lock().await;
        let line: String = lines.next_line().await.ok().flatten()?;
        line.trim().parse::<u32>().ok()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let session: Session = Session::new().await?;
    let adapter: Adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;

    // Wire the pairing agent with ONLY request_passkey → KeyboardOnly, and HOLD the handle for
    // the whole run (dropping it unregisters the agent → pairing fails "no agent").
    let provider: Arc<dyn PasskeyProvider> = Arc::new(StdinPasskey::new());
    let (agent, capability): (Agent, IoCapability) = build_agent(provider);
    let _agent_handle = session.register_agent(agent).await?;
    log::info!("pairing agent registered; IO capability = {capability} (MITM passkey entry)");

    log::info!("scanning for a '{NAME_PREFIX}' peripheral…");
    let address: Address = discover(&adapter, NAME_PREFIX).await?;
    log::info!("found {address}; connecting");

    let central: BluerCentral = BluerCentral::new(adapter, address)?;
    let mut driver: DriveLoop<BluerCentral, TokioSleeper> = DriveLoop::new(central, TokioSleeper);

    // Runs until a fatal condition (a missing agent). A healthy link reconnects forever.
    let reason: String = driver.run().await;
    log::error!("bridge stopped: {reason}");
    Err(reason.into())
}
