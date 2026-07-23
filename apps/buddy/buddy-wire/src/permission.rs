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

/// Why a line could not be read back into a [`PermissionResponse`] central-side.
///
/// A wrong or missing `decision` is a **typed** error, never a silent [`Decision::Once`]: a wrong
/// default here would approve a call the owner denied, so the parse refuses to guess.
#[derive(Debug)]
pub enum PermissionParseError {
    /// The line was not JSON.
    Json(serde_json::Error),
    /// The line was not a `{"cmd":"permission",..}` object (wrong or absent `cmd`).
    NotPermission,
    /// The `id` field was absent or not a string.
    MissingId,
    /// The `decision` token was absent or neither `"once"` nor `"deny"` — carried verbatim so the
    /// caller can log it. **Never** silently defaulted to `Once`.
    BadDecision(String),
}

impl core::fmt::Display for PermissionParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PermissionParseError::Json(err) => {
                write!(f, "permission line was not usable JSON: {err}")
            }
            PermissionParseError::NotPermission => {
                write!(f, "line was not a permission response")
            }
            PermissionParseError::MissingId => write!(f, "permission response had no id"),
            PermissionParseError::BadDecision(token) => {
                write!(f, "permission decision was not once/deny: {token:?}")
            }
        }
    }
}

impl std::error::Error for PermissionParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PermissionParseError::Json(err) => Some(err),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for PermissionParseError {
    fn from(err: serde_json::Error) -> Self {
        PermissionParseError::Json(err)
    }
}

impl PermissionResponse {
    /// A response answering prompt `id` with `decision`.
    pub fn new(id: String, decision: Decision) -> Self {
        PermissionResponse { id, decision }
    }

    /// Parse the central-side read of a permission line
    /// `{"cmd":"permission","id":<id>,"decision":"once"|"deny"}` back into a
    /// [`PermissionResponse`] — the exact mirror of [`to_json`](Self::to_json).
    ///
    /// Round-trips with `to_json`: `PermissionResponse::parse(r.to_json().as_bytes()) == Ok(r)` for
    /// any response. An unknown or absent `decision` token is [`PermissionParseError::BadDecision`],
    /// **never** a silent [`Decision::Once`] — approving a denied tool call is the one failure this
    /// parse exists to prevent.
    pub fn parse(line: &[u8]) -> Result<PermissionResponse, PermissionParseError> {
        let value: serde_json::Value = serde_json::from_slice(line)?;
        // The shape must be a permission command; a wrong or absent `cmd` is not our message.
        if value.get("cmd").and_then(serde_json::Value::as_str) != Some("permission") {
            return Err(PermissionParseError::NotPermission);
        }
        // The id must be present as a string — the prompt it answers.
        let id: String = value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or(PermissionParseError::MissingId)?
            .to_string();
        // The decision is the load-bearing field: an unknown or absent token is a typed error,
        // never a silent Decision::Once (which would approve a denied call).
        let decision: Decision = match value.get("decision").and_then(serde_json::Value::as_str) {
            Some("once") => Decision::Once,
            Some("deny") => Decision::Deny,
            Some(other) => return Err(PermissionParseError::BadDecision(other.to_string())),
            None => return Err(PermissionParseError::BadDecision(String::new())),
        };
        Ok(PermissionResponse { id, decision })
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

/// The central-side read of a permission line — the mirror of [`PermissionResponse::to_json`].
#[cfg(test)]
mod central_parse {
    use super::*;
    use proptest::prelude::*;

    // ---- Invariant 5: the central parse round-trips with the device-side to_json ----------

    #[test]
    fn a_once_response_round_trips_central_side() {
        let response: PermissionResponse =
            PermissionResponse::new("req_abc".to_string(), Decision::Once);
        assert_eq!(
            PermissionResponse::parse(response.to_json().as_bytes()).ok(),
            Some(response)
        );
    }

    #[test]
    fn a_deny_response_round_trips_central_side() {
        let response: PermissionResponse =
            PermissionResponse::new("req_abc".to_string(), Decision::Deny);
        assert_eq!(
            PermissionResponse::parse(response.to_json().as_bytes()).ok(),
            Some(response)
        );
    }

    // ---- Invariant 6: a malformed/unknown line is a TYPED error, never a silent Once -------

    #[test]
    fn an_unknown_decision_token_is_a_bad_decision_not_a_silent_once() {
        // The load-bearing pin: a wrong default here would approve a denied tool call.
        let result: Result<PermissionResponse, PermissionParseError> =
            PermissionResponse::parse(br#"{"cmd":"permission","id":"req_abc","decision":"maybe"}"#);
        assert!(
            matches!(result, Err(PermissionParseError::BadDecision(_))),
            "an unknown decision is a typed error, never Ok(Once)"
        );
    }

    #[test]
    fn an_absent_decision_is_a_bad_decision_not_a_silent_once() {
        let result: Result<PermissionResponse, PermissionParseError> =
            PermissionResponse::parse(br#"{"cmd":"permission","id":"req_abc"}"#);
        assert!(
            matches!(result, Err(PermissionParseError::BadDecision(_))),
            "an absent decision is a typed error, never Ok(Once)"
        );
    }

    #[test]
    fn an_absent_id_is_a_missing_id_error() {
        let result: Result<PermissionResponse, PermissionParseError> =
            PermissionResponse::parse(br#"{"cmd":"permission","decision":"once"}"#);
        assert!(matches!(result, Err(PermissionParseError::MissingId)));
    }

    #[test]
    fn a_non_permission_cmd_is_a_not_permission_error() {
        let result: Result<PermissionResponse, PermissionParseError> =
            PermissionResponse::parse(br#"{"cmd":"status"}"#);
        assert!(matches!(result, Err(PermissionParseError::NotPermission)));
    }

    #[test]
    fn garbage_bytes_are_a_typed_error_not_a_panic() {
        assert!(PermissionResponse::parse(b"not json").is_err());
    }

    proptest! {
        // Invariant 5, totality: for ANY id and either decision, the central parse round-trips
        // with the device-side to_json.
        #[test]
        fn any_response_round_trips_central_side(
            id in "[A-Za-z0-9_]{1,32}",
            deny in any::<bool>(),
        ) {
            let decision: Decision = if deny { Decision::Deny } else { Decision::Once };
            let response: PermissionResponse = PermissionResponse::new(id, decision);
            let parsed: Option<PermissionResponse> =
                PermissionResponse::parse(response.to_json().as_bytes()).ok();
            prop_assert_eq!(parsed, Some(response));
        }
    }
}
