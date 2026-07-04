//! # esphome-api
//!
//! The ESPHome native-API wire vocabulary: the prost message types and the
//! message-type-id registry that Home Assistant's client speaks. Vendored from
//! [aioesphomeapi](https://github.com/esphome/aioesphomeapi) (MIT) at a pinned
//! commit — never from `esphome/esphome`, which is GPLv3. See `PROVENANCE.md`.
//!
//! ## Hexagon
//! This crate is adapter-side **wire vocabulary and logic**, not the business
//! domain: it is the on-the-wire representation HA expects, so it lives at the
//! edge, and the framework-free `plant-core`/`led-core` domains never depend on
//! it. What it *does* hold — the frame codec, the entity model, the connection
//! FSM — is a **functional core**: pure, effect-free, host-testable with plain
//! values. Concrete effects (a `TcpStream`, an accept loop) are injected by the
//! shell: the byte-stream pumps in [`frame_io`] are generic over `Read`/`Write`,
//! so the socket comes from the server-host bead or the conformance test, never
//! from here. That FC/IS property is enforced by `effect-audit`, which is why
//! the crate carries `role = "domain"` in `Cargo.toml` (its functional-core
//! marker — distinct from the DDD sense of "domain" above).
//!
//! ## Scope
//! The wire vocabulary (types + id registry), the blocking `std::io` frame
//! codec ([`frame`] + [`frame_io`], qhw.17) and the generic entity model
//! ([`entity`], qhw.18). The pure connection FSM (qhw.19) builds on these.
//! There is no async runtime here: `prost` is the only runtime dependency.

// Exactly one api-version feature must select a vendored proto snapshot. Today
// only `api-1-14` exists; when a second version is vendored this guard grows to
// reject enabling two at once.
#[cfg(not(feature = "api-1-14"))]
compile_error!(
    "enable exactly one esphome-api version feature (currently only `api-1-14`); \
     do not build with --no-default-features unless you select one"
);

/// The prost message types, `@generated` from `proto/api.proto`.
///
/// One struct per ESPHome native-API message (`HelloRequest`, `DeviceInfoResponse`,
/// `ListEntitiesSensorResponse`, …), plus the enums and oneofs they reference.
/// The generated file is committed, so no `protoc` runs at build time.
pub mod proto {
    // Generated code: hold it to prost's conventions, not ours.
    #![allow(clippy::all)]
    #![allow(missing_docs)]
    include!("generated/api.rs");
}

// The id registry is a small committed table; keep it in a private module and
// re-export the constant so it reads as `esphome_api::MESSAGE_IDS`.
mod ids {
    include!("generated/ids.rs");
}

pub mod connection;
pub mod entity;
pub mod frame;
pub mod frame_io;

pub use connection::{broadcast_states, handle, Connection, Inbound, Outbound, Phase, Snapshot};
pub use entity::{
    object_id_key, ApiEntity, CommandMessage, ListMessage, Registry, SensorConfig, SensorEntity,
    StateMessage,
};
pub use frame::{
    decode_frame, encode_frame, encode_frame_vec, Frame, FrameError, MAX_FRAME_SIZE,
};
pub use frame_io::{FrameReader, FrameWriter};
pub use ids::MESSAGE_IDS;
pub use proto::*;

/// The ESPHome native-API version the vendored proto snapshot targets — the
/// `(major, minor)` the HA client negotiates against this build (`1.14`).
pub const API_VERSION: (u32, u32) = (1, 14);

/// The wire type id for a message, by its prost type name (e.g. `"HelloRequest"`
/// → `1`). `None` if no message carries that name.
///
/// Linear over [`MESSAGE_IDS`]; the table is small and this is not a hot path
/// (the codec keys on the numeric id, which [`message_name`] resolves).
pub fn message_id(name: &str) -> Option<u32> {
    MESSAGE_IDS
        .iter()
        .find(|(_, n): &&(u32, &str)| *n == name)
        .map(|(id, _): &(u32, &str)| *id)
}

/// The message name for a wire type id (e.g. `1` → `"HelloRequest"`). `None` if
/// the id is not one this proto snapshot defines.
///
/// [`MESSAGE_IDS`] is sorted by id, so this is a binary search.
pub fn message_name(id: u32) -> Option<&'static str> {
    MESSAGE_IDS
        .binary_search_by_key(&id, |(mid, _): &(u32, &str)| *mid)
        .ok()
        .map(|idx: usize| MESSAGE_IDS[idx].1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    #[test]
    fn the_registry_covers_the_pinned_api() {
        // A canary on the vendored snapshot: aioesphomeapi 1.14 defines 148
        // messages. A proto bump that adds or drops one trips this and forces a
        // conscious re-pin.
        assert_eq!(MESSAGE_IDS.len(), 148);
    }

    #[test]
    fn known_ids_round_trip_through_both_lookups() {
        assert_eq!(message_id("HelloRequest"), Some(1));
        assert_eq!(message_name(1), Some("HelloRequest"));
        assert_eq!(message_id("ListEntitiesSensorResponse"), Some(16));
        assert_eq!(message_name(16), Some("ListEntitiesSensorResponse"));
    }

    #[test]
    fn unknown_lookups_are_none() {
        assert_eq!(message_id("NoSuchMessage"), None);
        assert_eq!(message_name(0), None);
        assert_eq!(message_name(9999), None);
    }

    #[test]
    fn the_prost_types_encode_and_decode() {
        // Proves the generated types are real prost messages, not just names.
        let hello: HelloRequest = HelloRequest {
            client_info: "test".to_string(),
            api_version_major: 1,
            api_version_minor: 14,
        };
        let bytes: Vec<u8> = hello.encode_to_vec();
        let back: HelloRequest = HelloRequest::decode(bytes.as_slice()).unwrap();
        assert_eq!(back, hello);
    }
}
