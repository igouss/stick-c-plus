#![forbid(unsafe_code)]
//! # art-shell
//!
//! The generative-art gallery's imperative shell — the part that owns the selector cache and the
//! input thread, yet stays device-independent, so it is verified on the host with the same
//! discipline as the pure domain.
//!
//! The pure core (`art-core`) decides *what* a press does to the gallery: [`Selector::advance`]
//! steps to the next sketch, wrapping. This crate drives that core against the outside world:
//!
//! - [`SharedSelector`] — the current [`Selector`](art_core::Selector), shared between the input
//!   thread (the one writer) and the render loop (the reader). Poison-tolerant, so a panic in one
//!   holder cannot take the other down.
//! - [`spawn_input`] — the sized background thread: poll the front button, fold its raw level
//!   through the single-button gesture [`Recognizer`](platform_input::Recognizer), and advance the
//!   shared selector on a click. It takes the Button and Clock ports injected, so the whole
//!   control is exercised on the host with fake peripherals.
//!
//! Everything here is `std`, but nothing is ESP-specific, so the shell cross-compiles to esp-idf
//! `std` unchanged; the composition root only wires the real GPIO button and the `Monotonic` clock
//! in.

mod input;
mod shared;

pub use input::{
    spawn_input, Command, InputTask, GESTURES, INPUT_CONFIG, INPUT_STACK_SIZE, POLL_PERIOD,
};
pub use shared::SharedSelector;
