//! The [`Device`] port a native-API host serves — and [`SensorDevice`], a
//! ready-made implementation for a single read-only sensor whose value is pulled
//! from a live source.
//!
//! ## The port lives in the core, not the host
//! A native-API host (the `esphome-server` accept loop — a driving adapter) answers
//! every Home Assistant request from a snapshot of the device: its identity, its
//! entity descriptions, and their current states. [`Device`] is exactly that
//! snapshot, as a **port**. It is defined *here*, in the pure api core, rather than
//! in the host, so that anything implementing it depends only on this crate — never
//! on the transport that drives it. That is what lets a real device be host-tested
//! beside the domain and keeps the hexagon's dependencies pointing inward: the host
//! is generic over `Device` and names no concrete entity; a composition root
//! supplies one.
//!
//! ## A device from a live value
//! [`SensorDevice`] is the common case — one read-only sensor (moisture,
//! temperature, …) whose current value is *not* stored in the entity but **pulled**
//! from a [`SensorSource`] each time the host polls: the shared cache a sampler
//! shell writes, an ADC, a test's toggle. The mapping is the whole point. The
//! source yields `Option<f32>`, and a `None` — no fresh reading — surfaces to Home
//! Assistant as `missing_state`, i.e. *unavailable*, never a stale last-good value.
//! The source is injected, so this stays a pure functional core: the effect (a
//! clock, a socket, a lock) lives at the injection site, not in this crate.

use crate::proto::DeviceInfoResponse;
use crate::{ApiEntity, ListMessage, SensorConfig, SensorEntity, StateMessage};

/// The device a native-API host serves: its identity plus its entities' current
/// descriptions and states.
///
/// Shared across the host's connection threads by `Arc`, so `Send + Sync`, and
/// every method takes `&self` — a pure read of the device's *current* view. A host
/// reads [`device_info`](Self::device_info) and [`list_messages`](Self::list_messages)
/// once per connection (they do not change while a device runs) and
/// [`state_messages`](Self::state_messages) every turn and every poll tick, so a
/// live reading reaches a subscribed client without it asking. An implementation
/// whose values change over time supplies that through its own interior state (a
/// shared cache, a pulled source); this trait never mutates.
pub trait Device: Send + Sync {
    /// The `DeviceInfoResponse` returned for `DeviceInfoRequest`; its `name` and
    /// `esphome_version` also fill the `HelloResponse`. Constant for a device.
    fn device_info(&self) -> DeviceInfoResponse;

    /// Every entity's `ListEntities*` description, in advertise order — the reply
    /// to `ListEntitiesRequest`. Constant for a device (entities are registered at
    /// startup, not hot-added).
    fn list_messages(&self) -> Vec<ListMessage>;

    /// Every entity's current `*StateResponse`, in the same order — the initial
    /// `SubscribeStates` stream and the source the host diffs each poll tick to push
    /// device-driven updates. Reflects the latest values at call time.
    fn state_messages(&self) -> Vec<StateMessage>;
}

/// A live source of a sensor's current value.
///
/// [`read`](Self::read) returns the latest reading, or `None` when there is no
/// fresh one — [`SensorDevice`] maps that `None` to `missing_state`. It is
/// implemented for any `Fn() -> Option<f32>`, so a composition root passes a
/// closure over its shared cache — `move || cache.latest(clock.now(), max_age).map(…)`
/// — with no named type, and a test passes a toggling closure. `Send + Sync`
/// because the device is shared across the host's connection threads.
pub trait SensorSource: Send + Sync {
    /// The sensor's current value, or `None` for "no fresh reading" (→ unavailable).
    fn read(&self) -> Option<f32>;
}

impl<F> SensorSource for F
where
    F: Fn() -> Option<f32> + Send + Sync,
{
    fn read(&self) -> Option<f32> {
        self()
    }
}

/// A [`Device`] presenting exactly one read-only sensor, its value **pulled** from
/// a [`SensorSource`] on every poll.
///
/// The moisture monitor's whole native-API surface: the sensor's identity and
/// descriptor are fixed at construction, and each [`state_messages`](Device::state_messages)
/// call reads the source *now* — so a value the sampler shell has just published
/// reaches Home Assistant on the next poll, and a stale or absent one (`None`)
/// shows as unavailable rather than a frozen last reading.
pub struct SensorDevice<S: SensorSource> {
    info: DeviceInfoResponse,
    sensor: SensorEntity,
    source: S,
}

