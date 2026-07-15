#![forbid(unsafe_code)]
//! # pomodoro-shell
//!
//! The pomodoro timer's imperative shell — the part that owns the timer cache and the input
//! thread, yet stays device-independent, so it is verified on the host with the same
//! discipline as the pure FSM.
//!
//! The pure core (`pomodoro-core`) decides *what* a gesture or a tick does to the timer. This
//! crate drives that core against the outside world:
//!
//! - [`SharedTimer`] — the latest [`Timer`](pomodoro_core::Timer), shared between the input
//!   thread (the one writer) and the render loop (the reader). Poison-tolerant.
//! - [`spawn_input`] — the sized background thread: poll the front/side buttons, fold each
//!   through the pure `Debounce`, map the gesture to an [`Event`](pomodoro_core::Event), fire a
//!   tick, and sound the resulting jingles on the buzzer. It takes the Button / Clock / Tone
//!   ports injected, so the whole control is exercised on the host with fake peripherals.
//!
//! Everything here is `std`, but nothing is ESP-specific, so the shell cross-compiles to
//! esp-idf `std` unchanged; the composition root only wires the real GPIO buttons, the LEDC
//! buzzer, and the `Monotonic` clock in.

mod input;
mod shared;

pub use input::{spawn_input, InputTask, INPUT_STACK_SIZE, POLL_PERIOD};
pub use shared::SharedTimer;
