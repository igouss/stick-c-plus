//! The blocking accept loop and per-connection pump — the imperative shell around
//! the pure [`esphome_api::handle`] FSM.
//!
//! [`Server::bind`] claims the listener; [`Server::serve`] runs the accept loop,
//! spawning one sized thread per connection that pumps frames through the FSM
//! against a shared [`Device`]. The design targets a 520 KB ESP32 with no PSRAM,
//! so it is deliberately small: a handful of concurrent connections, an explicit
//! per-connection stack, a read timeout that both reaps a wedged client and paces
//! state broadcasts, and no allocation-per-byte buffering beyond the codec's own.
//!
//! ## What the loop guarantees
//!   - **Bounded concurrency.** At most [`ServerConfig::max_connections`] threads
//!     serve at once; the next connection is closed immediately and existing ones
//!     keep serving. A finished connection's slot is reclaimed, so a later
//!     connection succeeds.
//!   - **No wedged client bricks the API.** A connection idle (or stalled
//!     mid-frame) for [`ServerConfig::idle_timeout`] is dropped, freeing its slot.
//!     The timeout exceeds Home Assistant's keepalive ping interval, so a healthy
//!     subscribed client — which pings periodically — is never mistaken for dead.
//!   - **Device-driven updates.** Between reads, the pump polls the `Device` and
//!     pushes any changed state to a subscribed client, so a new sensor reading
//!     reaches HA without the client asking.
//!   - **Clean shutdown.** [`ServerHandle::shutdown`] stops the accept loop and
//!     lets in-flight connections drain, so a test (or a device teardown) leaks
//!     neither a socket nor a thread.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use esphome_api::{
    broadcast_states, handle, Connection, FrameReader, FrameWriter, Inbound, Outbound, Snapshot,
    StateMessage,
};

use crate::device::Device;

/// The ESPHome native-API TCP port every device listens on and HA connects to.
pub const NATIVE_API_PORT: u16 = 6053;

/// The per-connection thread stack budget for a **plaintext** connection on the
/// ESP32: the FSM is shallow (decode one small prost message, fold it through
/// `handle`, encode a few replies) and the codec's largest single frame is a
/// ~400-byte `DeviceInfoResponse`, so ~12 KB holds the deepest call with headroom.
///
/// This is the on-target figure; Noise (qhw.10) adds a cipher state and its own
/// buffers, so the budget MUST be re-measured when it lands. The host verifies the
/// code runs within this stack (`serves_within_the_plaintext_stack_budget`), a
/// proxy for the device — the true on-device headroom is a heap/stack measurement
/// on the board.
pub const PLAINTEXT_STACK_SIZE: usize = 12 * 1024;

/// How the [`Server`] runs: where it binds, how many connections it serves at
/// once, and the two timeouts that pace a connection.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// The address to bind. `0.0.0.0:6053` in production (see
    /// [`ServerConfig::default`]); `127.0.0.1:0` in a test to grab an ephemeral
    /// port.
    pub bind_addr: SocketAddr,
    /// The maximum number of connections served concurrently. HA opens one; a
    /// small cap (1–2, no PSRAM) covers a reconnect racing the old connection's
    /// teardown while bounding RAM. The `cap + 1`th connection is closed at once.
    pub max_connections: usize,
    /// How long a blocking read waits before the pump wakes to poll for state
    /// changes and to age the idle timer. Also the granularity of
    /// [`Self::idle_timeout`]. Must be non-zero.
    pub read_timeout: Duration,
    /// How long a connection may receive nothing before it is reaped. Must exceed
    /// HA's keepalive ping interval (~20 s) so a healthy idle client is not
    /// dropped; a wedged or stalled client is.
    pub idle_timeout: Duration,
    /// The stack size of each per-connection thread. Defaults to
    /// [`PLAINTEXT_STACK_SIZE`]; sized explicitly because the ESP32 has no room
    /// for the host's multi-megabyte default per connection.
    pub stack_size: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::from((Ipv4Addr::UNSPECIFIED, NATIVE_API_PORT)),
            max_connections: 2,
            read_timeout: Duration::from_secs(1),
            idle_timeout: Duration::from_secs(90),
            stack_size: PLAINTEXT_STACK_SIZE,
        }
    }
}

