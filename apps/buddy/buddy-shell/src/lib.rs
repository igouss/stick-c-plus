#![forbid(unsafe_code)]
//! # buddy-shell
//!
//! The desk pet's imperative shell on the *stick* — the part that owns the device state and the
//! input thread, yet stays device-independent, so it is verified on the host with the same
//! discipline as the pure domain.
//!
//! The pure core (`buddy-core`) decides what a snapshot, a shake or a button does to the buddy.
//! The wire crate (`buddy-wire`) decides what the bytes mean. The picture (`buddy-display`)
//! decides what it looks like. This crate drives all three against the outside world:
//!
//! - [`DeviceState`] — everything the stick knows, and the three ways it changes: a line arrives
//!   ([`apply`](DeviceState::apply)), a button is pressed ([`press`](DeviceState::press)), time
//!   passes ([`tick`](DeviceState::tick)). [`view`](DeviceState::view) is the picture for all of
//!   it.
//! - [`SharedDevice`] — the one state, shared between the BLE receive callback, the input
//!   thread, and the render loop. Poison-tolerant: a panicking writer must not freeze the glass.
//! - [`spawn_input`] — the sized background thread: poll the buttons, fold each press into the
//!   state, and carry out the [`Effect`] it hands back through the injected ports.
//! - [`spawn_sensors`] — the buddy's own senses on a slow cadence: the IMU (shake, nap) and the
//!   power rail, and the tick that advances the domain's clock.
//! - [`Receiver`] — raw GATT bytes in, framed, classified, and folded. Every malformed line is
//!   logged and dropped, never guessed at.
//! - [`Notifier`], [`Bond`], [`SpeciesStore`] — the three driven ports, each one method wide.
//!   The composition root implements them over NimBLE and NVS; a test implements them over a
//!   `Vec`.
//!
//! Everything here is `std`, but nothing is ESP-specific, so the shell cross-compiles to esp-idf
//! `std` unchanged; the composition root only wires the real GPIO buttons, the NUS
//! characteristic, and the `Monotonic` clock in.
//!
//! ## The two things this crate must not get wrong
//!
//! **The fail-safe.** Only a real snapshot may clear a pending prompt. `buddy_wire` makes that
//! structurally true; [`DeviceState::apply`] has to honour it rather than undo it, which is why
//! its match is exhaustive and three of its four arms go nowhere near the prompt.
//!
//! **The answer.** A press on A while a prompt is pending must produce
//! `{"cmd":"permission","id":…,"decision":"once"}` on the wire, and B the same with `"deny"`.
//! That is the whole feature; everything else on the glass is in service of it.

mod identity;
mod input;
mod nav;
mod ports;
mod receive;
mod sensors;
mod shared;
mod state;
mod tally;
mod wall;

pub use identity::Identity;
pub use input::{perform, spawn_input, InputTask, INPUT_CONFIG, INPUT_STACK_SIZE, POLL_PERIOD};
pub use nav::Nav;
pub use ports::{Bond, Notifier, NotifyError, SpeciesStore};
pub use receive::Receiver;
pub use sensors::{spawn_sensors, SenseTask, SENSE_PERIOD, SENSE_STACK_SIZE};
pub use shared::SharedDevice;
pub use state::{DeviceState, Effect, LINK_WINDOW_MS};
pub use tally::Tally;
pub use wall::WallClock;
