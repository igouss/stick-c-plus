//! The unix-socket **boundary**: turn each per-tool-call hook connection into a [`Daemon::ask`].
//!
//! The hook binary connects, writes one [`HookRequest`] JSON line, and blocks on one
//! [`DaemonReply`] JSON line back — holding its OWN deadline, so this side never needs a timeout of
//! its own. This module accepts those connections and bridges each to the actor. A connection that
//! sends nothing, closes early, or sends a malformed line is dropped (logged, never fatal): the
//! hook then reads EOF and degrades to the normal terminal prompt, the safe outcome.

use buddy_permission::{DaemonReply, HookRequest};
use log::{debug, warn};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::actor::Daemon;

/// Accept hook connections forever, bridging each to `daemon` on its own task. Returns only if the
/// listener itself fails irrecoverably.
pub async fn serve(daemon: Daemon, listener: UnixListener) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let daemon: Daemon = daemon.clone();
                tokio::spawn(handle(daemon, stream));
            }
            // A transient accept error is logged and retried; the loop is the daemon's front door.
            Err(err) => warn!("hook socket accept failed: {err}"),
        }
    }
}

/// Serve one hook connection: read its request line, ask the device, write the answer line.
///
/// Every early return leaves the hook reading EOF — the safe degrade to the terminal prompt. The
/// only line ever written back is a real [`DaemonReply`] the actor produced.
async fn handle(daemon: Daemon, stream: UnixStream) {
    let (read_half, mut write_half): (tokio::net::unix::OwnedReadHalf, _) = stream.into_split();
    let mut reader: BufReader<tokio::net::unix::OwnedReadHalf> = BufReader::new(read_half);
    let mut line: String = String::new();
    // EOF before any byte, or a read error: the hook closed early. Nothing to answer.
    match reader.read_line(&mut line).await {
        Ok(0) => return,
        Ok(_) => {}
        Err(err) => {
            debug!("hook connection read error: {err}");
            return;
        }
    }
    // A malformed request is dropped: the hook reads EOF and falls back to the terminal prompt.
    let req: HookRequest = match serde_json::from_str(line.trim()) {
        Ok(req) => req,
        Err(err) => {
            warn!("malformed hook request line: {err}");
            return;
        }
    };
    let reply: DaemonReply = daemon.ask(req).await;
    write_reply(&mut write_half, reply).await;
}

/// Write one [`DaemonReply`] as a single JSON line. A write failure means the hook already gave up
/// (its deadline passed) and closed — harmless, the loop is over anyway.
async fn write_reply(write_half: &mut tokio::net::unix::OwnedWriteHalf, reply: DaemonReply) {
    let mut encoded: String = match serde_json::to_string(&reply) {
        Ok(encoded) => encoded,
        Err(err) => {
            warn!("failed to encode daemon reply: {err}");
            return;
        }
    };
    encoded.push('\n');
    let _ = write_half.write_all(encoded.as_bytes()).await;
    let _ = write_half.flush().await;
}