/// A bound native-API server, ready to [`serve`](Self::serve).
///
/// [`bind`](Self::bind) owns the listener so a caller can read the chosen port
/// ([`local_addr`](Self::local_addr)) and take a [`handle`](Self::handle) for
/// shutdown *before* [`serve`](Self::serve) consumes `self` into the blocking
/// accept loop.
pub struct Server {
    listener: TcpListener,
    local_addr: SocketAddr,
    config: ServerConfig,
    /// The number of connections currently being served — read to enforce the cap,
    /// decremented by each connection thread's [`SlotGuard`] as it exits.
    active: Arc<AtomicUsize>,
    /// Cleared by [`ServerHandle::shutdown`] to stop the accept loop and signal
    /// in-flight connections to drain.
    running: Arc<AtomicBool>,
}

/// A shutdown handle for a running [`Server`]. Cheap to clone; every clone shares
/// the same stop signal and connection counter.
#[derive(Clone)]
pub struct ServerHandle {
    running: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
    local_addr: SocketAddr,
}

impl ServerHandle {
    /// Stop the accept loop and drain in-flight connections.
    ///
    /// Clears the running flag, then opens a throwaway loopback connection to
    /// unblock the accept that is parked in `serve`, so it observes the flag and
    /// returns instead of waiting for one more real client. In-flight connections
    /// notice the flag on their next read tick (within [`ServerConfig::read_timeout`])
    /// and exit; `serve` joins them before returning.
    pub fn shutdown(&self) {
        self.running.store(false, Ordering::SeqCst);
        // Best effort: if the loop already exited, the connect simply fails.
        let _ = TcpStream::connect(self.local_addr);
    }

    /// The number of connections being served right now — a device load metric,
    /// and the leak check a test asserts returns to zero.
    pub fn active_connections(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }
}

impl Server {
    /// Bind the listener at [`ServerConfig::bind_addr`]. Does not accept yet.
    pub fn bind(config: ServerConfig) -> io::Result<Self> {
        let listener: TcpListener = TcpListener::bind(config.bind_addr)?;
        let local_addr: SocketAddr = listener.local_addr()?;
        Ok(Self {
            listener,
            local_addr,
            config,
            active: Arc::new(AtomicUsize::new(0)),
            running: Arc::new(AtomicBool::new(true)),
        })
    }

    /// The address actually bound — the resolved port when the config asked for
    /// port 0.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// A [`ServerHandle`] to shut this server down, or read its load, from another
    /// thread.
    pub fn handle(&self) -> ServerHandle {
        ServerHandle {
            running: Arc::clone(&self.running),
            active: Arc::clone(&self.active),
            local_addr: self.local_addr,
        }
    }

    /// Run the accept loop until [`ServerHandle::shutdown`], serving `device` on
    /// every connection. Blocks; a device runs this on a dedicated thread.
    ///
    /// Each accepted connection either takes a slot and a sized thread, or — if the
    /// cap is reached — is closed at once. Threads that have finished are joined as
    /// new connections arrive, so their handles do not accumulate; the rest are
    /// joined on shutdown.
    pub fn serve<D: Device + 'static>(self, device: Arc<D>) -> io::Result<()> {
        let mut connections: Vec<JoinHandle<()>> = Vec::new();

        loop {
            let (stream, _peer): (TcpStream, SocketAddr) = match self.listener.accept() {
                Ok(accepted) => accepted,
                // A transient accept error (e.g. the peer reset before accept
                // returned) must not kill the server — log and keep accepting.
                Err(err) => {
                    log::warn!("accept failed: {err}");
                    continue;
                }
            };

            if !self.running.load(Ordering::SeqCst) {
                // Woken by `shutdown`'s loopback connect (or a real client racing
                // it); either way the server is stopping.
                drop(stream);
                break;
            }

            reap_finished(&mut connections);

            if self.active.load(Ordering::SeqCst) >= self.config.max_connections {
                // The cap+1th connection: close it immediately, keep serving the
                // rest. A single accept loop reads `active`, so there is no check
                // racing another accept; a concurrent decrement only frees a slot.
                log::debug!(
                    "connection cap {} reached — rejecting {_peer}",
                    self.config.max_connections
                );
                drop(stream);
                continue;
            }

            self.active.fetch_add(1, Ordering::SeqCst);
            let device: Arc<D> = Arc::clone(&device);
            let active: Arc<AtomicUsize> = Arc::clone(&self.active);
            let running: Arc<AtomicBool> = Arc::clone(&self.running);
            let config: ServerConfig = self.config.clone();

            match thread::Builder::new()
                .name("esphome-conn".to_string())
                .stack_size(config.stack_size)
                .spawn(move || serve_connection(stream, device, config, active, running))
            {
                Ok(handle) => connections.push(handle),
                Err(err) => {
                    // The stream was moved into the closure and is closed with it;
                    // just reclaim the slot the spawn never used.
                    log::error!("failed to spawn connection thread: {err}");
                    self.active.fetch_sub(1, Ordering::SeqCst);
                }
            }
        }

