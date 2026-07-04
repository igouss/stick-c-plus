//! The generic **entity model**: the abstraction Home Assistant enumerates and
//! subscribes to over the native API.
//!
//! ## The shape
//! An [`ApiEntity`] is anything that can name itself ([`ApiEntity::object_id`] /
//! [`ApiEntity::key`]), describe itself for the entity list
//! ([`ApiEntity::list_message`]), report its current value
//! ([`ApiEntity::state_message`]) and react to a command
//! ([`ApiEntity::handle_command`]). A [`Registry`] holds a set of boxed entities
//! and enumerates their list/state messages for the connection FSM.
//!
//! The wire messages are carried by three enums — [`ListMessage`],
//! [`StateMessage`], [`CommandMessage`] — with one variant per ESPHome entity
//! type. Only [`SensorEntity`] is implemented today; the other variants exist so
//! a new entity type is *added*, never a rewrite (see [`ListMessage`] for the
//! recipe). Non-`Sensor` variants are **UNVERIFIED placeholders**: their wire id
//! is checked against [`crate::MESSAGE_IDS`], but no entity produces one yet and
//! none has been driven against Home Assistant.
//!
//! ## Keys
//! A native-API `key` is a `fixed32` the *device* assigns; HA treats whatever
//! the device advertises in the entity list as that entity's identity and
//! matches state packets to it. So the only real requirements are that a key is
//! **stable across reboots** (or HA churns the entity on every restart) and
//! unique within the device. [`object_id_key`] derives it as a **seedless
//! FNV-1a** hash of the object_id — deterministic and process-independent,
//! unlike `std`'s `DefaultHasher`/`RandomState`, whose per-process seed would
//! hand HA a different key after every boot.

use prost::Message;

use crate::proto;

/// FNV-1a 32-bit offset basis (`2166136261`).
const FNV_OFFSET_BASIS: u32 = 0x811c_9dc5;
/// FNV-1a 32-bit prime (`16777619`).
const FNV_PRIME: u32 = 0x0100_0193;

/// The native-API entity `key` for an `object_id`: a seedless FNV-1a hash of its
/// UTF-8 bytes.
///
/// FNV-1a is *xor-then-multiply*, seeded only by the fixed offset basis, so it
/// is fully deterministic: a freshly-booted device recomputes the identical key
/// for the same object_id, and HA never churns the entity. This is deliberately
/// **not** `std::hash::DefaultHasher` — that is `RandomState`-seeded per process
/// and would produce a different key on every boot.
///
/// `wrapping_mul` makes the overflow explicit and total; there is no panic path.
pub fn object_id_key(object_id: &str) -> u32 {
    let mut hash: u32 = FNV_OFFSET_BASIS;
    for &byte in object_id.as_bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// A `ListEntities*Response` — the description HA reads to build the entity, one
/// variant per supported type.
///
/// ### Adding a new entity type
/// The whole recipe, no rewrite:
/// 1. add a variant here and to [`StateMessage`] (and [`CommandMessage`] if the
///    type is controllable) wrapping the prost message;
/// 2. add the match arm in [`ListMessage::type_name`] / [`Self::encode_payload`]
///    (the compiler's exhaustiveness check lists exactly what to fill in);
/// 3. implement [`ApiEntity`] for the new entity struct.
///
/// The wire id comes from [`crate::MESSAGE_IDS`], so there is nothing to keep in
/// sync by hand.
#[derive(Clone, PartialEq, Debug)]
pub enum ListMessage {
    /// A sensor's description ([`crate::SensorEntity`]).
    Sensor(proto::ListEntitiesSensorResponse),
    /// UNVERIFIED placeholder — no entity produces this yet.
    BinarySensor(proto::ListEntitiesBinarySensorResponse),
    /// UNVERIFIED placeholder — no entity produces this yet.
    Switch(proto::ListEntitiesSwitchResponse),
    /// A light's description ([`crate::LightEntity`]).
    Light(proto::ListEntitiesLightResponse),
    /// UNVERIFIED placeholder — no entity produces this yet.
    Number(proto::ListEntitiesNumberResponse),
    /// UNVERIFIED placeholder — no entity produces this yet.
    Select(proto::ListEntitiesSelectResponse),
    /// UNVERIFIED placeholder — no entity produces this yet.
    Cover(proto::ListEntitiesCoverResponse),
}

impl ListMessage {
    /// The prost type name of the wrapped message — the key into
    /// [`crate::MESSAGE_IDS`].
    fn type_name(&self) -> &'static str {
        match self {
            ListMessage::Sensor(_) => "ListEntitiesSensorResponse",
            ListMessage::BinarySensor(_) => "ListEntitiesBinarySensorResponse",
            ListMessage::Switch(_) => "ListEntitiesSwitchResponse",
            ListMessage::Light(_) => "ListEntitiesLightResponse",
            ListMessage::Number(_) => "ListEntitiesNumberResponse",
            ListMessage::Select(_) => "ListEntitiesSelectResponse",
            ListMessage::Cover(_) => "ListEntitiesCoverResponse",
        }
    }

    fn encode_payload(&self) -> Vec<u8> {
        match self {
            ListMessage::Sensor(m) => m.encode_to_vec(),
            ListMessage::BinarySensor(m) => m.encode_to_vec(),
            ListMessage::Switch(m) => m.encode_to_vec(),
            ListMessage::Light(m) => m.encode_to_vec(),
            ListMessage::Number(m) => m.encode_to_vec(),
            ListMessage::Select(m) => m.encode_to_vec(),
            ListMessage::Cover(m) => m.encode_to_vec(),
        }
    }

    /// The `(message type id, protobuf payload)` pair the frame codec sends.
    pub fn wire(&self) -> (u32, Vec<u8>) {
        (message_id_of(self.type_name()), self.encode_payload())
    }
}

