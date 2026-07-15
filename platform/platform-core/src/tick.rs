//! The one time unit the whole board measures in.

/// A monotonic time, in milliseconds since boot.
///
/// A plain `u64` so it is `Copy`, allocation-free, and unifies with every other crate's
/// millisecond clock (`plant_core::Tick` is the same alias): time is passed *in* as a value
/// through the [`Clock`](crate::Clock) port, never read from inside the domain. A `u64` of
/// milliseconds outlasts any device that will ever run this firmware.
pub type Tick = u64;
