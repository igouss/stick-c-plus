//! The native-API connection as a **pure Functional Core**.
//!
//! [`handle`] is the whole protocol: a pure function of the current
//! [`Connection`], one decoded [`Inbound`] message, and an [`Snapshot`] of the
//! device's entities, returning the next `Connection` and the [`Outbound`]
//! messages to send. No sockets, no clock, no `&mut` — the entire handshake and
//! subscription behaviour is exercisable on the host with plain values. The
//! blocking accept loop that owns a `TcpStream` and drives this core is a
//! separate firmware-infra bead; the conformance test in
//! `tests/aioesphomeapi_oracle.rs` supplies a minimal one and checks the core
//! against the **real** Home Assistant client.
//!
//! ## The flow
//! Home Assistant drives; the device answers. The real client
//! ([aioesphomeapi]) opens with `HelloRequest` and, only when a password is
//! configured, an `AuthenticationRequest`; then it issues `DeviceInfoRequest`,
//! `ListEntitiesRequest`, `SubscribeStatesRequest`, periodic `PingRequest`s and
//! finally `DisconnectRequest` — each a request this core answers. Because a
//! plaintext, password-less device is the target, the client sends no
//! authentication step; [`handle`] still answers one if it arrives, so the
//! password path is covered without being required.
//!
//! [aioesphomeapi]: https://github.com/esphome/aioesphomeapi

use prost::Message;

use crate::entity::{ListMessage, StateMessage};
use crate::frame::Frame;
use crate::{proto, API_VERSION};

/// Wire type ids of the inbound messages the core reacts to.
///
/// Kept as consts so the [`Inbound::decode`] match arms are `const` patterns;
/// `inbound_ids_match_the_registry` proves each equals its [`crate::MESSAGE_IDS`]
/// entry, so a re-pinned proto that renumbered a message trips the test rather
/// than silently misrouting.
mod msg {
    pub const HELLO_REQUEST: u32 = 1;
    pub const AUTHENTICATION_REQUEST: u32 = 3;
    pub const DISCONNECT_REQUEST: u32 = 5;
    pub const DISCONNECT_RESPONSE: u32 = 6;
    pub const PING_REQUEST: u32 = 7;
    pub const PING_RESPONSE: u32 = 8;
    pub const DEVICE_INFO_REQUEST: u32 = 9;
    pub const LIST_ENTITIES_REQUEST: u32 = 11;
    pub const SUBSCRIBE_STATES_REQUEST: u32 = 20;
}

/// Where a connection sits in its lifecycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Nothing received yet — only `HelloRequest` advances the connection.
    AwaitingHello,
    /// Handshaken: entity requests, subscription and keepalives are answered.
    Active,
    /// A disconnect was exchanged; the connection is absorbing and silent.
    Closed,
}

/// The connection's state — small and `Copy`, since [`handle`] threads it by
/// value (old in, new out) rather than mutating in place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Connection {
    phase: Phase,
    subscribed: bool,
}

impl Connection {
    /// A fresh connection, awaiting the client's `HelloRequest`.
    pub fn new() -> Self {
        Self { phase: Phase::AwaitingHello, subscribed: false }
    }

    /// The current lifecycle phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Whether the handshake has completed.
    pub fn is_active(&self) -> bool {
        self.phase == Phase::Active
    }

    /// Whether a disconnect has closed the connection.
    pub fn is_closed(&self) -> bool {
        self.phase == Phase::Closed
    }

    /// Whether the client has subscribed to state updates — the gate
    /// [`broadcast_states`] checks before pushing a device-driven reading.
    pub fn is_subscribed(&self) -> bool {
        self.subscribed
    }
}

impl Default for Connection {
    fn default() -> Self {
        Self::new()
    }
}

/// A decoded message from the client, narrowed to the ones the core acts on.
///
/// Anything else on the wire (log subscriptions, HA-state pushes, time replies)
/// is [`Inbound::Other`] and ignored by [`handle`] — a device need not answer
/// every message a rich client can send.
#[derive(Clone, PartialEq, Debug)]
pub enum Inbound {
    /// Opens the connection; carries the client's API version.
    Hello(proto::HelloRequest),
    /// The password step (password-configured devices only).
    Authenticate(proto::AuthenticationRequest),
    /// Asks for the device description.
    DeviceInfo(proto::DeviceInfoRequest),
    /// Asks for the entity list.
    ListEntities(proto::ListEntitiesRequest),
    /// Subscribes to state updates.
    SubscribeStates(proto::SubscribeStatesRequest),
    /// A keepalive ping.
    Ping(proto::PingRequest),
    /// The client's answer to one of our pings.
    Pong(proto::PingResponse),
    /// The client is closing the connection.
    Disconnect(proto::DisconnectRequest),
    /// The client acknowledged our disconnect.
    DisconnectAck(proto::DisconnectResponse),
    /// A message this core does not act on, kept by its wire id for logging.
    Other {
        /// The wire type id that was not recognised as an actionable message.
        msg_type: u32,
    },
}