        // Drain: in-flight connections observe `running == false` within one read
        // tick and return; join them so no socket or thread outlives the server.
        for handle in connections {
            let _ = handle.join();
        }
        Ok(())
    }
}

/// Join and drop every connection thread that has already finished, so the handle
/// vector tracks only live connections instead of growing with every past one.
fn reap_finished(connections: &mut Vec<JoinHandle<()>>) {
    let mut i: usize = 0;
    while i < connections.len() {
        if connections[i].is_finished() {
            // `join` is immediate on a finished thread; a panicked connection
            // surfaces as `Err` and is ignored — one bad connection is not fatal.
            let _ = connections.swap_remove(i).join();
        } else {
            i += 1;
        }
    }
}

/// Decrements the active-connection counter when a connection thread exits, by any
/// path (clean close, reap, protocol fault, or panic), so a slot is never leaked.
struct SlotGuard(Arc<AtomicUsize>);

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Serve one connection to completion, then release its slot.
fn serve_connection<D: Device>(
    stream: TcpStream,
    device: Arc<D>,
    config: ServerConfig,
    active: Arc<AtomicUsize>,
    running: Arc<AtomicBool>,
) {
    let _slot: SlotGuard = SlotGuard(active);
    if let Err(err) = pump(stream, device.as_ref(), &config, &running) {
        // An I/O or protocol fault ends this connection only; the server serves on.
        log::debug!("connection ended: {err}");
    }
}

/// The read/handle/write loop for one connection.
///
/// Blocks on a frame with a read timeout. A frame is decoded and folded through
/// [`handle`]; the replies are written back. A timeout is a tick: it ages the idle
/// timer (reaping a wedged client past [`ServerConfig::idle_timeout`]) and polls
/// the device for a changed state to push to a subscribed client. A clean close or
/// a closing handshake returns `Ok`; a protocol or I/O fault returns `Err` and the
/// caller drops the connection.
fn pump<D: Device>(
    stream: TcpStream,
    device: &D,
    config: &ServerConfig,
    running: &AtomicBool,
) -> io::Result<()> {
    stream.set_read_timeout(Some(config.read_timeout))?;
    // A write timeout too, so a subscribed client that stops draining cannot wedge
    // this thread in a blocked `write` (TCP backpressure) — the write fails, the
    // connection is dropped, and shutdown can still join it. A half-written frame on
    // the way out is moot: the socket is closing.
    stream.set_write_timeout(Some(config.idle_timeout))?;
    // Small frames, latency over throughput: send each reply without Nagle delay.
    let _ = stream.set_nodelay(true);

    let mut reader: FrameReader<TcpStream> = FrameReader::new(stream.try_clone()?);
    let mut writer: FrameWriter<TcpStream> = FrameWriter::new(stream);

    // Identity and entity list are fixed for the life of the device; fetch once.
    let device_info = device.device_info();
    let list = device.list_messages();

    let mut conn: Connection = Connection::new();
    // The states last pushed to the client, so a poll tick broadcasts only genuine
    // changes. Seeded when the client subscribes (the initial stream is `handle`'s
    // job), so the first tick does not re-send what was just streamed.
    let mut last_broadcast: Vec<StateMessage> = Vec::new();
    let mut idle: Duration = Duration::ZERO;

    loop {
        if !running.load(Ordering::SeqCst) {
            return Ok(()); // graceful shutdown
        }

        match reader.read_frame() {
            Ok(Some(frame)) => {
                idle = Duration::ZERO;
                let inbound: Inbound = Inbound::from_frame(&frame)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

                let states: Vec<StateMessage> = device.state_messages();
                let (next, outbound): (Connection, Vec<Outbound>) = {
                    let snapshot: Snapshot = Snapshot {
                        device_info: &device_info,
                        list: &list,
                        states: &states,
                    };
                    handle(conn, &inbound, &snapshot)
                };
                conn = next;
                write_all(&mut writer, &outbound)?;

                if matches!(inbound, Inbound::SubscribeStates(_)) {
                    last_broadcast = states;
                }
                if conn.is_closed() {
                    return Ok(());
                }
            }

            // A clean end of stream on a frame boundary: the client left.
            Ok(None) => return Ok(()),

            // A read timeout: not an error, a tick.
            Err(ref err) if is_timeout(err) => {
                idle = idle.saturating_add(config.read_timeout);
                if idle >= config.idle_timeout {
                    log::debug!("reaping connection idle for {idle:?}");
                    return Ok(());
                }
                broadcast_changes(&conn, device, &mut writer, &mut last_broadcast)?;
            }

            // A truncated frame (UnexpectedEof) or a non-frame on the wire
            // (InvalidData): a protocol fault. Drop the connection.
            Err(err) => return Err(err),
        }
    }
}

