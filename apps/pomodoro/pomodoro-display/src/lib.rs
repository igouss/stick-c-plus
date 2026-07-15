#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # pomodoro-display
//!
//! What the pomodoro timer's glass says, and nothing about how it is wired.
//!
//! This crate turns a [`PomodoroView`] — a snapshot of a `pomodoro_core::Timer` — into pixels
//! on any [`DrawTarget`](embedded_graphics::prelude::DrawTarget). It is the seam between the
//! pomodoro domain and the graphics port, built on the board-generic [`platform_display`]
//! primitives (the sprite library, the fixed-width text). It holds only the pomodoro-specific
//! picture: the phase label, the `MM:SS` clock, and the creature that codes through a focus
//! and dances through a break.
//!
//! - [`render`] — draw the label, the clock, and the creature for a view.
//! - [`PomodoroView`] — the snapshot the render loop shows; it implements
//!   [`Animated`](platform_core::Animated), so a pomodoro timer drives the *same* generic
//!   render loop the plant monitor does.
//!
//! Like the plant screen, this is renderable on the host: `cargo run -p pomodoro-display
//! --example screenshots` writes a PNG of every state the glass can be in — drawn by the same
//! code the panel runs.

mod layout;
mod scene;
mod screen;
mod view;

pub use screen::{label_text, render};
pub use view::PomodoroView;

// The board-generic foundation, re-exported so the panel adapter and the screenshots example
// reach it through `pomodoro_display`.
pub use platform_display::{RenderError, SCREEN_SIZE};
