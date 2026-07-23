//! THE fail-safe decision — the load-bearing guard against the HOST fail-open.
//!
//! A pure function from *the outcome of asking the device* to *what the PreToolUse hook prints*.
//! Claude Code proceeds on a hook timeout, so the ONLY safe emit is a real device decision; every
//! other outcome must print nothing and hand control to Claude Code's normal terminal prompt.
//!
//! The PreToolUse output contract (from the bead):
//!
//! ```json
//! {"hookSpecificOutput":{"hookEventName":"PreToolUse",
//!  "permissionDecision":"allow"|"deny",
//!  "permissionDecisionReason":"<reason>"}}
//! ```
//!
//! `permissionDecision` is ONLY ever `allow` or `deny` here — never `"ask"` (upstream #39344 can
//! silently disable `permissions.deny` rules) and never `"defer"`. The fail-safe is to emit an
//! EMPTY string, which hands control to Claude Code's normal prompt.

use buddy_wire::Decision;
use serde::Serialize;

/// The canonical reason for an allow emitted from a device approval.
pub const ALLOW_REASON: &str = "approved on the stick";
/// The canonical reason for a deny emitted from a device denial.
pub const DENY_REASON: &str = "denied on the stick";

/// The daemon's answer to "did the owner decide?", or *why there is none*.
///
/// The failure modes are named explicitly so the [`decide`] mapping is exhaustive: only
/// [`Decided`](AskOutcome::Decided) can ever produce an emit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AskOutcome {
    /// The owner pressed a button and the device answered with a [`Decision`].
    Decided(Decision),
    /// The hook's own deadline (shorter than Claude Code's 600 s) expired with no answer.
    TimedOut,
    /// The daemon was not reachable (not running / socket refused).
    DaemonDown,
    /// The daemon is up but nothing is bonded, so the device cannot be asked.
    Unbonded,
}

/// The allow/deny axis of a PreToolUse decision — never `ask`, never `defer`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Permission {
    /// `permissionDecision:"allow"`.
    Allow,
    /// `permissionDecision:"deny"`.
    Deny,
}

impl Permission {
    /// The wire token for this permission (`"allow"` or `"deny"`).
    pub const fn as_wire(self) -> &'static str {
        match self {
            Permission::Allow => "allow",
            Permission::Deny => "deny",
        }
    }
}

/// A concrete PreToolUse decision to print: the allow/deny axis and its human reason.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PreToolUseDecision {
    /// Allow or deny — the only two values this crate ever emits.
    pub permission: Permission,
    /// The `permissionDecisionReason` string.
    pub reason: String,
}

/// What the hook prints: an explicit decision, or nothing at all.
///
/// [`Silent`](HookOutput::Silent) means print an EMPTY string so Claude Code's normal permission
/// prompt takes over — the safe degrade. [`Emit`](HookOutput::Emit) carries an allow or a deny.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HookOutput {
    /// Print the PreToolUse JSON for this allow/deny decision.
    Emit(PreToolUseDecision),
    /// Print NOTHING; hand control to Claude Code's normal terminal prompt.
    Silent,
}

/// Map the outcome of asking the device to what the hook prints — the fail-safe guard.
///
/// The whole contract, exhaustively:
/// - [`AskOutcome::Decided(Decision::Once)`] → [`HookOutput::Emit`] allow;
/// - [`AskOutcome::Decided(Decision::Deny)`] → [`HookOutput::Emit`] deny;
/// - [`AskOutcome::TimedOut`] / [`DaemonDown`](AskOutcome::DaemonDown) /
///   [`Unbonded`](AskOutcome::Unbonded) → [`HookOutput::Silent`].
///
/// There is NO path from a non-decided outcome to an emit, and no `ask` / `defer` anywhere.
pub fn decide(outcome: AskOutcome) -> HookOutput {
    match outcome {
        // The ONLY two paths to an emit: a real device decision.
        AskOutcome::Decided(Decision::Once) => HookOutput::Emit(PreToolUseDecision {
            permission: Permission::Allow,
            reason: ALLOW_REASON.to_string(),
        }),
        AskOutcome::Decided(Decision::Deny) => HookOutput::Emit(PreToolUseDecision {
            permission: Permission::Deny,
            reason: DENY_REASON.to_string(),
        }),
        // The fail-safe: every non-decided outcome prints nothing, handing control to Claude
        // Code's normal terminal prompt. There is NO path from here to an emit.
        AskOutcome::TimedOut | AskOutcome::DaemonDown | AskOutcome::Unbonded => HookOutput::Silent,
    }
}