/// Push any changed state to a subscribed connection, updating the high-water mark.
///
/// A no-op unless the client has subscribed and the device's current states differ
/// from what was last sent — so an unsubscribed or unchanged tick sends nothing.
fn broadcast_changes<D: Device>(
    conn: &Connection,
    device: &D,
    writer: &mut FrameWriter<TcpStream>,
    last_broadcast: &mut Vec<StateMessage>,
) -> io::Result<()> {
    if !conn.is_subscribed() {
        return Ok(());
    }
    let states: Vec<StateMessage> = device.state_messages();
    if states == *last_broadcast {
        return Ok(());
    }
    let updates: Vec<Outbound> = broadcast_states(conn, &states);
    write_all(writer, &updates)?;
    *last_broadcast = states;
    Ok(())
}

/// Write each outbound message as a frame, in order.
fn write_all(writer: &mut FrameWriter<TcpStream>, messages: &[Outbound]) -> io::Result<()> {
    for message in messages {
        let (msg_type, payload): (u32, Vec<u8>) = message.wire();
        writer.write_frame(msg_type, &payload)?;
    }
    Ok(())
}

/// Whether an I/O error is a read timeout (the tick signal) rather than a fault.
///
/// `set_read_timeout` surfaces as `WouldBlock` on some platforms and `TimedOut` on
/// others, so both are the tick.
fn is_timeout(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_config_binds_the_native_api_port_on_all_interfaces() {
        let config: ServerConfig = ServerConfig::default();
        assert_eq!(config.bind_addr.port(), NATIVE_API_PORT);
        assert!(
            config.bind_addr.ip().is_unspecified(),
            "0.0.0.0 for the device"
        );
        assert_eq!(config.stack_size, PLAINTEXT_STACK_SIZE);
    }

    #[test]
    fn the_idle_timeout_exceeds_has_keepalive_interval() {
        // HA pings roughly every 20 s; the reap window must be longer or a healthy
        // idle client would be dropped between pings.
        assert!(ServerConfig::default().idle_timeout > Duration::from_secs(20));
    }

    #[test]
    fn a_timeout_is_a_tick_and_other_errors_are_faults() {
        assert!(is_timeout(&io::Error::from(io::ErrorKind::WouldBlock)));
        assert!(is_timeout(&io::Error::from(io::ErrorKind::TimedOut)));
        assert!(!is_timeout(&io::Error::from(io::ErrorKind::UnexpectedEof)));
        assert!(!is_timeout(&io::Error::from(io::ErrorKind::InvalidData)));
    }

    #[test]
    fn the_slot_guard_releases_its_slot_on_drop() {
        let active: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(1));
        {
            let _guard: SlotGuard = SlotGuard(Arc::clone(&active));
            assert_eq!(active.load(Ordering::SeqCst), 1);
        }
        assert_eq!(active.load(Ordering::SeqCst), 0, "drop reclaims the slot");
    }
}