impl<S: SensorSource> SensorDevice<S> {
    /// Build a one-sensor device from its identity, the sensor's descriptor, and
    /// the live value source.
    ///
    /// The [`SensorEntity`] is used only for its stable identity and description —
    /// its own mutable state is never set; the value always comes from `source`.
    pub fn new(info: DeviceInfoResponse, sensor: SensorConfig, source: S) -> Self {
        Self {
            info,
            sensor: SensorEntity::new(sensor),
            source,
        }
    }
}

impl<S: SensorSource> Device for SensorDevice<S> {
    fn device_info(&self) -> DeviceInfoResponse {
        self.info.clone()
    }

    fn list_messages(&self) -> Vec<ListMessage> {
        vec![self.sensor.list_message()]
    }

    fn state_messages(&self) -> Vec<StateMessage> {
        vec![self.sensor.state_message_for(self.source.read())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto;
    use crate::proto::SensorStateClass;

    /// The plant monitor's moisture sensor — the exact descriptor HA adopts.
    fn moisture_config() -> SensorConfig {
        SensorConfig {
            object_id: "soil_moisture".to_string(),
            name: "Soil Moisture".to_string(),
            unit_of_measurement: "%".to_string(),
            accuracy_decimals: 0,
            device_class: "moisture".to_string(),
            state_class: SensorStateClass::StateClassMeasurement,
        }
    }

    fn plantmon_info() -> DeviceInfoResponse {
        DeviceInfoResponse {
            name: "plantmon".to_string(),
            mac_address: "AA:BB:CC:DD:EE:FF".to_string(),
            model: "M5StickC Plus".to_string(),
            ..Default::default()
        }
    }

    /// The one sensor state message, unwrapped — a device serves exactly one.
    fn sole_sensor_state<S: SensorSource>(device: &SensorDevice<S>) -> proto::SensorStateResponse {
        let mut states: Vec<StateMessage> = device.state_messages();
        assert_eq!(states.len(), 1, "a SensorDevice serves exactly one entity");
        match states.pop().unwrap() {
            StateMessage::Sensor(m) => m,
            other => panic!("expected a Sensor state, got {other:?}"),
        }
    }

    #[test]
    fn a_fresh_reading_is_served_as_the_state() {
        // The source has a value: HA reads it, missing_state clear.
        let device: SensorDevice<_> =
            SensorDevice::new(plantmon_info(), moisture_config(), || Some(42.0));
        let state: proto::SensorStateResponse = sole_sensor_state(&device);
        assert!(!state.missing_state, "a present reading is not missing");
        assert_eq!(state.state, 42.0);
    }

    #[test]
    fn an_absent_reading_is_served_as_missing_state() {
        // The safety rule: a source that has no fresh value (stale or never
        // measured) surfaces as unavailable, never a frozen last-good value.
        let device: SensorDevice<_> =
            SensorDevice::new(plantmon_info(), moisture_config(), || None);
        let state: proto::SensorStateResponse = sole_sensor_state(&device);
        assert!(state.missing_state, "no fresh reading -> missing_state");
    }

    #[test]
    fn the_state_key_matches_the_advertised_entity() {
        // HA routes a state packet to an entity by key; the state's key must equal
        // the one the entity list advertises, or the reading lands nowhere.
        let device: SensorDevice<_> =
            SensorDevice::new(plantmon_info(), moisture_config(), || Some(10.0));
        let list_key: u32 = match device.list_messages().pop().unwrap() {
            ListMessage::Sensor(m) => m.key,
            other => panic!("expected a Sensor list, got {other:?}"),
        };
        assert_eq!(sole_sensor_state(&device).key, list_key);
    }

    #[test]
    fn the_list_message_carries_the_full_descriptor() {
        // Every field HA needs to render the entity — the "with the fields above"
        // acceptance, checked at the wire boundary.
        let device: SensorDevice<_> =
            SensorDevice::new(plantmon_info(), moisture_config(), || None);
        match device.list_messages().pop().unwrap() {
            ListMessage::Sensor(m) => {
                assert_eq!(m.object_id, "soil_moisture");
                assert_eq!(m.name, "Soil Moisture");
                assert_eq!(m.unit_of_measurement, "%");
                assert_eq!(m.accuracy_decimals, 0);
                assert_eq!(m.device_class, "moisture");
                assert_eq!(
                    m.state_class,
                    SensorStateClass::StateClassMeasurement as i32
                );
            }
            other => panic!("expected a Sensor list, got {other:?}"),
        }
    }

    #[test]
    fn device_info_is_carried_through() {
        let device: SensorDevice<_> =
            SensorDevice::new(plantmon_info(), moisture_config(), || None);
        let info: DeviceInfoResponse = device.device_info();
        assert_eq!(info.name, "plantmon");
        assert_eq!(info.mac_address, "AA:BB:CC:DD:EE:FF");
    }
}
