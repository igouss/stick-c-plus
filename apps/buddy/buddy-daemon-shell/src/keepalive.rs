//! The keepalive **tick**: nudge the actor on a fixed period so the glass stays warm.
//!
//! The Coordinator decides what a keepalive *means* — re-emit the current gated snapshot iff
//! bonded, and never clear a live prompt. This module only supplies the clock: an interval that
//! fires [`Daemon::keepalive`] forever. It is deliberately thin (like the concrete BLE adapter), so
//! the semantics stay in the host-tested Control, not here.

use std::time::Duration;

use crate::actor::Daemon;

/// Fire a keepalive on `daemon` every `period`, forever. The first tick fires immediately; each
/// tick is a fire-and-forget nudge the actor turns into a snapshot only while bonded.
pub async fn run(daemon: Daemon, period: Duration) {
    let mut ticker: tokio::time::Interval = tokio::time::interval(period);
    loop {
        ticker.tick().await;
        daemon.keepalive();
    }
}
