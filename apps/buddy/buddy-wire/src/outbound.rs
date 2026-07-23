//! The central→device serializers — the daemon's half of the wire contract.
//!
//! [`inbound`](crate::inbound) owns the device-side *parse* of every line the central sends; this
//! module owns the *serialize* of those same lines, so the bridge and the firmware share **one**
//! contract in both directions. Every function here is a total, faithful mirror: a message
//! serialized on the central and fed back through [`parse_inbound`](crate::parse_inbound)
//! reproduces the original.
//!
//! Pure serialize, no I/O and no clock: the caller supplies the epoch, the tz offset and the
//! packet. This crate stays `std`-but-device-independent (`#![forbid(unsafe_code)]`).
//!
//! ## What this module does NOT do
//!
//! No policy. The "never emit a bare snapshot while a prompt is outstanding" gating rule and the
//! session→snapshot ledger belong to the later `buddy-permission` crate. This module only makes
//! each shape faithfully serializable; *when* to send which shape is the daemon's decision.
//!
//! ## The round-trip invariants (frozen for the later crate to bind to)
//!
//! - [`serialize_snapshot`]: `parse_inbound(&serialize_snapshot(&p)) == Ok(Inbound::Snapshot(p))`
//!   for **any** packet, and a packet with `prompt: None` serializes to a line with **no** prompt
//!   key (absent, never `null`) — so it clears the prompt on the device.
//! - [`serialize_time`]: emits `{"time":[epoch, tz_offset_s]}` with **exactly two** elements, so it
//!   can never be the wrong arity the device rejects; round-trips to
//!   [`Inbound::Time`](crate::Inbound::Time).
//! - [`serialize_owner`] / [`serialize_name`]: the one-shot connect messages, in the `{"cmd":..}`
//!   shape [`parse_command`](crate::inbound) actually reads; round-trip to
//!   [`Command::Owner`](crate::Command::Owner) / [`Command::Name`](crate::Command::Name).

use crate::inbound::SnapshotPacket;
use serde::Serialize;

/// Serialize a heartbeat snapshot to the wire line the device parses as
/// [`Inbound::Snapshot`](crate::Inbound::Snapshot).
///
/// Absent (`None`) fields are **omitted**, never emitted as `null` — see [`SnapshotPacket`]. The
/// frozen invariant is a total round-trip through [`parse_inbound`](crate::parse_inbound): for any
/// `packet`, `parse_inbound(serialize_snapshot(&packet).as_bytes())` yields
/// `Ok(Inbound::Snapshot(packet.clone()))`; and `prompt: None` yields a line with no `prompt` key.
pub fn serialize_snapshot(packet: &SnapshotPacket) -> String {
    // SnapshotPacket derives Serialize with skip_serializing_if on every field, so an absent
    // (None) field stays absent on the wire — never emitted as null. This is the whole mirror.
    serde_json::to_string(packet).expect("a snapshot packet always serializes")
}

/// Serialize a time sync to `{"time":[epoch, tz_offset_s]}` — **always exactly two** elements.
///
/// A wrong arity would fall through the device's parse into the snapshot branch and clear a pending
/// prompt (the device-side fail-open hazard), so the two-element shape is structural here, not
/// merely conventional. Round-trips to [`Inbound::Time`](crate::Inbound::Time) with the same
/// `epoch` and `tz_offset_s`.
pub fn serialize_time(epoch: i64, tz_offset_s: i32) -> String {
    // A fixed-arity typed shape: the two-element array can never be the wrong arity the device
    // rejects (which would fall through and clear a pending prompt).
    let wire: TimeWire = TimeWire {
        time: [epoch, i64::from(tz_offset_s)],
    };
    serde_json::to_string(&wire).expect("a time sync always serializes")
}

/// Serialize the owner-name connect message `{"cmd":"owner","name":<owner>}`.
///
/// The `{"cmd":"owner",..}` shape [`parse_command`](crate::inbound) reads into
/// [`Command::Owner`](crate::Command::Owner); the owner label is carried under the `name` key (the
/// field name `parse_command` reads, not `owner`). Round-trips to
/// `Inbound::Command(Command::Owner(owner.to_string()))`.
pub fn serialize_owner(owner: &str) -> String {
    // parse_command reads the owner label under the `name` key (not `owner`) — match it exactly.
    let wire: CmdNameWire<'_> = CmdNameWire {
        cmd: "owner",
        name: owner,
    };
    serde_json::to_string(&wire).expect("an owner command always serializes")
}

