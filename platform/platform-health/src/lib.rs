#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # platform-health
//!
//! The board's judgement of "is this healthy, and should it be making noise" — a pure domain
//! crate with no hardware, no clock, and no notion of what a thread is. Everything else in the
//! health-supervision epic either feeds this crate evidence or acts on what it decides.
//!
//! ## Two separate responsibilities
//!
//! They answer different questions and are wrong in different ways, so they live in separate
//! files:
//!
//! - **The verdict** ([`assess`], [`Verdict`], [`fault`]) — given what every registered
//!   component last did and when, plus the boot cause and the heap reading, is anything
//!   broken, and what? At most one [`Fault`] is ever reported: several things can be wrong at
//!   once, but the operator gets one banner and one sound, so faults are ranked
//!   ([`Severity`]) and the fold picks the highest.
//! - **The alarm** ([`Alarm`], [`AlarmState`]) — given the verdict, should the buzzer be
//!   sounding right now? A small FSM that chirps on a cadence rather than holding a tone, and
//!   whose acknowledgement silences the *sound* only: the fault stays in the verdict so the
//!   glass keeps naming it. The trap this crate exists to get right is the re-fault case — a
//!   still-present, already-acknowledged fault must stay silent even after a higher-ranked
//!   fault has masked and then cleared. [`ack`] is the memory that makes that hold.
//!
//! ## What this crate is deliberately not
//!
//! It does not know how heartbeats are recorded, how a reset reason is read off the chip, how
//! the buzzer is driven, or what a component *is* at runtime. It sees data — [`Observed`],
//! [`BootVerdict`], [`Resources`] — and returns decisions. `now` is always a parameter; nothing
//! here reads a clock.

mod ack;
mod alarm;
mod boot;
mod component;
mod fault;
mod observed;
mod resources;
mod siren;
mod verdict;

pub use ack::{AckSet, ACK_CAPACITY};
pub use alarm::{Alarm, AlarmState};
pub use boot::{BootVerdict, CrashCause};
pub use component::Component;
pub use fault::{Fault, FaultKey, FaultKind, Severity};
pub use observed::Observed;
pub use resources::Resources;
pub use siren::Siren;
pub use verdict::{assess, Verdict};