impl Inbound {
    /// Decode an inbound message from its wire type id and protobuf payload.
    ///
    /// An unrecognised type is [`Inbound::Other`] (ignored, not an error); a
    /// recognised type whose payload does not parse is a `prost::DecodeError`,
    /// which the shell should treat as a protocol fault and close the stream —
    /// never silently drop.
    pub fn decode(msg_type: u32, payload: &[u8]) -> Result<Inbound, prost::DecodeError> {
        let inbound: Inbound = match msg_type {
            msg::HELLO_REQUEST => Inbound::Hello(proto::HelloRequest::decode(payload)?),
            msg::AUTHENTICATION_REQUEST => {
                Inbound::Authenticate(proto::AuthenticationRequest::decode(payload)?)
            }
            msg::DEVICE_INFO_REQUEST => {
                Inbound::DeviceInfo(proto::DeviceInfoRequest::decode(payload)?)
            }
            msg::LIST_ENTITIES_REQUEST => {
                Inbound::ListEntities(proto::ListEntitiesRequest::decode(payload)?)
            }
            msg::SUBSCRIBE_STATES_REQUEST => {
                Inbound::SubscribeStates(proto::SubscribeStatesRequest::decode(payload)?)
            }
            msg::PING_REQUEST => Inbound::Ping(proto::PingRequest::decode(payload)?),
            msg::PING_RESPONSE => Inbound::Pong(proto::PingResponse::decode(payload)?),
            msg::DISCONNECT_REQUEST => {
                Inbound::Disconnect(proto::DisconnectRequest::decode(payload)?)
            }
            msg::DISCONNECT_RESPONSE => {
                Inbound::DisconnectAck(proto::DisconnectResponse::decode(payload)?)
            }
            other => Inbound::Other { msg_type: other },
        };
        Ok(inbound)
    }

    /// Decode an inbound message straight from a [`Frame`].
    pub fn from_frame(frame: &Frame) -> Result<Inbound, prost::DecodeError> {
        Inbound::decode(frame.msg_type, &frame.payload)
    }
}

/// A message the core wants sent to the client.
///
/// [`Outbound::wire`] turns each into the `(type id, payload)` the frame codec
/// serialises. Empty-body messages (`ListDone`, `Pong`, `DisconnectAck`) carry
/// no payload, exactly as their protobuf definitions are empty.
#[derive(Clone, PartialEq, Debug)]
pub enum Outbound {
    /// Answer to `HelloRequest`.
    Hello(proto::HelloResponse),
    /// Answer to `AuthenticationRequest`.
    Authenticated(proto::AuthenticationResponse),
    /// Answer to `DeviceInfoRequest`. Boxed: `DeviceInfoResponse` is ~400 bytes
    /// of optional fields and would otherwise bloat every `Outbound` (and the
    /// `Vec<Outbound>` a handshake produces) to its size.
    DeviceInfo(Box<proto::DeviceInfoResponse>),
    /// One entity's description, part of the `ListEntities` reply.
    List(ListMessage),
    /// Terminates the `ListEntities` reply.
    ListDone,
    /// One entity's state — the initial stream or a later update.
    State(StateMessage),
    /// Answer to a `PingRequest`.
    Pong,
    /// Answer to a `DisconnectRequest`.
    DisconnectAck,
}

impl Outbound {
    /// The `(message type id, protobuf payload)` the frame codec sends.
    pub fn wire(&self) -> (u32, Vec<u8>) {
        match self {
            Outbound::Hello(m) => (id("HelloResponse"), m.encode_to_vec()),
            Outbound::Authenticated(m) => (id("AuthenticationResponse"), m.encode_to_vec()),
            Outbound::DeviceInfo(m) => (id("DeviceInfoResponse"), m.encode_to_vec()),
            Outbound::List(list) => list.wire(),
            Outbound::ListDone => (id("ListEntitiesDoneResponse"), Vec::new()),
            Outbound::State(state) => state.wire(),
            Outbound::Pong => (id("PingResponse"), Vec::new()),
            Outbound::DisconnectAck => (id("DisconnectResponse"), Vec::new()),
        }
    }
}

