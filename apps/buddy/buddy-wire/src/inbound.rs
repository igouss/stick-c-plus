//! Classifying one inbound line, and the asymmetric snapshot merge.
//!
//! [`Inbound`] is an **exhaustive** enum over the four message shapes: a heartbeat snapshot, a
//! time sync, a command, and a transcript event. [`parse_inbound`] classifies a line into one
//! of them (or a typed error). The classification is the whole point of defect fix (b/D1).
//!
//! ## Defect fix (b / D1): only a snapshot may touch prompt state
//!
//! Upstream had no `evt` branch, so a `{"evt":"turn"}` fell through into the snapshot merge,
//! where an absent `prompt` field cleared a pending approval off the glass — a routine
//! transcript event wiping a live decision. Here every non-snapshot shape is its **own**
//! variant, and only [`Inbound::Snapshot`] carries a [`SnapshotPacket`] that can reach
//! [`SnapshotState::merge`]. The wire format is unchanged; the fix is structural.
//!
//! ## The merge is asymmetric on absent fields
//!
//! | Field | Absent behaviour |
//! |---|---|
//! | `completed` | **resets to `false`** |
//! | `prompt` | **clears** id / tool / hint |
//! | `total`, `running`, `waiting`, `tokens_today`, `msg`, `entries` | **keep prior** |
//! | `tokens` | forwarded to the stats latch **only** when present as a `u32` |
//!
//! ## Time sync arity is strict
//!
//! A time sync is accepted **only** as a top-level array of exactly two elements —
//! `{"time":[epoch, tz_offset_seconds]}`. Wrong arity is [`InboundError::TimeArity`], **not**
//! a fall-through to the snapshot merge (which upstream did, clearing the prompt).

use serde::{Deserialize, Serialize};

/// A pending approval prompt on the wire: the fields the snapshot may set or clear.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Serialize)]
pub struct Prompt {
    /// The prompt id echoed back in a permission response.
    pub id: String,
    /// The tool the prompt is about.
    pub tool: String,
    /// A short human hint.
    pub hint: String,
}

/// A transcript event — the `{"evt":..}` shape. Its own variant so it can **never** reach the
/// snapshot merge (defect fix b/D1).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BuddyEvent {
    /// The documented `{"evt":"turn"}` event — a turn boundary, which must **not** disturb a
    /// pending prompt.
    Turn,
    /// Any other `evt` value — carried, still kept away from prompt state.
    Other(String),
}

/// One parsed inbound message — an exhaustive classification of the wire line.
///
/// Exhaustive by design: adding a shape forces every consumer to handle it, and only
/// [`Snapshot`](Inbound::Snapshot) can touch prompt state.
#[derive(Clone, PartialEq, Debug)]
pub enum Inbound {
    /// A heartbeat snapshot — the only shape that merges into [`SnapshotState`].
    Snapshot(SnapshotPacket),
    /// A time sync: a top-level two-element array `{"time":[epoch, tz_offset_seconds]}`.
    Time {
        /// Seconds since the Unix epoch.
        epoch: i64,
        /// The owner's UTC offset, in seconds, folded into the epoch downstream.
        tz_offset_s: i32,
    },
    /// A device command (see [`crate::command`]).
    Command(crate::command::Command),
    /// A transcript event (kept away from prompt state).
    Event(BuddyEvent),
}

/// Why a line could not be classified into an [`Inbound`].
#[derive(Debug)]
pub enum InboundError {
    /// The line was not JSON matching any shape.
    Json(serde_json::Error),
    /// A `{"time":[..]}` array was not exactly two elements — a strict error, never a
    /// fall-through to the snapshot merge.
    TimeArity,
}

impl core::fmt::Display for InboundError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            InboundError::Json(err) => write!(f, "inbound line was not a usable message: {err}"),
            InboundError::TimeArity => write!(f, "time sync array was not exactly two elements"),
        }
    }
}

impl std::error::Error for InboundError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InboundError::Json(err) => Some(err),
            InboundError::TimeArity => None,
        }
    }
}

impl From<serde_json::Error> for InboundError {
    fn from(err: serde_json::Error) -> Self {
        InboundError::Json(err)
    }
}

