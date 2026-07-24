#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # plume-display
//!
//! What the plume's glass shows, and nothing about how it is wired.
//!
//! This crate turns the elapsed animation clock into pixels on any
//! [`DrawTarget`](embedded_graphics::prelude::DrawTarget) — the same code drives the on-target
//! ST7789 panel and a host framebuffer. It is the seam between the plume domain
//! ([`plume_core`]) and the graphics port: the domain says where the frond's points are, this
//! crate decides where they land on the glass and how the picture reaches it.
//!
//! - [`Plume`] — the renderer. Holds the sine table and the offscreen frame across frames;
//!   [`render`](Plume::render) plots the frond of the moment and blits it.
//! - [`PlumeView`] — the app state the render loop shows. It carries no data, only the
//!   animation policy, so the plume rides the *same* generic loop every other app does.
//! - [`canvas_size`] — the shape of the canvas a given rotation draws on.
//! - [`PLUME_COLOUR`]/[`GROUND_COLOUR`] — the two colours the one-bit frame is blitted through.
//!
//! ## One transaction a frame, not five thousand
//!
//! The frond is a cloud of scattered dots, and a full-screen picture that changes every frame.
//! Drawing each dot straight onto the panel would be thousands of tiny addressed SPI writes a
//! frame — unwatchable. So the dots are plotted into a one-bit offscreen frame in RAM, and the
//! whole frame is streamed to the panel as a **single** contiguous window. The picture crosses
//! the wire the way a video frame does: one fill, not thousands of pokes. That the whole frame
//! is rewritten each time is also what makes the animation self-erasing — nothing of the last
//! frond survives, so a moving picture never smears.
//!
//! Like every screen here, this is renderable on the host: `cargo run -p plume-display
//! --example plume-screenshots` writes a PNG of several phases of the breath — drawn by the
//! same code the panel runs.

extern crate alloc;

mod canvas;
mod project;
mod screen;
mod view;

pub use screen::{canvas_size, Plume, GROUND_COLOUR, PLUME_COLOUR};
pub use view::PlumeView;

// The board-generic foundation, re-exported so the panel adapter and the screenshots example
// reach it through `plume_display`.
pub use platform_display::{RenderError, SCREEN_SIZE};