/// A `*StateResponse` — an entity's current value, one variant per supported
/// type. See [`ListMessage`] for the add-a-type recipe.
#[derive(Clone, PartialEq, Debug)]
pub enum StateMessage {
    /// A sensor's current reading ([`crate::SensorEntity`]).
    Sensor(proto::SensorStateResponse),
    /// UNVERIFIED placeholder — no entity produces this yet.
    BinarySensor(proto::BinarySensorStateResponse),
    /// UNVERIFIED placeholder — no entity produces this yet.
    Switch(proto::SwitchStateResponse),
    /// A light's current state ([`crate::LightEntity`]).
    Light(proto::LightStateResponse),
    /// UNVERIFIED placeholder — no entity produces this yet.
    Number(proto::NumberStateResponse),
    /// UNVERIFIED placeholder — no entity produces this yet.
    Select(proto::SelectStateResponse),
    /// UNVERIFIED placeholder — no entity produces this yet.
    Cover(proto::CoverStateResponse),
}

impl StateMessage {
    fn type_name(&self) -> &'static str {
        match self {
            StateMessage::Sensor(_) => "SensorStateResponse",
            StateMessage::BinarySensor(_) => "BinarySensorStateResponse",
            StateMessage::Switch(_) => "SwitchStateResponse",
            StateMessage::Light(_) => "LightStateResponse",
            StateMessage::Number(_) => "NumberStateResponse",
            StateMessage::Select(_) => "SelectStateResponse",
            StateMessage::Cover(_) => "CoverStateResponse",
        }
    }

    fn encode_payload(&self) -> Vec<u8> {
        match self {
            StateMessage::Sensor(m) => m.encode_to_vec(),
            StateMessage::BinarySensor(m) => m.encode_to_vec(),
            StateMessage::Switch(m) => m.encode_to_vec(),
            StateMessage::Light(m) => m.encode_to_vec(),
            StateMessage::Number(m) => m.encode_to_vec(),
            StateMessage::Select(m) => m.encode_to_vec(),
            StateMessage::Cover(m) => m.encode_to_vec(),
        }
    }

    /// The key of the entity this state belongs to — how HA matches a state
    /// packet to an entity from the list.
    pub fn key(&self) -> u32 {
        match self {
            StateMessage::Sensor(m) => m.key,
            StateMessage::BinarySensor(m) => m.key,
            StateMessage::Switch(m) => m.key,
            StateMessage::Light(m) => m.key,
            StateMessage::Number(m) => m.key,
            StateMessage::Select(m) => m.key,
            StateMessage::Cover(m) => m.key,
        }
    }

    /// The `(message type id, protobuf payload)` pair the frame codec sends.
    pub fn wire(&self) -> (u32, Vec<u8>) {
        (message_id_of(self.type_name()), self.encode_payload())
    }
}