/// The parsed fields of one snapshot packet: absent fields are `None` and drive the asymmetric
/// merge in [`SnapshotState::merge`].
///
/// The same DTO round-trips in **both** directions: [`parse_inbound`] deserializes it device-side,
/// and [`crate::outbound::serialize_snapshot`] serializes it central-side. Every field is
/// `#[serde(skip_serializing_if = "Option::is_none")]`, so an absent field stays **absent** on the
/// wire — omitting `prompt` clears it on the device, omitting a keep-prior field keeps it. Emitting
/// `null` would be a different, wrong message; skipping is load-bearing.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize, Serialize)]
pub struct SnapshotPacket {
    /// Total session count, capped `u8`; absent keeps prior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u8>,
    /// Running session count; absent keeps prior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running: Option<u8>,
    /// Waiting session count; absent keeps prior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting: Option<u8>,
    /// Completion flag; **absent resets to `false`**.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<bool>,
    /// Desktop token total; forwarded to the stats latch **only** when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u32>,
    /// Today's token count; absent keeps prior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_today: Option<u32>,
    /// The status line; absent keeps prior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg: Option<String>,
    /// The transcript entries; absent keeps prior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<String>>,
    /// The pending prompt; **absent clears** id / tool / hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<Prompt>,
}

/// The merged snapshot state the firmware maps into a `buddy_core::Snapshot`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SnapshotState {
    /// Total session count.
    pub total: u8,
    /// Running session count.
    pub running: u8,
    /// Waiting session count.
    pub waiting: u8,
    /// Whether a session completed in the last packet (reset by an absent `completed`).
    pub completed: bool,
    /// Today's token count.
    pub tokens_today: u32,
    /// The status line.
    pub msg: String,
    /// The transcript entries, oldest-first.
    pub entries: Vec<String>,
    /// The pending prompt, or `None` when cleared.
    pub prompt: Option<Prompt>,
}

impl SnapshotState {
    /// Merge one packet into this state, asymmetrically on absent fields (see the module
    /// docs), and return the desktop token total to forward to the stats latch **only** when
    /// the packet carried `tokens` as a `u32`.
    pub fn merge(&mut self, packet: &SnapshotPacket) -> Option<u32> {
        // Keep-prior fields: an absent value leaves the field untouched.
        if let Some(total) = packet.total {
            self.total = total;
        }
        if let Some(running) = packet.running {
            self.running = running;
        }
        if let Some(waiting) = packet.waiting {
            self.waiting = waiting;
        }
        if let Some(tokens_today) = packet.tokens_today {
            self.tokens_today = tokens_today;
        }
        if let Some(msg) = packet.msg.as_ref() {
            self.msg = msg.clone();
        }
        if let Some(entries) = packet.entries.as_ref() {
            // Oldest-first (D7): the wire order is preserved verbatim, index 0 the oldest.
            self.entries = entries.clone();
        }
        // Destructive-on-absent fields: an absent value is a reset, not a keep.
        self.completed = packet.completed.unwrap_or(false);
        self.prompt = packet.prompt.clone();
        // Forwarded to the stats latch only when the packet carried `tokens` (as a u32).
        packet.tokens
    }
}

/// Classify one inbound line into an [`Inbound`], or a typed [`InboundError`].
///
/// Dispatch order: a `{"time":[..]}` array (strict arity) → [`Inbound::Time`]; a `{"evt":..}`
/// object → [`Inbound::Event`]; a `{"cmd":..}` object → [`Inbound::Command`]; otherwise →
/// [`Inbound::Snapshot`]. Only the last can touch prompt state.
pub fn parse_inbound(line: &[u8]) -> Result<Inbound, InboundError> {
    let value: serde_json::Value = serde_json::from_slice(line)?;

    // A `{"time":[..]}` is a time sync — strict arity, and never a fall-through to the merge.
    if let Some(time) = value.get("time") {
        return parse_time(time);
    }
    // A `{"evt":..}` is a transcript event — its own variant, kept away from prompt state (D1).
    if let Some(evt) = value.get("evt") {
        return Ok(Inbound::Event(parse_event(evt)));
    }
    // A `{"cmd":..}` is a device command.
    if let Some(cmd) = value.get("cmd") {
        return Ok(Inbound::Command(parse_command(&value, cmd)));
    }
    // Otherwise it is a heartbeat snapshot — the only shape that may touch prompt state.
    let packet: SnapshotPacket = serde_json::from_value(value)?;
    Ok(Inbound::Snapshot(packet))
}

/// Parse the `time` value as the strict two-element array `[epoch, tz_offset_seconds]`.
///
/// Anything other than a length-2 array is [`InboundError::TimeArity`] — never a fall-through
/// to the snapshot merge (which upstream did, clearing the prompt).
fn parse_time(time: &serde_json::Value) -> Result<Inbound, InboundError> {
    let array: &Vec<serde_json::Value> = time.as_array().ok_or(InboundError::TimeArity)?;
    if array.len() != 2 {
        return Err(InboundError::TimeArity);
    }
    let epoch: i64 = array[0].as_i64().ok_or(InboundError::TimeArity)?;
    let tz_offset_s: i64 = array[1].as_i64().ok_or(InboundError::TimeArity)?;
    Ok(Inbound::Time {
        epoch,
        tz_offset_s: tz_offset_s as i32,
    })
}

