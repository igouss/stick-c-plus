//! Golden known-answer test for the plaintext frame codec.
//!
//! The frames in `fixtures/plaintext_frames.tsv` were **captured from
//! aioesphomeapi** — its protobuf serialised the payload and its own frame
//! helper wrapped it (see the fixture header and `tools/capture_frames.py`).
//! For each case this test proves two directions against that captured wire:
//!
//!   1. **encode** — building the same prost message here and framing it yields
//!      bytes identical to the captured frame, and
//!   2. **decode** — feeding the captured frame to [`decode_frame`] recovers the
//!      captured `(msg_type, payload)`.
//!
//! Matching a foreign capture (not our own re-encode) is what makes this a
//! known-answer test rather than a round-trip: it proves our understanding of
//! the wire matches the client Home Assistant actually ships, per the crate's
//! two-oracle discipline (see PROVENANCE.md).

use esphome_api::proto::{
    DeviceInfoResponse, HelloResponse, ListEntitiesSensorResponse, PingRequest,
    SensorStateResponse,
};
use esphome_api::{decode_frame, encode_frame_vec, Frame};
use prost::Message;

const FIXTURE: &str = include_str!("fixtures/plaintext_frames.tsv");

/// One captured frame: its message name, wire type id, the full frame bytes and
/// the payload bytes, parsed from a TSV row.
struct Golden {
    name: String,
    msg_type: u32,
    frame: Vec<u8>,
    payload: Vec<u8>,
}

fn goldens() -> Vec<Golden> {
    FIXTURE
        .lines()
        .filter(|line: &&str| {
            let t: &str = line.trim();
            !t.is_empty() && !t.starts_with('#')
        })
        .map(|line: &str| {
            let cols: Vec<&str> = line.split('\t').collect();
            assert!(
                cols.len() >= 4,
                "fixture row needs name<TAB>type<TAB>frame_hex<TAB>payload_hex: {line:?}"
            );
            Golden {
                name: cols[0].to_string(),
                msg_type: cols[1].parse::<u32>().expect("msg_type is a number"),
                frame: unhex(cols[2]),
                payload: unhex(cols[3]),
            }
        })
        .collect()
}

/// Decode a hex string (possibly empty) into bytes. A tiny local parser keeps
/// the crate free of a hex dependency for one fixture.
fn unhex(s: &str) -> Vec<u8> {
    let s: &str = s.trim();
    assert!(s.len().is_multiple_of(2), "hex must have an even length: {s:?}");
    (0..s.len())
        .step_by(2)
        .map(|i: usize| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex byte"))
        .collect()
}

/// Look up one captured frame by name — the encode direction re-creates the
/// message by hand, so it must target a specific known row.
fn golden(name: &str) -> Golden {
    goldens()
        .into_iter()
        .find(|g: &Golden| g.name == name)
        .unwrap_or_else(|| panic!("fixture is missing a {name} row"))
}

/// The message reconstructed here must serialise to the captured payload, and
/// framing it must reproduce the captured frame byte-for-byte.
fn assert_encodes_to_capture<M: Message>(name: &str, message: &M) {
    let g: Golden = golden(name);
    let payload: Vec<u8> = message.encode_to_vec();
    assert_eq!(
        payload, g.payload,
        "{name}: our prost payload differs from aioesphomeapi's"
    );
    let framed: Vec<u8> = encode_frame_vec(g.msg_type, &payload);
    assert_eq!(
        framed, g.frame,
        "{name}: our frame differs from the captured aioesphomeapi frame"
    );
}

#[test]
fn ping_request_matches_the_capture() {
    // The empty-payload frame: proves len(0) is one 0x00 byte, not omitted.
    assert_encodes_to_capture("PingRequest", &PingRequest {});
}

#[test]
fn hello_response_matches_the_capture() {
    assert_encodes_to_capture(
        "HelloResponse",
        &HelloResponse {
            api_version_major: 1,
            api_version_minor: 14,
            server_info: "esphome-api".to_string(),
            name: "plantmon".to_string(),
        },
    );
}

#[test]
fn sensor_state_matches_the_capture() {
    // fixed32 key + IEEE-754 float; the default `missing_state = false` must be
    // omitted, exactly as the Python client omits it.
    assert_encodes_to_capture(
        "SensorStateResponse",
        &SensorStateResponse {
            key: 0x1A2B_3C4D,
            state: 42.5,
            missing_state: false,
            device_id: 0,
        },
    );
}

#[test]
fn device_info_with_a_long_field_matches_the_capture() {
    // The >=128-byte payload that forces a two-byte length varuint.
    assert_encodes_to_capture(
        "DeviceInfoResponse",
        &DeviceInfoResponse {
            name: "plantmon".to_string(),
            esphome_version: "rust-0.1".to_string(),
            model: "M".repeat(200),
            ..Default::default()
        },
    );
}

#[test]
fn list_entities_sensor_matches_the_capture() {
    assert_encodes_to_capture(
        "ListEntitiesSensorResponse",
        &ListEntitiesSensorResponse {
            object_id: "soil_moisture".to_string(),
            key: 0x1A2B_3C4D,
            name: "Soil Moisture".to_string(),
            unit_of_measurement: "%".to_string(),
            accuracy_decimals: 0,
            device_class: "moisture".to_string(),
            ..Default::default()
        },
    );
}

#[test]
fn every_captured_frame_decodes_to_its_type_and_payload() {
    // The decode direction, over all rows at once: each captured frame yields
    // exactly the captured (type, payload), consuming the whole frame.
    for g in goldens() {
        let (frame, consumed): (Frame, usize) = decode_frame(&g.frame)
            .unwrap_or_else(|e: esphome_api::FrameError| panic!("{}: {e}", g.name))
            .unwrap_or_else(|| panic!("{}: frame decoded as incomplete", g.name));
        assert_eq!(consumed, g.frame.len(), "{}: leftover bytes after decode", g.name);
        assert_eq!(frame.msg_type, g.msg_type, "{}: wrong type", g.name);
        assert_eq!(frame.payload, g.payload, "{}: wrong payload", g.name);
    }
}
