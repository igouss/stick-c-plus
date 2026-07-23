//! The device→central permission response — the buddy answering an approval prompt.
//!
//! The one message the device *originates* on the wire:
//! `{"cmd":"permission","id":<promptId>,"decision":"once"|"deny"}`. The prompt id echoes the
//! id the snapshot delivered (see [`crate::inbound::Prompt`]); the decision is the owner's
//! button press.

use serde::Serialize;

/// The owner's decision on a permission prompt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    /// Approve this once — serializes as `"once"`.
    Once,
    /// Deny — serializes as `"deny"`.
    Deny,
}

impl Decision {
    /// The wire token for this decision (`"once"` or `"deny"`).
    pub const fn as_wire(self) -> &'static str {
        match self {
            Decision::Once => "once",
            Decision::Deny => "deny",
        }
    }
}

/// A permission response the device sends up: which prompt, and the decision.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PermissionResponse {
    /// The prompt id being answered (echoes the snapshot's prompt id).
    pub id: String,
    /// The owner's decision.
    pub decision: Decision,
}

impl PermissionResponse {
    /// A response answering prompt `id` with `decision`.
    pub fn new(id: String, decision: Decision) -> Self {
        PermissionResponse { id, decision }
    }

    /// Serialize to the wire line
    /// `{"cmd":"permission","id":<id>,"decision":"once"|"deny"}` (no trailing newline; the
    /// transport adds framing).
    pub fn to_json(&self) -> String {
        let wire: PermissionWire<'_> = PermissionWire {
            cmd: "permission",
            id: &self.id,
            decision: self.decision.as_wire(),
        };
        serde_json::to_string(&wire).expect("a permission response always serializes")
    }
}

/// The on-wire shape of a permission response, with the field order fixed at `cmd`, `id`,
/// `decision`.
#[derive(Serialize)]
struct PermissionWire<'a> {
    cmd: &'a str,
    id: &'a str,
    decision: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn the_wire_token_for_once_is_once() {
        assert_eq!(Decision::Once.as_wire(), "once");
    }

    #[test]
    fn the_wire_token_for_deny_is_deny() {
        assert_eq!(Decision::Deny.as_wire(), "deny");
    }

    #[test]
    fn an_approve_response_serializes_to_the_permission_line() {
        let response: PermissionResponse =
            PermissionResponse::new("req_abc123".to_string(), Decision::Once);
        assert_eq!(
            response.to_json(),
            r#"{"cmd":"permission","id":"req_abc123","decision":"once"}"#
        );
    }

    #[test]
    fn a_deny_response_serializes_with_the_deny_decision() {
        let response: PermissionResponse =
            PermissionResponse::new("req_abc123".to_string(), Decision::Deny);
        assert_eq!(
            response.to_json(),
            r#"{"cmd":"permission","id":"req_abc123","decision":"deny"}"#
        );
    }

    #[test]
    fn the_id_echoes_the_prompt_id_verbatim() {
        let response: PermissionResponse =
            PermissionResponse::new("req_XYZ_9".to_string(), Decision::Once);
        assert_eq!(response.id, "req_XYZ_9");
    }

    proptest! {
        // Any prompt id round-trips into the response line as a JSON string with the fixed
        // cmd and a valid decision token.
        #[test]
        fn any_prompt_id_serializes_into_the_line(id in "[A-Za-z0-9_]{1,32}") {
            let response: PermissionResponse =
                PermissionResponse::new(id.clone(), Decision::Once);
            let json: String = response.to_json();
            let starts: bool = json.starts_with(r#"{"cmd":"permission","id":""#);
            let carries_id: bool = json.contains(&format!(r#""id":"{id}""#));
            let ends: bool = json.ends_with(r#""decision":"once"}"#);
            prop_assert!(starts);
            prop_assert!(carries_id);
            prop_assert!(ends);
        }
    }
}
