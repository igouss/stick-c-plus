//! The pairing agent — wired so the secure IO capability and the passkey handler are one act.
//!
//! bluer infers the IO capability from which handlers are set: only `request_passkey` yields
//! KeyboardOnly (Passkey Entry against the device's DisplayOnly); *no* handlers yields
//! NoInputNoOutput — a silent Just-Works downgrade with no MITM protection. [`build_agent`] is
//! the sole place the agent is built: it sets exactly `request_passkey` and, in the same
//! expression, mints the [`IoCapability::KeyboardOnly`] proof token the composition root logs.
//! Because the token has no insecure variant, "we wired the passkey handler" and "our posture
//! is KeyboardOnly" cannot drift apart.

use std::sync::Arc;

use async_trait::async_trait;
use bluer::agent::{Agent, ReqError, RequestPasskey};
use buddy_bridge_core::IoCapability;

/// Supplies the 6-digit passkey the device shows on its glass. The composition root implements
/// this over stdin; tests supply a canned value (and observe that it was asked for).
#[async_trait]
pub trait PasskeyProvider: Send + Sync {
    /// The passkey to enter, or `None` to reject the pairing.
    async fn passkey(&self) -> Option<u32>;
}

/// Build the pairing agent with ONLY `request_passkey` set, returning it alongside the
/// [`IoCapability`] it therefore pairs as. Register the returned [`Agent`] with
/// `Session::register_agent` and **hold the resulting `AgentHandle`** for the daemon's life —
/// dropping it unregisters the agent and pairing then fails with "no agent".
pub fn build_agent(provider: Arc<dyn PasskeyProvider>) -> (Agent, IoCapability) {
    let agent: Agent = Agent {
        request_passkey: Some(Box::new(move |_request: RequestPasskey| {
            let provider: Arc<dyn PasskeyProvider> = Arc::clone(&provider);
            Box::pin(async move {
                match provider.passkey().await {
                    Some(passkey) => Ok(passkey),
                    None => Err(ReqError::Canceled),
                }
            })
        })),
        ..Default::default()
    };
    (agent, IoCapability::KeyboardOnly)
}