/// Classify the `evt` value: the documented `"turn"`, or any other event as [`BuddyEvent::Other`].
fn parse_event(evt: &serde_json::Value) -> BuddyEvent {
    match evt.as_str() {
        Some("turn") => BuddyEvent::Turn,
        Some(other) => BuddyEvent::Other(other.to_string()),
        None => BuddyEvent::Other(evt.to_string()),
    }
}

/// Classify the `cmd` value into a [`Command`], with anything unrecognised as
/// [`Command::Unknown`] so it can be nacked, never swallowed (defect fix a/D3).
fn parse_command(root: &serde_json::Value, cmd: &serde_json::Value) -> crate::command::Command {
    use crate::command::Command;

    let name: &str = match cmd.as_str() {
        Some(name) => name,
        None => return Command::Unknown(cmd.to_string()),
    };
    let string_field = |key: &str| -> String {
        root.get(key)
            .and_then(|value: &serde_json::Value| value.as_str())
            .unwrap_or("")
            .to_string()
    };
    match name {
        "name" => Command::Name(string_field("name")),
        "species" => {
            // `0xFF` (255) is the "use the installed GIF" sentinel, and the absent default.
            let idx: u8 = root
                .get("idx")
                .and_then(|value: &serde_json::Value| value.as_u64())
                .map(|idx: u64| idx as u8)
                .unwrap_or(0xFF);
            Command::Species(idx)
        }
        "unpair" => Command::Unpair,
        "owner" => Command::Owner(string_field("name")),
        "status" => Command::Status,
        "char_begin" => Command::CharBegin,
        "file" => Command::File {
            path: string_field("path"),
        },
        "chunk" => Command::Chunk,
        "file_end" => Command::FileEnd,
        "char_end" => Command::CharEnd,
        other => Command::Unknown(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use proptest::prelude::*;

    fn a_prompt() -> Prompt {
        Prompt {
            id: "req_abc".to_string(),
            tool: "bash".to_string(),
            hint: "rm -rf".to_string(),
        }
    }

    // ---- Classification: the exhaustive Inbound ------------------------------------------

    #[test]
    fn a_bare_object_classifies_as_a_snapshot() {
        let inbound: Inbound = parse_inbound(br#"{"running":2}"#).expect("valid snapshot");
        assert!(matches!(inbound, Inbound::Snapshot(_)));
    }

    #[test]
    fn a_two_element_time_array_classifies_as_time() {
        let inbound: Inbound =
            parse_inbound(br#"{"time":[1775731234,-25200]}"#).expect("valid time");
        assert_eq!(
            inbound,
            Inbound::Time {
                epoch: 1775731234,
                tz_offset_s: -25200,
            }
        );
    }

    #[test]
    fn a_cmd_object_classifies_as_a_command() {
        let inbound: Inbound = parse_inbound(br#"{"cmd":"status"}"#).expect("valid command");
        assert_eq!(inbound, Inbound::Command(Command::Status));
    }

    // ---- Defect fix (b / D1): an event never reaches the snapshot merge ------------------

    #[test]
    fn an_evt_turn_classifies_as_an_event_not_a_snapshot() {
        // The pin: upstream had no evt branch, so {"evt":"turn"} fell into the snapshot merge
        // and an absent prompt cleared the pending approval. Here it is its own variant.
        let inbound: Inbound =
            parse_inbound(br#"{"evt":"turn","role":"assistant"}"#).expect("valid event");
        assert_eq!(inbound, Inbound::Event(BuddyEvent::Turn));
        assert!(
            !matches!(inbound, Inbound::Snapshot(_)),
            "a turn event must never be a snapshot"
        );
    }

    #[test]
    fn a_turn_event_cannot_be_merged_because_it_carries_no_packet() {
        // Structural proof of D1: only Inbound::Snapshot yields a SnapshotPacket, so a turn
        // event cannot reach SnapshotState::merge to clear a prompt.
        let mut state: SnapshotState = SnapshotState {
            prompt: Some(a_prompt()),
            ..SnapshotState::default()
        };
        let before: Option<Prompt> = state.prompt.clone();
        let inbound: Inbound = parse_inbound(br#"{"evt":"turn"}"#).expect("event");
        if let Inbound::Snapshot(packet) = inbound {
            state.merge(&packet);
        }
        assert_eq!(
            state.prompt, before,
            "a turn event leaves a pending prompt intact"
        );
    }

    #[test]
    fn a_valid_time_sync_leaves_a_pending_prompt_intact() {
        // D1, positive side: a correct-arity {"time":[epoch,tz]} is Inbound::Time, not Snapshot,
        // so it can never reach merge to clear a prompt — the same protection the evt case gets.
        let mut state: SnapshotState = SnapshotState {
            prompt: Some(a_prompt()),
            ..SnapshotState::default()
        };
        let before: Option<Prompt> = state.prompt.clone();
        let inbound: Inbound =
            parse_inbound(br#"{"time":[1775731234,-14400]}"#).expect("valid time");
        assert!(
            matches!(inbound, Inbound::Time { .. }),
            "a two-element time is a time-sync"
        );
        if let Inbound::Snapshot(packet) = inbound {
            state.merge(&packet);
        }
        assert_eq!(
            state.prompt, before,
            "a time sync leaves a pending prompt intact"
        );
    }

    #[test]
    fn a_known_command_leaves_a_pending_prompt_intact() {
        // D1, positive side: a recognised {"cmd":..} is Inbound::Command, not Snapshot, so it
        // cannot reach merge either.
        let mut state: SnapshotState = SnapshotState {
            prompt: Some(a_prompt()),
            ..SnapshotState::default()
        };
        let before: Option<Prompt> = state.prompt.clone();
        let inbound: Inbound = parse_inbound(br#"{"cmd":"status"}"#).expect("known command");
        assert!(
            matches!(inbound, Inbound::Command(_)),
            "a cmd is a command, not a snapshot"
        );
        if let Inbound::Snapshot(packet) = inbound {
            state.merge(&packet);
        }
        assert_eq!(
            state.prompt, before,
            "a known command leaves a pending prompt intact"
        );
    }

    #[test]
    fn an_unknown_evt_value_is_carried_as_other() {
        let inbound: Inbound = parse_inbound(br#"{"evt":"heartbeat"}"#).expect("event");
        assert_eq!(
            inbound,
            Inbound::Event(BuddyEvent::Other("heartbeat".to_string()))
        );
    }

    // ---- Time-sync arity is strict, never a fall-through --------------------------------

    #[test]
    fn a_one_element_time_array_is_an_arity_error() {
        let error: InboundError = parse_inbound(br#"{"time":[1775731234]}"#).expect_err("arity");
        assert!(matches!(error, InboundError::TimeArity));
    }

    #[test]
    fn a_three_element_time_array_is_an_arity_error() {
        let error: InboundError = parse_inbound(br#"{"time":[1,2,3]}"#).expect_err("arity");
        assert!(matches!(error, InboundError::TimeArity));
    }

    #[test]
    fn a_wrong_arity_time_does_not_fall_through_to_a_snapshot() {
        // The pin: upstream's wrong-arity time fell into the merge and cleared the prompt.
        // Here it is an error, so it is never handed to merge at all.
        let result: Result<Inbound, InboundError> = parse_inbound(br#"{"time":[1],"prompt":null}"#);
        assert!(
            result.is_err(),
            "a wrong-arity time is an error, not a prompt-clearing snapshot"
        );
    }

    // ---- The asymmetric merge ------------------------------------------------------------

    #[test]
    fn an_absent_completed_resets_to_false() {
        let mut state: SnapshotState = SnapshotState {
            completed: true,
            ..SnapshotState::default()
        };
        let packet: SnapshotPacket = SnapshotPacket::default();
        state.merge(&packet);
        assert!(!state.completed, "absent completed resets to false");
    }

    #[test]
    fn a_present_completed_sets_true() {
        let mut state: SnapshotState = SnapshotState::default();
        let packet: SnapshotPacket = SnapshotPacket {
            completed: Some(true),
            ..SnapshotPacket::default()
        };
        state.merge(&packet);
        assert!(state.completed);
    }

    #[test]
    fn an_absent_prompt_clears_a_pending_prompt() {
        let mut state: SnapshotState = SnapshotState {
            prompt: Some(a_prompt()),
            ..SnapshotState::default()
        };
        let packet: SnapshotPacket = SnapshotPacket::default();
        state.merge(&packet);
        assert_eq!(state.prompt, None, "absent prompt clears id/tool/hint");
    }

    #[test]
    fn a_present_prompt_is_set() {
        let mut state: SnapshotState = SnapshotState::default();
        let packet: SnapshotPacket = SnapshotPacket {
            prompt: Some(a_prompt()),
            ..SnapshotPacket::default()
        };
        state.merge(&packet);
        assert_eq!(state.prompt, Some(a_prompt()));
    }

    #[test]
    fn an_absent_total_keeps_the_prior_value() {
        let mut state: SnapshotState = SnapshotState {
            total: 7,
            ..SnapshotState::default()
        };
        let packet: SnapshotPacket = SnapshotPacket::default();
        state.merge(&packet);
        assert_eq!(state.total, 7, "absent total keeps prior");
    }

    #[test]
    fn an_absent_msg_and_entries_keep_prior() {
        let mut state: SnapshotState = SnapshotState {
            msg: "hello".to_string(),
            entries: vec!["a".to_string(), "b".to_string()],
            ..SnapshotState::default()
        };
        let packet: SnapshotPacket = SnapshotPacket::default();
        state.merge(&packet);
        assert_eq!(state.msg, "hello");
        assert_eq!(state.entries, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn entries_are_stored_oldest_first_in_wire_order() {
        // D7: index 0 is the oldest; the wire order is preserved verbatim.
        let mut state: SnapshotState = SnapshotState::default();
        let packet: SnapshotPacket = SnapshotPacket {
            entries: Some(vec![
                "oldest".to_string(),
                "middle".to_string(),
                "newest".to_string(),
            ]),
            ..SnapshotPacket::default()
        };
        state.merge(&packet);
        assert_eq!(state.entries.first().map(String::as_str), Some("oldest"));
        assert_eq!(state.entries.last().map(String::as_str), Some("newest"));
    }

    // ---- Token latch forwarding (the three-case handling lives in buddy-core) ------------

    #[test]
    fn a_present_tokens_is_forwarded_to_the_latch() {
        let mut state: SnapshotState = SnapshotState::default();
        let packet: SnapshotPacket = SnapshotPacket {
            tokens: Some(12345),
            ..SnapshotPacket::default()
        };
        assert_eq!(state.merge(&packet), Some(12345));
    }

    #[test]
    fn an_absent_tokens_forwards_nothing() {
        let mut state: SnapshotState = SnapshotState::default();
        let packet: SnapshotPacket = SnapshotPacket::default();
        assert_eq!(state.merge(&packet), None);
    }

    #[test]
    fn tokens_forwarding_survives_a_full_wire_snapshot() {
        let mut state: SnapshotState = SnapshotState::default();
        let inbound: Inbound = parse_inbound(br#"{"tokens":900,"running":1}"#).expect("snapshot");
        let forwarded: Option<u32> = match inbound {
            Inbound::Snapshot(packet) => state.merge(&packet),
            _ => panic!("expected a snapshot"),
        };
        assert_eq!(forwarded, Some(900));
    }

    // ---- Never panics --------------------------------------------------------------------

    #[test]
    fn garbage_bytes_are_an_error_not_a_panic() {
        assert!(parse_inbound(b"not json at all").is_err());
    }

    #[test]
    fn a_non_object_json_value_is_a_snapshot_error() {
        // A bare array has no time/evt/cmd keys and is not a snapshot object either.
        assert!(parse_inbound(b"[1,2,3]").is_err());
    }

    // ---- Properties ----------------------------------------------------------------------

    proptest! {
        // parse_inbound never panics on arbitrary bytes.
        #[test]
        fn parse_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
            let _: Result<Inbound, InboundError> = parse_inbound(&bytes);
        }

        // Absent completed always resets to false, whatever the prior value and other fields.
        #[test]
        fn absent_completed_always_resets(prior in any::<bool>(), total in any::<u8>()) {
            let mut state: SnapshotState = SnapshotState {
                completed: prior,
                total,
                ..SnapshotState::default()
            };
            state.merge(&SnapshotPacket::default());
            prop_assert!(!state.completed);
        }

        // Absent prompt always clears, whatever the prior prompt.
        #[test]
        fn absent_prompt_always_clears(id in "[a-z]{1,8}") {
            let mut state: SnapshotState = SnapshotState {
                prompt: Some(Prompt { id, tool: "t".to_string(), hint: "h".to_string() }),
                ..SnapshotState::default()
            };
            state.merge(&SnapshotPacket::default());
            prop_assert_eq!(state.prompt, None);
        }

        // A well-formed two-element time array always classifies as Time with those values.
        #[test]
        fn a_two_element_time_always_classifies(epoch in any::<i32>(), tz in -50000i32..50000) {
            let line: String = format!(r#"{{"time":[{epoch},{tz}]}}"#);
            let inbound: Inbound = parse_inbound(line.as_bytes()).expect("valid time");
            prop_assert_eq!(inbound, Inbound::Time { epoch: epoch as i64, tz_offset_s: tz });
        }
    }
}
