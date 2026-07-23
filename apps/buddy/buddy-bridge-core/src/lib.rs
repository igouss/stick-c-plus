#![forbid(unsafe_code)]
//! # buddy-bridge-core
//!
//! The framework-free policy of the Linux BLE-central bridge — the piece Claude Code does not
//! ship. It decides *what to do next*, never *how*: the concrete BLE work (bluer, tokio,
//! D-Bus) lives in the driving-adapter shell, and this crate stays a pure function of its
//! inputs so every rule is proven on the host.
//!
//! ## The layers
//! - [`fsm`] — the pairing/connection decision [`Fsm`]: `(State, Event) → Action`, with
//!   stale-LTK recovery ([`Action::RemoveDeviceThenReacquire`]) as a first-class transition
//!   and a fatal [`Action::FailFast`] for a missing pairing agent.
//! - [`chunk`] — [`chunk`], the `mtu - 3` TX splitter the firmware's `notify_chunked` mirrors.
//! - [`backoff`] — [`backoff`], the capped-exponential reconnect schedule (a pure function of
//!   the consecutive-failure count; BlueZ never auto-reconnects a central).
//! - [`io_capability`] — [`IoCapability`], a one-variant type that makes the insecure
//!   NoInputNoOutput (Just Works) pairing posture unrepresentable.
//!
//! Time is a parameter, never a call: the FSM emits [`Action::Backoff`] with a `Duration` and
//! the adapter owns the sleep, so this crate holds no clock (and stays effect-audit clean).

pub mod backoff;
pub mod chunk;
pub mod fsm;
pub mod io_capability;

pub use backoff::backoff;
pub use chunk::chunk;
pub use fsm::{Action, Event, Fsm, State};
pub use io_capability::IoCapability;
