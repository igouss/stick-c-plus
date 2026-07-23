//! The injected sleep — so the loop's timing is a port, and tests need no real clock.
//!
//! [`Action::Backoff`](buddy_bridge_core::Action::Backoff) carries a `Duration`; the loop hands
//! it to a [`Sleeper`] rather than calling `tokio::time::sleep` directly, so a test drives the
//! whole reconnect schedule instantly with a [`NoSleep`], and only the daemon links the real
//! [`TokioSleeper`].

use std::time::Duration;

use async_trait::async_trait;

/// Something that can wait a `Duration` asynchronously.
#[async_trait]
pub trait Sleeper: Send {
    /// Sleep for `dur`.
    async fn sleep(&self, dur: Duration);
}

/// The real sleep, for the daemon.
#[derive(Clone, Copy, Debug, Default)]
pub struct TokioSleeper;

#[async_trait]
impl Sleeper for TokioSleeper {
    async fn sleep(&self, dur: Duration) {
        tokio::time::sleep(dur).await;
    }
}

/// A sleep that returns immediately, for tests: the backoff schedule runs with no wall-clock
/// cost, so a reconnect sequence is exercised in a single fast test.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSleep;

#[async_trait]
impl Sleeper for NoSleep {
    async fn sleep(&self, _dur: Duration) {}
}
