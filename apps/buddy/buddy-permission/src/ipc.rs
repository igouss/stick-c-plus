//! The hook ↔ daemon IPC DTOs — ours to define, serde-serialized over the daemon's unix socket.
//!
//! A per-tool-call hook process cannot own a bonded BLE connection, so it forwards the tool-call
//! context to the long-lived daemon and blocks on the reply. These are the two messages that cross
//! that socket. The *transport* (the socket, the framing, the deadline) belongs to the daemon and
//! the hook binary; this module only fixes the message shapes and their round-trip.
//!
//! The reply maps back to a [`hook::AskOutcome`](crate::hook::AskOutcome). Note which outcomes are
//! *not* here: [`TimedOut`](crate::hook::AskOutcome::TimedOut) and
//! [`DaemonDown`](crate::hook::AskOutcome::DaemonDown) are the hook's OWN local conditions — no
//! reply arrived, or the socket refused — so the daemon never sends them. Only the answers the
//! daemon can actually give ([`Allow`](DaemonReply::Allow) / [`Deny`](DaemonReply::Deny) /
//! [`Unbonded`](DaemonReply::Unbonded)) are wire values.

use crate::hook::AskOutcome;
use buddy_wire::Decision;
use serde::{Deserialize, Serialize};

/// The tool-call context the hook forwards to the daemon, from the (externally parsed) PreToolUse
/// payload.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct HookRequest {
    /// The Claude Code session id (keys the ledger).
    pub session_id: String,
    /// The tool being invoked (shown on the glass).
    pub tool_name: String,
    /// The working directory the tool call runs in.
    pub cwd: String,
    /// The permission mode in effect (`default` / `acceptEdits` / `plan` / …).
    pub permission_mode: String,
    /// The transcript path — the raw material for token accounting.
    pub transcript_path: String,
}

/// The daemon's reply to a [`HookRequest`] — the answers the daemon can actually give.
///
/// Maps to an [`AskOutcome`] via [`to_outcome`](DaemonReply::to_outcome). The hook-local outcomes
/// (timeout, daemon-down) are deliberately absent — they are not things the daemon says.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DaemonReply {
    /// The owner approved on the stick.
    Allow,
    /// The owner denied on the stick.
    Deny,
    /// The daemon is up but nothing is bonded, so the device could not be asked.
    Unbonded,
}

impl DaemonReply {
    /// Map this reply to the [`AskOutcome`] the fail-safe [`decide`](crate::hook::decide) consumes.
    ///
    /// [`Allow`](DaemonReply::Allow) → `Decided(Decision::Once)`,
    /// [`Deny`](DaemonReply::Deny) → `Decided(Decision::Deny)`,
    /// [`Unbonded`](DaemonReply::Unbonded) → `Unbonded`.
    pub fn to_outcome(self) -> AskOutcome {
        match self {
            DaemonReply::Allow => AskOutcome::Decided(Decision::Once),
            DaemonReply::Deny => AskOutcome::Decided(Decision::Deny),
            DaemonReply::Unbonded => AskOutcome::Unbonded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::{decide, HookOutput};

    /// An `Allow` reply carries a device approval into an `Once` decision.
    #[test]
    fn an_allow_reply_maps_to_a_once_decision() {
        assert_eq!(
            DaemonReply::Allow.to_outcome(),
            AskOutcome::Decided(Decision::Once)
        );
    }

    /// A `Deny` reply carries a device denial into a `Deny` decision.
    #[test]
    fn a_deny_reply_maps_to_a_deny_decision() {
        assert_eq!(
            DaemonReply::Deny.to_outcome(),
            AskOutcome::Decided(Decision::Deny)
        );
    }

    /// An `Unbonded` reply carries the fail-safe: it maps to the non-decided `Unbonded` outcome, so
    /// [`decide`] stays [`HookOutput::Silent`]. A mis-wiring to a `Decided` variant here would let
    /// an unbonded device silently emit — this pins the whole `Unbonded -> Silent` edge.
    #[test]
    fn an_unbonded_reply_stays_the_silent_fail_safe() {
        assert_eq!(DaemonReply::Unbonded.to_outcome(), AskOutcome::Unbonded);
        assert_eq!(
            decide(DaemonReply::Unbonded.to_outcome()),
            HookOutput::Silent
        );
    }
}