/// Resolve a prost type name to its wire id via the authoritative registry (see
/// [`crate::entity`]'s identical helper for the invariant this `expect` guards).
fn id(type_name: &'static str) -> u32 {
    crate::message_id(type_name)
        .unwrap_or_else(|| panic!("{type_name} is not in MESSAGE_IDS — re-pin drift?"))
}

/// A read-only view of the device the core answers requests from: its identity
/// and its entities' current descriptions and states.
///
/// Borrowed, so the shell can hand [`handle`] a fresh snapshot of a mutable
/// [`crate::Registry`] each turn without the core ever touching the entities.
pub struct Snapshot<'a> {
    /// The device description returned for `DeviceInfoRequest`; its `name` and
    /// `esphome_version` also fill the `HelloResponse`.
    pub device_info: &'a proto::DeviceInfoResponse,
    /// Every entity's `ListEntities*` description, in advertise order.
    pub list: &'a [ListMessage],
    /// Every entity's current state, for the initial subscription stream.
    pub states: &'a [StateMessage],
}

impl Snapshot<'_> {
    /// The `HelloResponse` for this device: our vendored API version plus the
    /// device's name and version, so a single source (`device_info`) drives
    /// both the hello and the device-info replies.
    fn hello_response(&self) -> proto::HelloResponse {
        proto::HelloResponse {
            api_version_major: API_VERSION.0,
            api_version_minor: API_VERSION.1,
            server_info: self.device_info.esphome_version.clone(),
            name: self.device_info.name.clone(),
        }
    }

    /// This device's entity states as `Outbound::State` messages, in order.
    fn state_stream(&self) -> Vec<Outbound> {
        self.states.iter().cloned().map(Outbound::State).collect()
    }
}

/// Advance the connection by one inbound message, purely.
///
/// Returns the next [`Connection`] and the messages to send, in order. Total: a
/// [`Closed`](Phase::Closed) connection absorbs everything to an empty reply,
/// and the entity request/response trio is answered only once
/// [`Active`](Phase::Active) — a request before `HelloRequest` (which the real
/// client never sends, but a fuzzer will) is ignored, leaving the state
/// unchanged.
pub fn handle(
    conn: Connection,
    inbound: &Inbound,
    snapshot: &Snapshot<'_>,
) -> (Connection, Vec<Outbound>) {
    if conn.phase == Phase::Closed {
        return (conn, Vec::new());
    }

    match inbound {
        Inbound::Hello(_) => {
            let out: Vec<Outbound> = vec![Outbound::Hello(snapshot.hello_response())];
            (Connection { phase: Phase::Active, ..conn }, out)
        }

        // No password is configured, so authentication always succeeds. Valid
        // in any open phase — the client sends it right after Hello.
        Inbound::Authenticate(_) => {
            let ok: proto::AuthenticationResponse =
                proto::AuthenticationResponse { invalid_password: false };
            (conn, vec![Outbound::Authenticated(ok)])
        }

        Inbound::Ping(_) => (conn, vec![Outbound::Pong]),

        // The client answered our keepalive; nothing to send.
        Inbound::Pong(_) => (conn, Vec::new()),

        Inbound::Disconnect(_) => {
            (Connection { phase: Phase::Closed, ..conn }, vec![Outbound::DisconnectAck])
        }

        // The client acknowledged a disconnect we initiated; close silently.
        Inbound::DisconnectAck(_) => (Connection { phase: Phase::Closed, ..conn }, Vec::new()),

        Inbound::DeviceInfo(_) => {
            let out: Vec<Outbound> = if conn.phase == Phase::Active {
                vec![Outbound::DeviceInfo(Box::new(snapshot.device_info.clone()))]
            } else {
                Vec::new()
            };
            (conn, out)
        }

        Inbound::ListEntities(_) => {
            let out: Vec<Outbound> = if conn.phase == Phase::Active {
                let mut msgs: Vec<Outbound> =
                    snapshot.list.iter().cloned().map(Outbound::List).collect();
                msgs.push(Outbound::ListDone);
                msgs
            } else {
                Vec::new()
            };
            (conn, out)
        }

        Inbound::SubscribeStates(_) => {
            if conn.phase == Phase::Active {
                (Connection { subscribed: true, ..conn }, snapshot.state_stream())
            } else {
                (conn, Vec::new())
            }
        }

        Inbound::Other { .. } => (conn, Vec::new()),
    }
}

