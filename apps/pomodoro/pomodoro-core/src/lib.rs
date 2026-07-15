#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # pomodoro-core
//!
//! Framework-free pomodoro-timer domain for the M5StickC Plus.
//!
//! Pure `no_std`: the timer state machine, the pause-correct countdown, the long-break
//! cadence, and the transition jingles — deterministic and host-testable, with the Xtensa side
//! supplying only a clock and the button events.
//!
//! ## Hexagon
//! - **Entities**: [`Phase`] (focus / short break / long break) and [`Durations`] — the value
//!   objects, with [`CLASSIC`] the one 25 / 5 / 15 constant to change.
//! - **Entities**: [`Timer`], [`Status`] and [`Event`] — the state machine's data and alphabet.
//!   A [`Timer`] is [`Copy`], so the use case takes and returns it by value.
//! - **Control**: [`step`] — the sampling use case, a pure function `(Timer, Event, now,
//!   Durations) -> `[`Stepped`], folding a clock tick or a button gesture into the next timer
//!   and the sound the transition makes.
//! - **Entities / policy**: [`Jingle`] — which sound a transition plays, mapped to the
//!   `platform_core::Note`s the buzzer adapter sounds. The domain owns the melody; the adapter
//!   only plays it.
//!
//! The freshness and animation of the *display* are not here: the pomodoro screen is one
//! instance of the board-generic render loop, fed a view built from a [`Timer`] and the clock.

mod config;
mod jingle;
mod phase;
mod timer;

pub use config::{Durations, CLASSIC};
pub use jingle::Jingle;
pub use phase::Phase;
pub use timer::{step, Event, Status, Stepped, Timer};
