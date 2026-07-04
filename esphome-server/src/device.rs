//! The [`Device`] port: the entity-set the server host serves, abstracted so the
//! host names no concrete entity type.
//!
//! The accept loop ([`crate::server`]) drives the pure [`esphome_api::handle`]
//! FSM, which answers every request from a [`esphome_api::Snapshot`] of the
//! device — its identity, its entity descriptions, and their current states. This
//! trait is exactly that snapshot, as a **port**: the composition root implements
//! it (the plant monitor over its `Registry` + sampler cache, the LED driver over
//! its light `Registry`), and the host serves any implementation. That is what
//! makes the host reusable and free of project-specific logic — it never mentions
//! a sensor, a light, or a moisture reading.
//!
//! ## Threading
//! One `Device` is shared, by `Arc`, across every concurrent connection thread,
//! so it is `Send + Sync` and every method takes `&self`. An implementation whose
//! state changes over time (the sampler publishing a new reading) supplies that
//! through its own interior synchronisation — typically the same `Arc<Mutex<…>>`
//! the producer writes — so [`state_messages`](Device::state_messages) returns the
//! latest values each time the host polls it.

use esphome_api::proto::DeviceInfoResponse;
use esphome_api::{ListMessage, StateMessage};

/// The device a [`Server`](crate::Server) serves: its identity plus its entities'
/// current descriptions and states.
///
/// Shared across connection threads by `Arc`, so `Send + Sync`. Each method is a
/// pure read of the device's *current* view; the host calls
/// [`device_info`](Self::device_info) and [`list_messages`](Self::list_messages)
/// once per connection (they do not change while a device runs) and
/// [`state_messages`](Self::state_messages) every turn and every poll tick, so a
/// live reading reaches a subscribed client.
pub trait Device: Send + Sync {
    /// The `DeviceInfoResponse` returned for `DeviceInfoRequest`; its `name` and
    /// `esphome_version` also fill the `HelloResponse`. Constant for a device.
    fn device_info(&self) -> DeviceInfoResponse;

    /// Every entity's `ListEntities*` description, in advertise order — the reply
    /// to `ListEntitiesRequest`. Constant for a device (entities are registered
    /// at startup, not hot-added).
    fn list_messages(&self) -> Vec<ListMessage>;

    /// Every entity's current `*StateResponse`, in the same order — the initial
    /// `SubscribeStates` stream and the source the host diffs each poll tick to
    /// push device-driven updates. Reflects the latest values at call time.
    fn state_messages(&self) -> Vec<StateMessage>;
}