/// A `*CommandRequest` from HA — one variant per **controllable** type.
///
/// Read-only types (Sensor, BinarySensor) have no command message and so no
/// variant here; a command is always routed to its target entity by
/// [`Self::key`].
#[derive(Clone, PartialEq, Debug)]
pub enum CommandMessage {
    /// UNVERIFIED placeholder — no entity handles this yet.
    Switch(proto::SwitchCommandRequest),
    /// A light command, handled by [`crate::LightEntity`].
    Light(proto::LightCommandRequest),
    /// UNVERIFIED placeholder — no entity handles this yet.
    Number(proto::NumberCommandRequest),
    /// UNVERIFIED placeholder — no entity handles this yet.
    Select(proto::SelectCommandRequest),
    /// UNVERIFIED placeholder — no entity handles this yet.
    Cover(proto::CoverCommandRequest),
}

impl CommandMessage {
    /// The key of the entity this command targets.
    pub fn key(&self) -> u32 {
        match self {
            CommandMessage::Switch(m) => m.key,
            CommandMessage::Light(m) => m.key,
            CommandMessage::Number(m) => m.key,
            CommandMessage::Select(m) => m.key,
            CommandMessage::Cover(m) => m.key,
        }
    }
}

/// Resolve a prost message type name to its wire id via the authoritative
/// [`crate::MESSAGE_IDS`] table.
///
/// Every name passed here is a compile-time constant that the `golden_ids` test
/// proves is present in the table, so the `expect` guards a genuine invariant
/// (a typo, or a message dropped from a re-pinned proto) rather than a runtime
/// input. `entity_wire_ids_match_the_registry` re-checks it for every variant.
fn message_id_of(type_name: &'static str) -> u32 {
    crate::message_id(type_name)
        .unwrap_or_else(|| panic!("{type_name} is not in MESSAGE_IDS — re-pin drift?"))
}

/// The behaviour every entity type provides to the connection FSM.
///
/// Object-safe on purpose: a [`Registry`] holds `Box<dyn ApiEntity>`, so a
/// device can mix sensor, switch and light entities in one collection.
pub trait ApiEntity {
    /// The entity's native-API key (see [`object_id_key`]).
    fn key(&self) -> u32;

    /// The entity's object_id — its stable, human-meaningful identifier.
    fn object_id(&self) -> &str;

    /// The description HA reads during `ListEntities`.
    fn list_message(&self) -> ListMessage;

    /// The entity's current value, for the initial state stream and updates.
    fn state_message(&self) -> StateMessage;

    /// Apply a command from HA, returning the new state to broadcast if the
    /// command changed anything observable (`None` for a read-only entity or a
    /// no-op command).
    fn handle_command(&mut self, command: &CommandMessage) -> Option<StateMessage>;
}

/// The immutable configuration of a [`SensorEntity`] — everything HA needs to
/// render it, minus the live value.
#[derive(Clone, Debug)]
pub struct SensorConfig {
    /// The stable identifier; its FNV-1a hash becomes the entity key.
    pub object_id: String,
    /// The friendly name shown in HA.
    pub name: String,
    /// e.g. `"%"`, `"°C"`. Empty for a unitless sensor.
    pub unit_of_measurement: String,
    /// Digits after the decimal point HA should display.
    pub accuracy_decimals: i32,
    /// e.g. `"moisture"`, `"temperature"`. Empty for none.
    pub device_class: String,
    /// How HA aggregates history — measurement, total, etc.
    pub state_class: proto::SensorStateClass,
}

