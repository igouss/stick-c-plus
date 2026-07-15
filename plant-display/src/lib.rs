#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # plant-display
//!
//! What the plant monitor's glass says, and nothing about how it is wired.
//!
//! This crate turns a [`plant_core::Observation`] into pixels on any
//! [`DrawTarget`](embedded_graphics::prelude::DrawTarget). It is the seam between the
//! domain and the graphics port: `DrawTarget` is the port, the on-target ST7789 panel
//! (`firmware/adapters`) is one adapter for it, and a host framebuffer is another.
//!
//! - [`render`] — the observation screen: the raw count and the percent (or the named
//!   reason there is neither), beside the creature that stands for the device's health.
//! - [`scene`] — the animation policy: which creature, and whether it moves. A healthy
//!   reading maps to a *motionless* one, so the render loop repaints nothing and the
//!   device may sleep; only the states an operator must notice are allowed to move.
//! - [`sprite`] — the vendored 20×20 creature library, 4-bit palette indices packed two to
//!   a byte. See `kb/sources/claudepix.md` for provenance and its unresolved licence.
//! - [`colour_bands`] — the bring-up self-test: three primaries, so a channel swap
//!   becomes falsifiable.
//!
//! ## Why this is not inside the display adapter
//!
//! It was, until it needed to be *seen*. The layout lived beside the SPI bus and the
//! CGRAM offsets, so the only way to look at the screen was to flash a board and hold
//! it up to the light — and the only way to test a wording or an alignment was to
//! read the code and imagine the result. Pulling the picture out from under the panel
//! makes it renderable on the host: `cargo run -p plant-display --example screenshots`
//! writes a PNG of every state the glass can be in.
//!
//! Crucially the PNG is drawn by *this* code, the same code the panel runs — not a
//! replica of it. A replica would agree with whatever the author believed, which is
//! precisely the failure this project has already paid for once.
//!
//! ## What a host render can and cannot prove
//!
//! It **can** prove: the layout, the wording, the alignment, the colour each state is
//! asked to be drawn in, that a short value erases the longer one it replaces, and
//! that nothing is clipped off the canvas.
//!
//! It **cannot** prove anything about the panel below [`DrawTarget`]: the MADCTL
//! colour order, the CGRAM offset, the colour inversion, or whether the backlight
//! rail is even powered. A host framebuffer renders `Rgb565::RED` as red whatever the
//! glass would do. The red/blue swap fixed in `adapters::st7789` would **not** have
//! been caught here — only [`colour_bands`] on the real panel can catch it. Believing
//! otherwise would be exactly the convenient signal that hid that bug for months.

mod colour_check;
mod error;
mod layout;
pub mod scene;
mod screen;
pub mod sprite;
#[cfg(test)]
mod testing;

pub use colour_check::colour_bands;
pub use error::RenderError;
pub use layout::{LINE_WIDTH, PCT_Y, RAW_Y, SCREEN_SIZE, SPRITE_ORIGIN, SPRITE_SCALE, TEXT_X};
pub use scene::{frame_index, is_animated, scene, Motion, Scene};
pub use screen::{fault_label, render};