/// The state packets to push for a **device-driven** change — a new sensor
/// reading the sampler produced, not a reply to any inbound message.
///
/// Pure and gated: nothing is emitted unless the connection is
/// [`Active`](Phase::Active) *and* the client has subscribed. The initial stream
/// on `SubscribeStates` is [`handle`]'s job; this covers every reading after it,
/// so an unsubscribed or half-open connection is never sent unsolicited state.
pub fn broadcast_states(conn: &Connection, states: &[StateMessage]) -> Vec<Outbound> {
    if conn.phase != Phase::Active || !conn.subscribed {
        return Vec::new();
    }
    states.iter().cloned().map(Outbound::State).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A device-info fixture the snapshot answers from.
    fn device_info() -> proto::DeviceInfoResponse {
        proto::DeviceInfoResponse {
            name: "plantmon".to_string(),
            esphome_version: "rust-0.1".to_string(),
            mac_address: "AA:BB:CC:DD:EE:FF".to_string(),
            model: "M5StickC Plus".to_string(),
            ..Default::default()
        }
    }

    /// A one-sensor list, its matching state, and the device info — the plant
    /// monitor's minimal shape.
    fn one_sensor() -> (proto::DeviceInfoResponse, Vec<ListMessage>, Vec<StateMessage>) {
        let list: Vec<ListMessage> = vec![ListMessage::Sensor(proto::ListEntitiesSensorResponse {
            object_id: "soil_moisture".to_string(),
            key: 0x1A2B_3C4D,
            name: "Soil Moisture".to_string(),
            ..Default::default()
        })];
        let states: Vec<StateMessage> = vec![StateMessage::Sensor(proto::SensorStateResponse {
            key: 0x1A2B_3C4D,
            state: 42.5,
            missing_state: false,
            device_id: 0,
        })];
        (device_info(), list, states)
    }

    fn snapshot<'a>(
        info: &'a proto::DeviceInfoResponse,
        list: &'a [ListMessage],
        states: &'a [StateMessage],
    ) -> Snapshot<'a> {
        Snapshot { device_info: info, list, states }
    }

    #[test]
    fn hello_is_answered_and_activates_the_connection() {
        let info: proto::DeviceInfoResponse = device_info();
        let snap: Snapshot = snapshot(&info, &[], &[]);
        let (conn, out): (Connection, Vec<Outbound>) = handle(
            Connection::new(),
            &Inbound::Hello(proto::HelloRequest::default()),
            &snap,
        );
        assert!(conn.is_active());
        assert_eq!(
            out,
            vec![Outbound::Hello(proto::HelloResponse {
                api_version_major: 1,
                api_version_minor: 14,
                server_info: "rust-0.1".to_string(),
                name: "plantmon".to_string(),
            })]
        );
    }

    #[test]
    fn authentication_without_a_password_succeeds() {
        let info: proto::DeviceInfoResponse = device_info();
        let snap: Snapshot = snapshot(&info, &[], &[]);
        let (_, out): (Connection, Vec<Outbound>) = handle(
            Connection::new(),
            &Inbound::Authenticate(proto::AuthenticationRequest::default()),
            &snap,
        );
        assert_eq!(
            out,
            vec![Outbound::Authenticated(proto::AuthenticationResponse { invalid_password: false })]
        );
    }

    #[test]
    fn device_info_is_answered_once_active() {
        let info: proto::DeviceInfoResponse = device_info();
        let snap: Snapshot = snapshot(&info, &[], &[]);
        let active: Connection = handle(Connection::new(), &Inbound::Hello(Default::default()), &snap).0;
        let (_, out): (Connection, Vec<Outbound>) =
            handle(active, &Inbound::DeviceInfo(Default::default()), &snap);
        assert_eq!(out, vec![Outbound::DeviceInfo(Box::new(info.clone()))]);
    }

    #[test]
    fn list_entities_streams_each_entity_then_done() {
        let (info, list, states) = one_sensor();
        let snap: Snapshot = snapshot(&info, &list, &states);
        let active: Connection = handle(Connection::new(), &Inbound::Hello(Default::default()), &snap).0;
        let (_, out): (Connection, Vec<Outbound>) =
            handle(active, &Inbound::ListEntities(Default::default()), &snap);
        assert_eq!(out, vec![Outbound::List(list[0].clone()), Outbound::ListDone]);
    }

    #[test]
    fn list_entities_with_no_entities_is_just_done() {
        let info: proto::DeviceInfoResponse = device_info();
        let snap: Snapshot = snapshot(&info, &[], &[]);
        let active: Connection = handle(Connection::new(), &Inbound::Hello(Default::default()), &snap).0;
        let (_, out): (Connection, Vec<Outbound>) =
            handle(active, &Inbound::ListEntities(Default::default()), &snap);
        assert_eq!(out, vec![Outbound::ListDone]);
    }

    #[test]
    fn list_entities_with_many_streams_each_in_order() {
        let info: proto::DeviceInfoResponse = device_info();
        let list: Vec<ListMessage> = vec![
            ListMessage::Sensor(proto::ListEntitiesSensorResponse {
                object_id: "a".to_string(),
                key: 1,
                ..Default::default()
            }),
            ListMessage::Sensor(proto::ListEntitiesSensorResponse {
                object_id: "b".to_string(),
                key: 2,
                ..Default::default()
            }),
        ];
        let snap: Snapshot = snapshot(&info, &list, &[]);
        let active: Connection = handle(Connection::new(), &Inbound::Hello(Default::default()), &snap).0;
        let (_, out): (Connection, Vec<Outbound>) =
            handle(active, &Inbound::ListEntities(Default::default()), &snap);
        assert_eq!(
            out,
            vec![
                Outbound::List(list[0].clone()),
                Outbound::List(list[1].clone()),
                Outbound::ListDone,
            ]
        );
    }

    #[test]
    fn subscribe_streams_initial_states_and_sets_subscribed() {
        let (info, list, states) = one_sensor();
        let snap: Snapshot = snapshot(&info, &list, &states);
        let active: Connection = handle(Connection::new(), &Inbound::Hello(Default::default()), &snap).0;
        let (conn, out): (Connection, Vec<Outbound>) =
            handle(active, &Inbound::SubscribeStates(Default::default()), &snap);
        assert!(conn.is_subscribed());
        assert_eq!(out, vec![Outbound::State(states[0].clone())]);
    }

    #[test]
    fn ping_is_ponged_and_pong_is_silent() {
        let info: proto::DeviceInfoResponse = device_info();
        let snap: Snapshot = snapshot(&info, &[], &[]);
        let active: Connection = handle(Connection::new(), &Inbound::Hello(Default::default()), &snap).0;
        let (_, pong): (Connection, Vec<Outbound>) =
            handle(active, &Inbound::Ping(Default::default()), &snap);
        assert_eq!(pong, vec![Outbound::Pong]);
        let (_, nothing): (Connection, Vec<Outbound>) =
            handle(active, &Inbound::Pong(Default::default()), &snap);
        assert!(nothing.is_empty());
    }

    #[test]
    fn disconnect_is_acked_and_closes() {
        let info: proto::DeviceInfoResponse = device_info();
        let snap: Snapshot = snapshot(&info, &[], &[]);
        let active: Connection = handle(Connection::new(), &Inbound::Hello(Default::default()), &snap).0;
        let (closed, out): (Connection, Vec<Outbound>) =
            handle(active, &Inbound::Disconnect(Default::default()), &snap);
        assert!(closed.is_closed());
        assert_eq!(out, vec![Outbound::DisconnectAck]);
        // A closed connection absorbs everything afterwards.
        let (still_closed, silence): (Connection, Vec<Outbound>) =
            handle(closed, &Inbound::Ping(Default::default()), &snap);
        assert!(still_closed.is_closed());
        assert!(silence.is_empty());
    }

    #[test]
    fn a_request_before_hello_is_ignored() {
        // The real client never does this, but the core must not answer entity
        // requests from an un-handshaked peer, nor advance its state.
        let (info, list, states) = one_sensor();
        let snap: Snapshot = snapshot(&info, &list, &states);
        let fresh: Connection = Connection::new();
        let (conn, out): (Connection, Vec<Outbound>) =
            handle(fresh, &Inbound::SubscribeStates(Default::default()), &snap);
        assert_eq!(conn, fresh, "state must be unchanged");
        assert!(!conn.is_subscribed());
        assert!(out.is_empty());
    }

    #[test]
    fn an_unknown_message_is_ignored() {
        let info: proto::DeviceInfoResponse = device_info();
        let snap: Snapshot = snapshot(&info, &[], &[]);
        let active: Connection = handle(Connection::new(), &Inbound::Hello(Default::default()), &snap).0;
        let (conn, out): (Connection, Vec<Outbound>) =
            handle(active, &Inbound::Other { msg_type: 999 }, &snap);
        assert_eq!(conn, active);
        assert!(out.is_empty());
    }

    #[test]
    fn a_full_handshake_produces_the_expected_sequence() {
        // Hello -> DeviceInfo -> ListEntities -> SubscribeStates, folded, is the
        // exact outbound conversation HA sees on adoption.
        let (info, list, states) = one_sensor();
        let snap: Snapshot = snapshot(&info, &list, &states);
        let script: [Inbound; 4] = [
            Inbound::Hello(Default::default()),
            Inbound::DeviceInfo(Default::default()),
            Inbound::ListEntities(Default::default()),
            Inbound::SubscribeStates(Default::default()),
        ];
        let mut conn: Connection = Connection::new();
        let mut transcript: Vec<Outbound> = Vec::new();
        for inbound in &script {
            let (next, out): (Connection, Vec<Outbound>) = handle(conn, inbound, &snap);
            conn = next;
            transcript.extend(out);
        }
        assert!(conn.is_active() && conn.is_subscribed());
        assert_eq!(
            transcript,
            vec![
                Outbound::Hello(snap.hello_response()),
                Outbound::DeviceInfo(Box::new(info.clone())),
                Outbound::List(list[0].clone()),
                Outbound::ListDone,
                Outbound::State(states[0].clone()),
            ]
        );
    }

    #[test]
    fn broadcast_is_gated_on_an_active_subscription() {
        let (info, list, states) = one_sensor();
        let snap: Snapshot = snapshot(&info, &list, &states);

        // Un-subscribed active connection: no unsolicited state.
        let active: Connection = handle(Connection::new(), &Inbound::Hello(Default::default()), &snap).0;
        assert!(broadcast_states(&active, &states).is_empty());

        // After subscribing, a new reading is pushed.
        let subscribed: Connection =
            handle(active, &Inbound::SubscribeStates(Default::default()), &snap).0;
        assert_eq!(
            broadcast_states(&subscribed, &states),
            vec![Outbound::State(states[0].clone())]
        );
    }

    #[test]
    fn inbound_decode_maps_types_and_flags_bad_payloads() {
        // A known id with an empty (valid) payload decodes to its variant.
        assert_eq!(
            Inbound::decode(msg::HELLO_REQUEST, &proto::HelloRequest::default().encode_to_vec()),
            Ok(Inbound::Hello(proto::HelloRequest::default()))
        );
        // An unknown id is Other, not an error.
        assert_eq!(Inbound::decode(9999, &[]), Ok(Inbound::Other { msg_type: 9999 }));
        // A known id with a truncated/garbage payload is a decode error, never a
        // silent drop.
        assert!(Inbound::decode(msg::HELLO_REQUEST, &[0xFF, 0xFF, 0xFF]).is_err());
    }

    #[test]
    fn inbound_ids_match_the_registry() {
        // Every routing const equals the authoritative MESSAGE_IDS entry, so a
        // renumbered proto trips here instead of misrouting a message.
        let pairs: [(u32, &str); 9] = [
            (msg::HELLO_REQUEST, "HelloRequest"),
            (msg::AUTHENTICATION_REQUEST, "AuthenticationRequest"),
            (msg::DISCONNECT_REQUEST, "DisconnectRequest"),
            (msg::DISCONNECT_RESPONSE, "DisconnectResponse"),
            (msg::PING_REQUEST, "PingRequest"),
            (msg::PING_RESPONSE, "PingResponse"),
            (msg::DEVICE_INFO_REQUEST, "DeviceInfoRequest"),
            (msg::LIST_ENTITIES_REQUEST, "ListEntitiesRequest"),
            (msg::SUBSCRIBE_STATES_REQUEST, "SubscribeStatesRequest"),
        ];
        for (id_const, name) in pairs {
            assert_eq!(Some(id_const), crate::message_id(name), "id for {name}");
        }
    }
}