/// Serialize the buddy-name connect message `{"cmd":"name","name":<name>}`.
///
/// The `{"cmd":"name",..}` shape [`parse_command`](crate::inbound) reads into
/// [`Command::Name`](crate::Command::Name). Round-trips to
/// `Inbound::Command(Command::Name(name.to_string()))`.
pub fn serialize_name(name: &str) -> String {
    let wire: CmdNameWire<'_> = CmdNameWire { cmd: "name", name };
    serde_json::to_string(&wire).expect("a name command always serializes")
}

/// The on-wire shape of a time sync: a two-element `[epoch, tz_offset_s]` array under `time`.
#[derive(Serialize)]
struct TimeWire {
    time: [i64; 2],
}

/// The on-wire shape of a `name`/`owner` connect message, both carrying their label under `name`
/// (the key [`parse_command`](crate::inbound) reads), with `cmd`, `name` field order fixed.
#[derive(Serialize)]
struct CmdNameWire<'a> {
    cmd: &'a str,
    name: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;
    use crate::inbound::{parse_inbound, Inbound, Prompt};
    use proptest::prelude::*;

    fn a_prompt() -> Prompt {
        Prompt {
            id: "req_abc".to_string(),
            tool: "bash".to_string(),
            hint: "rm -rf".to_string(),
        }
    }

    // ---- Invariant 2: absent is preserved (unit: prompt present vs absent) ---------------

    #[test]
    fn a_default_packet_serializes_to_an_empty_object() {
        // Every field None: no keys at all, and never a null.
        let line: String = serialize_snapshot(&SnapshotPacket::default());
        assert_eq!(line, "{}");
    }

    #[test]
    fn a_present_prompt_emits_a_prompt_key() {
        let packet: SnapshotPacket = SnapshotPacket {
            prompt: Some(a_prompt()),
            ..SnapshotPacket::default()
        };
        let line: String = serialize_snapshot(&packet);
        assert!(line.contains("\"prompt\""), "a present prompt is emitted");
    }

    #[test]
    fn an_absent_prompt_omits_the_prompt_key() {
        // The anti-hazard pin: prompt:None clears on the device, so the key must be absent.
        let packet: SnapshotPacket = SnapshotPacket {
            running: Some(1),
            ..SnapshotPacket::default()
        };
        let line: String = serialize_snapshot(&packet);
        assert!(
            !line.contains("prompt"),
            "an absent prompt omits the key entirely, never null"
        );
    }

    // ---- Invariant 1 + 2: totality of the snapshot round-trip (proptest) -----------------

    fn a_prompt_strategy() -> impl Strategy<Value = Prompt> {
        // Arbitrary strings, not [a-z]: the round-trip must survive quotes, backslashes, control
        // chars and unicode in prompt fields, since serialize delegates escaping to serde_json.
        (any::<String>(), any::<String>(), any::<String>())
            .prop_map(|(id, tool, hint): (String, String, String)| Prompt { id, tool, hint })
    }

    /// The nine optional wire fields of a [`SnapshotPacket`], in declaration order.
    type PacketFields = (
        Option<u8>,
        Option<u8>,
        Option<u8>,
        Option<bool>,
        Option<u32>,
        Option<u32>,
        Option<String>,
        Option<Vec<String>>,
        Option<Prompt>,
    );

    fn a_packet_strategy() -> impl Strategy<Value = SnapshotPacket> {
        (
            any::<Option<u8>>(),
            any::<Option<u8>>(),
            any::<Option<u8>>(),
            any::<Option<bool>>(),
            any::<Option<u32>>(),
            any::<Option<u32>>(),
            // Arbitrary strings, not [a-z ]: quotes/backslashes/control chars/unicode must
            // round-trip too — a hand-rolled-escaping regression would slip past an [a-z] alphabet.
            any::<Option<String>>(),
            proptest::option::of(proptest::collection::vec(any::<String>(), 0..4)),
            proptest::option::of(a_prompt_strategy()),
        )
            .prop_map(|fields: PacketFields| {
                let (
                    total,
                    running,
                    waiting,
                    completed,
                    tokens,
                    tokens_today,
                    msg,
                    entries,
                    prompt,
                ) = fields;
                SnapshotPacket {
                    total,
                    running,
                    waiting,
                    completed,
                    tokens,
                    tokens_today,
                    msg,
                    entries,
                    prompt,
                }
            })
    }

    proptest! {
        // Invariant 1: for ANY packet, the device parses the serialized line back to the same
        // snapshot — the faithful mirror of parse_inbound in both directions.
        #[test]
        fn any_snapshot_round_trips_through_the_device_parser(packet in a_packet_strategy()) {
            let line: String = serialize_snapshot(&packet);
            let parsed: Option<Inbound> = parse_inbound(line.as_bytes()).ok();
            prop_assert_eq!(parsed, Some(Inbound::Snapshot(packet)));
        }

        // Invariant 2: a packet whose prompt is None never emits a prompt key, nor any null,
        // whatever the other (keep-prior) fields carry — asserted STRUCTURALLY, not by substring,
        // so a msg value that literally spells "prompt" or "null" can never false-red the pin.
        #[test]
        fn an_absent_prompt_never_appears_as_a_key(
            running in any::<Option<u8>>(),
            msg in any::<Option<String>>(),
        ) {
            let packet: SnapshotPacket = SnapshotPacket {
                running,
                msg,
                ..SnapshotPacket::default()
            };
            let line: String = serialize_snapshot(&packet);
            let value: serde_json::Value =
                serde_json::from_str(&line).expect("a snapshot is valid JSON");
            let object: &serde_json::Map<String, serde_json::Value> =
                value.as_object().expect("a snapshot serializes to a JSON object");
            // The absent prompt is omitted entirely: no `prompt` key on the object.
            prop_assert!(!object.contains_key("prompt"));
            // And no field is emitted as null — skip_serializing_if omits, it never nulls.
            prop_assert!(object.values().all(|field: &serde_json::Value| !field.is_null()));
        }
    }

    // ---- Invariant 3: the time sync is a strict two-element array (proptest) --------------

    proptest! {
        // For ANY epoch x tz, the time sync is a two-element array that round-trips — it can
        // never emit the wrong arity the device rejects (which would clear a prompt).
        #[test]
        fn any_time_sync_round_trips_as_a_two_element_array(epoch in any::<i64>(), tz in any::<i32>()) {
            let line: String = serialize_time(epoch, tz);
            let value: serde_json::Value =
                serde_json::from_str(&line).expect("a time sync is valid JSON");
            let len: Option<usize> = value
                .get("time")
                .and_then(|time: &serde_json::Value| time.as_array())
                .map(Vec::len);
            prop_assert_eq!(len, Some(2));
            let parsed: Option<Inbound> = parse_inbound(line.as_bytes()).ok();
            prop_assert_eq!(parsed, Some(Inbound::Time { epoch, tz_offset_s: tz }));
        }
    }

    // ---- Invariant 4: connect messages round-trip to the intended command (zero/one/many) ----

    #[test]
    fn an_empty_owner_name_round_trips_to_an_owner_command() {
        let line: String = serialize_owner("");
        let parsed: Option<Inbound> = parse_inbound(line.as_bytes()).ok();
        assert_eq!(
            parsed,
            Some(Inbound::Command(Command::Owner("".to_string())))
        );
    }

    #[test]
    fn an_owner_name_round_trips_to_an_owner_command() {
        let line: String = serialize_owner("Felix");
        let parsed: Option<Inbound> = parse_inbound(line.as_bytes()).ok();
        assert_eq!(
            parsed,
            Some(Inbound::Command(Command::Owner("Felix".to_string())))
        );
    }

    #[test]
    fn a_multi_word_buddy_name_round_trips_to_a_name_command() {
        let line: String = serialize_name("Rex the Destroyer");
        let parsed: Option<Inbound> = parse_inbound(line.as_bytes()).ok();
        assert_eq!(
            parsed,
            Some(Inbound::Command(Command::Name(
                "Rex the Destroyer".to_string()
            )))
        );
    }

    proptest! {
        // Any owner/name string round-trips through the device parser to the intended command.
        #[test]
        fn any_owner_name_round_trips(name in "[A-Za-z0-9 _]{0,24}") {
            let parsed: Option<Inbound> = parse_inbound(serialize_owner(&name).as_bytes()).ok();
            prop_assert_eq!(parsed, Some(Inbound::Command(Command::Owner(name))));
        }
    }
}