/// A read-only sensor entity: a numeric value HA graphs over time.
///
/// The state is `Option<f32>`: `None` means "no valid reading yet", which maps
/// to the wire's `missing_state = true` — HA shows the entity as unavailable
/// rather than as `0.0`.
pub struct SensorEntity {
    key: u32,
    config: SensorConfig,
    state: Option<f32>,
}

impl SensorEntity {
    /// Build a sensor from its config, deriving the key from the object_id. The
    /// entity starts with no state (`missing_state`) until [`Self::set_state`].
    pub fn new(config: SensorConfig) -> Self {
        Self {
            key: object_id_key(&config.object_id),
            config,
            state: None,
        }
    }

    /// Record a new reading. The next [`ApiEntity::state_message`] reports it.
    pub fn set_state(&mut self, value: f32) {
        self.state = Some(value);
    }

    /// Drop the current reading — HA will see the sensor as unavailable
    /// (`missing_state`).
    pub fn clear_state(&mut self) {
        self.state = None;
    }

    /// The current reading, or `None` if none has been recorded.
    pub fn state(&self) -> Option<f32> {
        self.state
    }
}

impl ApiEntity for SensorEntity {
    fn key(&self) -> u32 {
        self.key
    }

    fn object_id(&self) -> &str {
        &self.config.object_id
    }

    fn list_message(&self) -> ListMessage {
        ListMessage::Sensor(proto::ListEntitiesSensorResponse {
            object_id: self.config.object_id.clone(),
            key: self.key,
            name: self.config.name.clone(),
            unit_of_measurement: self.config.unit_of_measurement.clone(),
            accuracy_decimals: self.config.accuracy_decimals,
            device_class: self.config.device_class.clone(),
            state_class: self.config.state_class as i32,
            ..Default::default()
        })
    }

    fn state_message(&self) -> StateMessage {
        StateMessage::Sensor(proto::SensorStateResponse {
            key: self.key,
            state: self.state.unwrap_or(0.0),
            missing_state: self.state.is_none(),
            device_id: 0,
        })
    }

    fn handle_command(&mut self, _command: &CommandMessage) -> Option<StateMessage> {
        // A sensor is read-only: it has no CommandMessage variant, and command
        // routing is by key, so one is never actually delivered here.
        None
    }
}

/// A device's set of entities, in registration order.
///
/// The FSM enumerates [`Self::list_messages`] for `ListEntities` and
/// [`Self::state_messages`] for the initial `SubscribeStates` stream, and routes
/// an inbound command through [`Self::apply_command`].
#[derive(Default)]
pub struct Registry {
    entities: Vec<Box<dyn ApiEntity>>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an entity. Registration order is the enumeration order HA sees.
    pub fn register(&mut self, entity: Box<dyn ApiEntity>) {
        self.entities.push(entity);
    }

    /// How many entities are registered.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether no entity is registered.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Every entity's `ListEntities*` description, in registration order.
    pub fn list_messages(&self) -> Vec<ListMessage> {
        let mut out: Vec<ListMessage> = Vec::with_capacity(self.entities.len());
        for entity in &self.entities {
            out.push(entity.list_message());
        }
        out
    }

    /// Every entity's current `*StateResponse`, in registration order.
    pub fn state_messages(&self) -> Vec<StateMessage> {
        let mut out: Vec<StateMessage> = Vec::with_capacity(self.entities.len());
        for entity in &self.entities {
            out.push(entity.state_message());
        }
        out
    }

    /// The entity with `key`, if any, for command routing.
    pub fn entity_by_key(&mut self, key: u32) -> Option<&mut (dyn ApiEntity + '_)> {
        for entity in self.entities.iter_mut() {
            if entity.key() == key {
                return Some(entity.as_mut());
            }
        }
        None
    }

