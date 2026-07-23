//! Parse the Claude Code PreToolUse stdin payload into the daemon-facing [`HookRequest`].
//!
//! The hook binary reads this JSON on stdin; this pure parser turns it into the request the daemon
//! consumes. Two disciplines shape it:
//!
//! - **Tolerant reader.** The real payload carries far more than we use (`tool_input`,
//!   `hook_event_name`, `effort`, `prompt_id`, …). Unknown fields are ignored, and the contextual
//!   fields we merely *display* or *account* with — `cwd`, `permission_mode`, `transcript_path` —
//!   default to empty when absent, so an older or trimmed payload still asks the device.
//! - **Fail safe, never guess.** The two fields we cannot function without — `session_id` (routes
//!   the answer back) and `tool_name` (what the glass shows) — are required. A payload missing
//!   either, or that is not JSON at all, is a typed [`PreToolUseParseError`], NOT a defaulted
//!   request: the hook degrades to silence (the normal terminal prompt), it does not invent a tool
//!   call to approve.

use crate::ipc::HookRequest;
use serde::Deserialize;

/// Why a PreToolUse payload could not be turned into a [`HookRequest`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PreToolUseParseError {
    /// The bytes were not valid JSON, or a required field (`session_id` / `tool_name`) was absent
    /// or the wrong type. Carries serde's message for the log; the hook itself just falls silent.
    Malformed(String),
}

impl core::fmt::Display for PreToolUseParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PreToolUseParseError::Malformed(why) => {
                write!(f, "malformed PreToolUse payload: {why}")
            }
        }
    }
}

impl std::error::Error for PreToolUseParseError {}

/// The subset of the PreToolUse payload we bind to. Serde ignores every other field (tolerant
/// reader); the two required fields have no `#[serde(default)]`, so their absence is a parse error.
#[derive(Deserialize)]
struct PreToolUsePayload {
    /// Required: keys the ledger and routes the device answer back to this call.
    session_id: String,
    /// Required: the tool shown on the glass.
    tool_name: String,
    /// Contextual: the working directory; absent → empty.
    #[serde(default)]
    cwd: String,
    /// Contextual: the permission mode in effect; absent → empty.
    #[serde(default)]
    permission_mode: String,
    /// Contextual: the transcript path (token-accounting raw material); absent → empty.
    #[serde(default)]
    transcript_path: String,
}

/// Parse a PreToolUse stdin payload into a [`HookRequest`], or a typed error the hook degrades on.
pub fn parse_pretooluse(bytes: &[u8]) -> Result<HookRequest, PreToolUseParseError> {
    let payload: PreToolUsePayload = serde_json::from_slice(bytes)
        .map_err(|err: serde_json::Error| PreToolUseParseError::Malformed(err.to_string()))?;
    Ok(HookRequest {
        session_id: payload.session_id,
        tool_name: payload.tool_name,
        cwd: payload.cwd,
        permission_mode: payload.permission_mode,
        transcript_path: payload.transcript_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A full payload parses into exactly the forwarded request.
    #[test]
    fn a_full_payload_parses_to_the_request() {
        let bytes: &[u8] = br#"{"session_id":"s1","tool_name":"Bash","cwd":"/w",
            "permission_mode":"default","transcript_path":"/t.jsonl"}"#;
        assert_eq!(
            parse_pretooluse(bytes),
            Ok(HookRequest {
                session_id: "s1".to_string(),
                tool_name: "Bash".to_string(),
                cwd: "/w".to_string(),
                permission_mode: "default".to_string(),
                transcript_path: "/t.jsonl".to_string(),
            })
        );
    }

    /// Tolerant reader: the many fields we do not use are ignored, not rejected.
    #[test]
    fn unknown_fields_are_ignored() {
        let bytes: &[u8] = br#"{"session_id":"s1","tool_name":"Edit",
            "hook_event_name":"PreToolUse","tool_input":{"file_path":"/x"},
            "effort":{"level":"high"},"prompt_id":"p9"}"#;
        assert_eq!(
            parse_pretooluse(bytes),
            Ok(HookRequest {
                session_id: "s1".to_string(),
                tool_name: "Edit".to_string(),
                cwd: String::new(),
                permission_mode: String::new(),
                transcript_path: String::new(),
            })
        );
    }

    /// The contextual fields default to empty when absent — a trimmed payload still asks.
    #[test]
    fn absent_contextual_fields_default_to_empty() {
        let bytes: &[u8] = br#"{"session_id":"s1","tool_name":"Write"}"#;
        assert_eq!(
            parse_pretooluse(bytes),
            Ok(HookRequest {
                session_id: "s1".to_string(),
                tool_name: "Write".to_string(),
                cwd: String::new(),
                permission_mode: String::new(),
                transcript_path: String::new(),
            })
        );
    }

    /// A missing `session_id` is a parse error — we cannot route the answer, so we never guess.
    #[test]
    fn a_missing_session_id_is_malformed() {
        let bytes: &[u8] = br#"{"tool_name":"Bash"}"#;
        assert!(matches!(
            parse_pretooluse(bytes),
            Err(PreToolUseParseError::Malformed(_))
        ));
    }

    /// A missing `tool_name` is a parse error — there is nothing to show the owner.
    #[test]
    fn a_missing_tool_name_is_malformed() {
        let bytes: &[u8] = br#"{"session_id":"s1"}"#;
        assert!(matches!(
            parse_pretooluse(bytes),
            Err(PreToolUseParseError::Malformed(_))
        ));
    }

    /// Bytes that are not JSON at all are malformed, not a panic.
    #[test]
    fn non_json_bytes_are_malformed() {
        assert!(matches!(
            parse_pretooluse(b"not json"),
            Err(PreToolUseParseError::Malformed(_))
        ));
    }

    proptest! {
        /// Totality: any payload carrying both required strings parses, and the two load-bearing
        /// fields round-trip verbatim whatever else the payload contains.
        #[test]
        fn any_payload_with_the_two_required_fields_parses(
            session_id in "[a-zA-Z0-9_-]{1,40}",
            tool_name in "[A-Za-z]{1,20}",
        ) {
            let bytes: Vec<u8> = format!(
                r#"{{"session_id":"{session_id}","tool_name":"{tool_name}"}}"#
            )
            .into_bytes();
            let request: HookRequest = parse_pretooluse(&bytes).unwrap();
            prop_assert_eq!(request.session_id, session_id);
            prop_assert_eq!(request.tool_name, tool_name);
        }
    }
}
