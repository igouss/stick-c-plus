#![forbid(unsafe_code)]
//! # buddy-bridge-shell
//!
//! The imperative shell of the Linux BLE-central bridge: it runs the pure decision
//! (`buddy-bridge-core`) against the real BLE stack, and stays host-testable everywhere the
//! device is not strictly required.
//!
//! - [`Central`] — the port the FSM drives: connect / pair / recover a stale bond / subscribe /
//!   write, with every stack error mapped to a typed [`CentralError`].
//! - [`DriveLoop`] — the generic loop that runs a `Fsm` against any [`Central`] + [`Sleeper`].
//!   Because both are ports, the whole reconnect/recovery flow is proven on the host against a
//!   fake; only the concrete bluer adapter needs the stick.
//! - [`RxReassembler`] — reuses `buddy-wire`'s `Ring` + `Framer` to turn fragmented TX
//!   notifications (and the device's separate-`\n` quirk) back into whole JSON lines.
//! - [`Sleeper`] — the injected async sleep, so the backoff schedule runs instantly under test.
//!
//! The concrete `BluerCentral` over bluer 0.17.4 and the `#[ignore]`d device-in-the-loop test
//! land alongside these once the bluer API is pinned.

pub mod central;
pub mod drive_loop;
pub mod reassembler;
pub mod sleeper;

pub use central::{Central, CentralError, Connected};
pub use drive_loop::{DriveLoop, Step};
pub use reassembler::RxReassembler;
pub use sleeper::{NoSleep, Sleeper, TokioSleeper};