    /// Route a command to its target entity by key, returning any new state to
    /// broadcast. `None` if no entity matches or the command was a no-op.
    pub fn apply_command(&mut self, command: &CommandMessage) -> Option<StateMessage> {
        self.entity_by_key(command.key())
            .and_then(|e: &mut dyn ApiEntity| e.handle_command(command))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A moisture sensor config, the plant monitor's canonical entity.
    fn moisture_config() -> SensorConfig {
        SensorConfig {
            object_id: "soil_moisture".to_string(),
            name: "Soil Moisture".to_string(),
            unit_of_measurement: "%".to_string(),
            accuracy_decimals: 0,
            device_class: "moisture".to_string(),
            state_class: proto::SensorStateClass::StateClassMeasurement,
        }
    }

    #[test]
    fn object_id_key_matches_the_fnv1a_snapshot() {
        // The exact value the Python oracle (tools/fnv1a_oracle.py) computes; a
        // change here means the hash algorithm drifted, which would rekey every
        // entity in HA.
        assert_eq!(object_id_key("soil_moisture"), 0x22f6_23e5);
        // The empty object_id is the bare offset basis — no bytes mixed in.
        assert_eq!(object_id_key(""), FNV_OFFSET_BASIS);
    }

    #[test]
    fn the_key_is_derived_from_the_object_id() {
        let sensor: SensorEntity = SensorEntity::new(moisture_config());
        assert_eq!(sensor.key(), object_id_key("soil_moisture"));
        assert_eq!(sensor.object_id(), "soil_moisture");
    }

    #[test]
    fn a_fresh_sensor_reports_missing_state() {
        let sensor: SensorEntity = SensorEntity::new(moisture_config());
        match sensor.state_message() {
            StateMessage::Sensor(m) => {
                assert!(m.missing_state, "no reading yet -> missing_state");
                assert_eq!(m.key, sensor.key());
            }
            other => panic!("expected a Sensor state, got {other:?}"),
        }
    }

    #[test]
    fn setting_a_reading_clears_missing_state() {
        let mut sensor: SensorEntity = SensorEntity::new(moisture_config());
        sensor.set_state(42.5);
        match sensor.state_message() {
            StateMessage::Sensor(m) => {
                assert!(!m.missing_state);
                assert_eq!(m.state, 42.5);
            }
            other => panic!("expected a Sensor state, got {other:?}"),
        }
        sensor.clear_state();
        match sensor.state_message() {
            StateMessage::Sensor(m) => assert!(m.missing_state, "cleared -> missing again"),
            other => panic!("expected a Sensor state, got {other:?}"),
        }
    }

    #[test]
    fn the_list_message_carries_the_config() {
        let sensor: SensorEntity = SensorEntity::new(moisture_config());
        match sensor.list_message() {
            ListMessage::Sensor(m) => {
                assert_eq!(m.object_id, "soil_moisture");
                assert_eq!(m.key, sensor.key());
                assert_eq!(m.unit_of_measurement, "%");
                assert_eq!(m.device_class, "moisture");
                assert_eq!(
                    m.state_class,
                    proto::SensorStateClass::StateClassMeasurement as i32
                );
            }
            other => panic!("expected a Sensor list, got {other:?}"),
        }
    }

    #[test]
    fn an_empty_registry_enumerates_nothing() {
        let registry: Registry = Registry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.list_messages().is_empty());
        assert!(registry.state_messages().is_empty());
    }