/// The `hookSpecificOutput` envelope — the exact PreToolUse JSON shape, serde-rendered so the
/// reason string is faithfully escaped.
#[derive(Serialize)]
struct PreToolUseWire<'a> {
    #[serde(rename = "hookSpecificOutput")]
    hook_specific_output: HookSpecificOutput<'a>,
}

/// The inner object: the fixed event name, the allow/deny decision, and the reason.
#[derive(Serialize)]
struct HookSpecificOutput<'a> {
    #[serde(rename = "hookEventName")]
    hook_event_name: &'static str,
    #[serde(rename = "permissionDecision")]
    permission_decision: &'static str,
    #[serde(rename = "permissionDecisionReason")]
    permission_decision_reason: &'a str,
}

/// Render a [`HookOutput`] to exactly what the hook writes to stdout.
///
/// [`HookOutput::Silent`] → `""` (empty). [`HookOutput::Emit`] → the exact PreToolUse JSON with
/// `permissionDecision` of `"allow"` or `"deny"` and the carried reason. It never emits `"ask"`.
pub fn to_stdout(output: &HookOutput) -> String {
    match output {
        // The safe degrade: an empty string, never "{}" and never "null".
        HookOutput::Silent => String::new(),
        HookOutput::Emit(decision) => {
            let wire: PreToolUseWire<'_> = PreToolUseWire {
                hook_specific_output: HookSpecificOutput {
                    hook_event_name: "PreToolUse",
                    permission_decision: decision.permission.as_wire(),
                    permission_decision_reason: &decision.reason,
                },
            };
            serde_json::to_string(&wire).expect("a PreToolUse decision always serializes")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A device approval maps to an allow emit with the canonical reason.
    #[test]
    fn an_approval_emits_allow() {
        assert_eq!(
            decide(AskOutcome::Decided(Decision::Once)),
            HookOutput::Emit(PreToolUseDecision {
                permission: Permission::Allow,
                reason: ALLOW_REASON.to_string(),
            })
        );
    }

    /// A device denial maps to a deny emit with the canonical reason.
    #[test]
    fn a_denial_emits_deny() {
        assert_eq!(
            decide(AskOutcome::Decided(Decision::Deny)),
            HookOutput::Emit(PreToolUseDecision {
                permission: Permission::Deny,
                reason: DENY_REASON.to_string(),
            })
        );
    }

    /// A timeout is the safe degrade: print nothing.
    #[test]
    fn a_timeout_is_silent() {
        assert_eq!(decide(AskOutcome::TimedOut), HookOutput::Silent);
    }

    /// Silent renders to the empty string, not "{}" and not "null".
    #[test]
    fn silent_renders_empty() {
        assert_eq!(to_stdout(&HookOutput::Silent), "");
    }

    /// The allow emit is exactly the frozen PreToolUse shape.
    #[test]
    fn allow_renders_the_exact_shape() {
        let line: String = to_stdout(&decide(AskOutcome::Decided(Decision::Once)));
        assert_eq!(
            line,
            r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","permissionDecisionReason":"approved on the stick"}}"#
        );
    }

    /// The deny emit is exactly the frozen PreToolUse shape.
    #[test]
    fn deny_renders_the_exact_shape() {
        let line: String = to_stdout(&decide(AskOutcome::Decided(Decision::Deny)));
        assert_eq!(
            line,
            r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"denied on the stick"}}"#
        );
    }

    /// The permission wire tokens are exactly allow/deny — never ask, never defer.
    #[test]
    fn permission_wire_tokens_are_allow_and_deny() {
        assert_eq!(Permission::Allow.as_wire(), "allow");
        assert_eq!(Permission::Deny.as_wire(), "deny");
    }

    proptest! {
        /// Totality of the fail-open guard: EVERY non-decided outcome is Silent.
        #[test]
        fn every_non_decided_outcome_is_silent(kind in 0u8..3) {
            let outcome: AskOutcome = match kind {
                0 => AskOutcome::TimedOut,
                1 => AskOutcome::DaemonDown,
                _ => AskOutcome::Unbonded,
            };
            prop_assert_eq!(decide(outcome), HookOutput::Silent);
        }

        /// Over the whole emit space, the rendered permissionDecision is only allow or deny — the
        /// structured field never becomes ask or defer, whatever the reason carries.
        #[test]
        fn the_decision_field_is_only_allow_or_deny(deny in any::<bool>(), reason in ".*") {
            let permission: Permission = if deny { Permission::Deny } else { Permission::Allow };
            let line: String = to_stdout(&HookOutput::Emit(PreToolUseDecision {
                permission,
                reason,
            }));
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            let field: &serde_json::Value = &value["hookSpecificOutput"]["permissionDecision"];
            prop_assert!(field == "allow" || field == "deny");
        }
    }
}
