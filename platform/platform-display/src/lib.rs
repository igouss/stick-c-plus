#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # platform-display
//!
//! The board-generic display foundation, shared by every app on the M5StickC Plus. It
//! turns pixels into any [`DrawTarget`](embedded_graphics::prelude::DrawTarget) — the same
//! code drives the on-target ST7789 panel and a host framebuffer — but names no app: no
//! `Observation`, no timer, only the parts every screen is built from.
//!
//! - [`sprite`] — the vendored ClaudePix 20×20 creature library, 4-bit palette indices
//!   packed two to a byte, with a pure elapsed-milliseconds animation clock. See
//!   `kb/sources/claudepix.md` for provenance and its unresolved licence.
//! - [`text_line`]/[`text_overlay`] — the two fixed-width text primitives, placed by an
//!   explicit `origin` so every app's layout is the same primitive positioned differently.
//! - [`sparkline`] — a bar-graph of `0..=100` values into an explicit `area`, drawn as one
//!   contiguous erase-in-place fill so a scrolling usage graph never flashes.
//! - [`signed_bar`] — one `-full..=+full` value as a bar growing left or right from a centre
//!   axis, so a signed reading's sign is legible as direction. The same one-window
//!   erase-in-place fill, so a bar tracking a live sensor never flashes either.
//! - [`colour_bands`] — the RGB bring-up self-test that makes a channel swap falsifiable.
//! - [`rotation_frame`] — its sibling for rotation: a border on the canvas edge that makes a
//!   wrong CGRAM window, and a wrong way up, falsifiable on the glass.
//! - [`RenderError`] — the render failure, generic in the target's own error, so this
//!   crate names no concrete panel.
//! - [`testing`] — a pixel-counting host `DrawTarget` for dependents' tests (feature
//!   `testing`); never compiled into firmware.
//!
//! ## What a host render can and cannot prove
//!
//! It **can** prove the layout, the wording, the alignment, the colour each state is asked
//! to be drawn in, and that a short value erases the longer one it replaces. It **cannot**
//! prove anything below [`DrawTarget`]: the panel's MADCTL colour order, its CGRAM offset,
//! its inversion, or whether the backlight rail is even powered. Only [`colour_bands`] on
//! the real panel can catch a channel swap.

#[cfg(any(test, feature = "testing"))]
extern crate alloc;

mod colour_check;
mod error;
mod rotation_check;
mod signed_bar;
mod sparkline;
mod text;

pub mod sprite;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use colour_check::colour_bands;
pub use error::RenderError;
pub use rotation_check::rotation_frame;
pub use signed_bar::signed_bar;
pub use sparkline::sparkline;
pub use text::{text_field, text_line, text_overlay, FieldAlign, FONT, LINE_CAP, SCREEN_SIZE};
