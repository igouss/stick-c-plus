//! The emission gating rule — the device-side anti-hazard, made unrepresentable.
//!
//! An absent `prompt` field in a snapshot legitimately means "the prompt is gone", so a bare,
//! prompt-less snapshot emitted as a keepalive WHILE a prompt is outstanding would clear a live
//! approval off the glass — the exact device-side fail-open the domain crate closed and this crate
//! must not reintroduce from the bridge.
//!
//! [`GatedSnapshot`] is the guard: a synthesized [`SnapshotPacket`] can only reach the daemon's
//! serializer through [`GatedSnapshot::gate`], which re-attaches the outstanding prompt whenever one
//! is live. Clearing the prompt stays a DELIBERATE act — the ledger drops the outstanding prompt on
//! an explicit [`Resolved`](crate::ledger::SessionEvent::Resolved), so `gate` is passed
//! `outstanding == None` only when the clear is intended.

use buddy_wire::{Prompt, SnapshotPacket};

/// A snapshot that has passed the emission gate.
///
/// The invariant it carries: if a prompt was outstanding at gate time, the packet's `prompt` is
/// `Some`, so emitting it can never silently clear a live approval on the device. The daemon
/// serializes only through [`packet`](GatedSnapshot::packet) / [`into_packet`](GatedSnapshot::into_packet),
/// so it cannot hand a bare snapshot to the link while a prompt is live.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GatedSnapshot(SnapshotPacket);

impl GatedSnapshot {
    /// Gate a synthesized snapshot against the live outstanding prompt.
    ///
    /// When `outstanding` is `Some`, the returned packet carries that prompt (restoring it if the
    /// synthesized packet omitted it). When `outstanding` is `None`, the packet is passed through
    /// as-is — an absent prompt is then the DELIBERATE clear the device acts on.
    pub fn gate(packet: SnapshotPacket, outstanding: Option<Prompt>) -> Self {
        match outstanding {
            // A live prompt must ride the snapshot: restore it, overwriting whatever the bare
            // packet carried, so emitting it can never clear the approval on the device.
            Some(prompt) => GatedSnapshot(SnapshotPacket {
                prompt: Some(prompt),
                ..packet
            }),
            // No live prompt: pass the packet through as-is — an absent prompt is then the
            // DELIBERATE clear the device acts on.
            None => GatedSnapshot(packet),
        }
    }

    /// The gated packet, ready for [`buddy_wire::serialize_snapshot`].
    pub fn packet(&self) -> &SnapshotPacket {
        &self.0
    }

    /// Consume the gate, yielding the packet for serialization.
    pub fn into_packet(self) -> SnapshotPacket {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bare (prompt-less) keepalive snapshot.
    fn a_bare_snapshot() -> SnapshotPacket {
        SnapshotPacket {
            running: Some(1),
            ..SnapshotPacket::default()
        }
    }

    /// A prompt fixture.
    fn a_prompt(id: &str) -> Prompt {
        Prompt {
            id: id.to_string(),
            tool: "bash".to_string(),
            hint: "rm -rf".to_string(),
        }
    }

    /// Gating a bare snapshot against an outstanding prompt restores the prompt.
    #[test]
    fn gating_restores_the_outstanding_prompt() {
        let gated: GatedSnapshot = GatedSnapshot::gate(a_bare_snapshot(), Some(a_prompt("req_9")));
        assert_eq!(gated.packet().prompt, Some(a_prompt("req_9")));
    }

    /// Gating with no outstanding prompt passes the bare snapshot through — a deliberate clear.
    #[test]
    fn gating_without_a_prompt_passes_through() {
        let gated: GatedSnapshot = GatedSnapshot::gate(a_bare_snapshot(), None);
        assert!(gated.packet().prompt.is_none());
    }

    /// Gating preserves the other fields untouched.
    #[test]
    fn gating_preserves_the_other_fields() {
        let gated: GatedSnapshot = GatedSnapshot::gate(a_bare_snapshot(), Some(a_prompt("req_9")));
        assert_eq!(gated.into_packet().running, Some(1));
    }
}