    #[test]
    fn a_registry_of_one_enumerates_one() {
        let mut registry: Registry = Registry::new();
        registry.register(Box::new(SensorEntity::new(moisture_config())));
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.list_messages().len(), 1);
        assert_eq!(registry.state_messages().len(), 1);
    }

    #[test]
    fn a_registry_of_many_enumerates_each_in_order() {
        let mut registry: Registry = Registry::new();
        registry.register(Box::new(SensorEntity::new(moisture_config())));
        let temp: SensorConfig = SensorConfig {
            object_id: "temperature".to_string(),
            name: "Temperature".to_string(),
            unit_of_measurement: "°C".to_string(),
            accuracy_decimals: 1,
            device_class: "temperature".to_string(),
            state_class: proto::SensorStateClass::StateClassMeasurement,
        };
        registry.register(Box::new(SensorEntity::new(temp)));

        assert_eq!(registry.len(), 2);
        let states: Vec<StateMessage> = registry.state_messages();
        assert_eq!(states.len(), 2);
        // Order preserved: soil first, temperature second, each with its key.
        assert_eq!(states[0].key(), object_id_key("soil_moisture"));
        assert_eq!(states[1].key(), object_id_key("temperature"));
    }

    #[test]
    fn a_command_to_a_read_only_sensor_is_a_no_op() {
        let mut registry: Registry = Registry::new();
        let sensor: SensorEntity = SensorEntity::new(moisture_config());
        let key: u32 = sensor.key();
        registry.register(Box::new(sensor));

        // A switch command aimed (by key) at the sensor: routed, ignored.
        let command: CommandMessage = CommandMessage::Switch(proto::SwitchCommandRequest {
            key,
            state: true,
            device_id: 0,
        });
        assert_eq!(registry.apply_command(&command), None);
    }

    #[test]
    fn a_command_for_an_unknown_key_routes_nowhere() {
        let mut registry: Registry = Registry::new();
        registry.register(Box::new(SensorEntity::new(moisture_config())));
        let command: CommandMessage = CommandMessage::Switch(proto::SwitchCommandRequest {
            key: 0xDEAD_BEEF,
            state: true,
            device_id: 0,
        });
        assert_eq!(registry.apply_command(&command), None);
    }

    #[test]
    fn every_wire_id_agrees_with_the_registry_table() {
        // Each enum variant's wire id must equal the authoritative MESSAGE_IDS
        // entry for its prost type — the single source of truth. Constructing
        // every variant also proves the placeholders compile against real types.
        let lists: Vec<(ListMessage, &str)> = vec![
            (
                ListMessage::Sensor(Default::default()),
                "ListEntitiesSensorResponse",
            ),
            (
                ListMessage::BinarySensor(Default::default()),
                "ListEntitiesBinarySensorResponse",
            ),
            (
                ListMessage::Switch(Default::default()),
                "ListEntitiesSwitchResponse",
            ),
            (
                ListMessage::Light(Default::default()),
                "ListEntitiesLightResponse",
            ),
            (
                ListMessage::Number(Default::default()),
                "ListEntitiesNumberResponse",
            ),
            (
                ListMessage::Select(Default::default()),
                "ListEntitiesSelectResponse",
            ),
            (
                ListMessage::Cover(Default::default()),
                "ListEntitiesCoverResponse",
            ),
        ];
        for (msg, name) in lists {
            assert_eq!(
                msg.wire().0,
                crate::message_id(name).unwrap(),
                "list id for {name}"
            );
        }

        let states: Vec<(StateMessage, &str)> = vec![
            (
                StateMessage::Sensor(Default::default()),
                "SensorStateResponse",
            ),
            (
                StateMessage::BinarySensor(Default::default()),
                "BinarySensorStateResponse",
            ),
            (
                StateMessage::Switch(Default::default()),
                "SwitchStateResponse",
            ),
            (
                StateMessage::Light(Default::default()),
                "LightStateResponse",
            ),
            (
                StateMessage::Number(Default::default()),
                "NumberStateResponse",
            ),
            (
                StateMessage::Select(Default::default()),
                "SelectStateResponse",
            ),
            (
                StateMessage::Cover(Default::default()),
                "CoverStateResponse",
            ),
        ];
        for (msg, name) in states {
            assert_eq!(
                msg.wire().0,
                crate::message_id(name).unwrap(),
                "state id for {name}"
            );
        }

        // Commands are inbound (HA -> device), so they carry no wire() encoder;
        // assert the key accessor is total across every variant instead.
        let commands: Vec<CommandMessage> = vec![
            CommandMessage::Switch(Default::default()),
            CommandMessage::Light(Default::default()),
            CommandMessage::Number(Default::default()),
            CommandMessage::Select(Default::default()),
            CommandMessage::Cover(Default::default()),
        ];
        for msg in commands {
            assert_eq!(msg.key(), 0, "a default command targets key 0");
        }
    }
}
