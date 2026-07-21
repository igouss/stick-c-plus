//! What one registered component last did, and when.

use platform_core::Tick;

use crate::Component;

/// One entry in the evidence `assess` folds over: what one registered component last did,
/// and when.
///
/// One struct, not a heartbeat joined against a separate concluded-report table, because it
/// answers one question — the spec's own sentence, "what every registered component last did
/// and when". Splitting it would let a report exist for a component that was never registered,
/// a failure mode this shape makes unrepresentable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Observed {
    /// Which component this reading is about.
    pub component: Component,
    /// The deadline this component's own loop period sets for itself.
    pub deadline_ms: u32,
    /// The last tick this component was heard from. `None` means registered but never once
    /// heard from — the zero case a stall can't otherwise name.
    pub last_beat: Option<Tick>,
    /// Set when the component concluded its own fault and said so, rather than the fold
    /// inferring a stall from silence.
    pub gave_up_at: Option<Tick>,
}
