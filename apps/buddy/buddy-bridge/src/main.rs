#![forbid(unsafe_code)]
//! `buddy-bridge` — the daemon that is the BLE central Claude Code does not ship.
//!
//! It registers the pairing agent (KeyboardOnly, holding its handle for the run), scans for the
//! `Claude-XXXX` stick, and runs the pure decision (`buddy-bridge-core`) against a live
//! [`BluerCentral`]. This is the composition root: the sole place bluer, stdin, the logger, and
//! the real sleeper are wired together, so every inner crate stays framework-free and tested.
//!
//! It now also runs the permission daemon: it spawns the coordinator actor, serves the hook unix
//! socket, ticks the keepalive, and wires a [`DaemonPeer`] behind the DriveLoop's `LinkPeer` so the
//! live BLE link carries snapshots down and device answers up. This is the one place the clock, the
//! socket path, and the owner label are read.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use bluer::agent::Agent;
use bluer::{Adapter, Address, Session};
use buddy_bridge_shell::{
    build_agent, discover, BluerCentral, DriveLoop, IoCapability, LinkPeer, PasskeyProvider,
    TokioSleeper,
};
use buddy_daemon_shell::{keepalive, socket, Daemon};
use buddy_permission::{DEFAULT_SOCK_FILENAME, SOCK_ENV};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines, Stdin};
use tokio::net::UnixListener;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Mutex;

/// The advertised-name prefix the firmware uses (`Claude-XXXX` from the BT-MAC).
const NAME_PREFIX: &str = "Claude-";

/// How often the daemon re-emits the current gated snapshot down the link, keeping the glass warm.
const KEEPALIVE_PERIOD: Duration = Duration::from_secs(5);

/// The env var overriding the owner label shown on the glass; absent → [`DEFAULT_OWNER`].
const OWNER_ENV: &str = "BUDDY_OWNER";

/// The env var overriding the tz offset (seconds east of UTC) for the device clock; absent → 0.
const TZ_OFFSET_ENV: &str = "BUDDY_TZ_OFFSET_S";

/// The owner label when [`OWNER_ENV`] is unset.
const DEFAULT_OWNER: &str = "Buddy";

/// The driving-adapter behind the DriveLoop's `LinkPeer`: it turns each link session into the
/// daemon's handshake and pumps lines both ways. This is where the impure clock is read — the pure
/// coordinator only ever receives an injected epoch/tz.
struct DaemonPeer {
    /// The handle onto the coordinator actor the whole daemon shares.
    daemon: Daemon,
    /// The owner label shown on the glass, read once at startup.
    owner: String,
    /// The tz offset (seconds east of UTC) stamped into the device time-sync, read once at startup.
    tz_offset_s: i32,
}

impl LinkPeer for DaemonPeer {
    fn on_up(&self) -> UnboundedReceiver<String> {
        // Read the wall clock at the impure edge; the coordinator stays clock-free.
        let epoch: i64 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since: Duration| since.as_secs() as i64)
            .unwrap_or(0);
        self.daemon.link_up(epoch, self.tz_offset_s, &self.owner)
    }
    fn on_line(&self, line: Vec<u8>) {
        self.daemon.device_line(line);
    }
    fn on_down(&self) {
        self.daemon.link_down();
    }
}

/// Resolve the daemon's unix-socket path from the shared contract: [`SOCK_ENV`] wins, else
/// `$XDG_RUNTIME_DIR/`[`DEFAULT_SOCK_FILENAME`], else the system temp dir — the same order the hook
/// binary resolves, so both agree on where the IPC lives.
fn socket_path() -> PathBuf {
    if let Ok(explicit) = std::env::var(SOCK_ENV) {
        return PathBuf::from(explicit);
    }
    let dir: PathBuf = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    dir.join(DEFAULT_SOCK_FILENAME)
}

/// The tz offset (seconds east of UTC) from [`TZ_OFFSET_ENV`], or 0 (UTC) when unset or unparseable.
fn tz_offset_s() -> i32 {
    std::env::var(TZ_OFFSET_ENV)
        .ok()
        .and_then(|raw: String| raw.parse::<i32>().ok())
        .unwrap_or(0)
}

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

    // Spawn the permission daemon: the coordinator actor, the hook unix socket, and the keepalive
    // tick. The actor owns the coordinator; the socket and keepalive drive it; the DaemonPeer below
    // bridges the live link. All share the one Daemon handle.
    let (daemon, _actor): (Daemon, tokio::task::JoinHandle<()>) = Daemon::spawn();
    let sock: PathBuf = socket_path();
    let _ = std::fs::remove_file(&sock); // a stale socket file would refuse the bind
    let listener: UnixListener = UnixListener::bind(&sock)?;
    log::info!("hook socket listening at {}", sock.display());
    tokio::spawn(socket::serve(daemon.clone(), listener));
    tokio::spawn(keepalive::run(daemon.clone(), KEEPALIVE_PERIOD));

    let owner: String = std::env::var(OWNER_ENV).unwrap_or_else(|_| DEFAULT_OWNER.to_string());
    let peer: DaemonPeer = DaemonPeer {
        daemon,
        owner,
        tz_offset_s: tz_offset_s(),
    };

    log::info!("scanning for a '{NAME_PREFIX}' peripheral…");
    let address: Address = discover(&adapter, NAME_PREFIX).await?;
    log::info!("found {address}; connecting");

    let central: BluerCentral = BluerCentral::new(adapter, address)?;
    let mut driver: DriveLoop<BluerCentral, TokioSleeper, DaemonPeer> =
        DriveLoop::new(central, TokioSleeper, peer);

    // Runs until a fatal condition (a missing agent). A healthy link reconnects forever.
    let reason: String = driver.run().await;
    log::error!("bridge stopped: {reason}");
    Err(reason.into())
}
