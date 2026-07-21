#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # orientation-display
//!
//! What the orientation readout's glass says, and nothing about how it is wired.
//!
//! This crate turns an [`OrientationView`] — a snapshot of an `orientation_core::Orientation`
//! — into pixels on any [`DrawTarget`](embedded_graphics::prelude::DrawTarget). It is the
//! seam between the orientation domain and the graphics port, built on the board-generic
//! [`platform_display`] primitives. It holds only the orientation-specific picture: the
//! resting-face label, the pitch and roll, and the three signed X/Y/Z bars.
//!
//! - [`render`] — draw the whole readout for a view.
//! - [`canvas_size`] — the shape of the canvas a given rotation draws on.
//! - [`OrientationView`] — the snapshot the render loop shows; it implements
//!   [`Animated`](platform_core::Animated), so the orientation readout drives the *same*
//!   generic render loop the pomodoro timer and the plant monitor do.
//!
//! ## Why bars and not a 3D cube
//!
//! A rotating wireframe would be prettier and would say less. Three centre-zero bars show the
//! *measurements* — sign, magnitude, and which axis carries them — so what is on the glass is
//! what the sensor said, and a mis-mapped or dead axis is visible immediately rather than
//! hidden inside a projection. The face label and the angles sit above them for the
//! at-a-glance answer; the bars are there so that answer can be checked.
//!
//! ## Why there are two layouts and not one transform
//!
//! The view carries the rotation it is drawn at, and [`render`] lays the same picture out
//! through the layout for that rotation's canvas shape. Two layouts rather than one wrapped in
//! a rotating `DrawTarget`, because the two canvases are not the same picture scaled: a
//! landscape line holds 24 characters and a portrait one holds 13, which is too few for the
//! header's label and angles side by side. Both layouts' geometry is proven by the compiler —
//! a field off an edge or a bar into its value fails the *build*.
//!
//! Like every screen here, this is renderable on the host: `cargo run -p orientation-display
//! --example screenshots` writes a PNG of every pose the board can be in — drawn by the same
//! code the panel runs.

mod layout;
mod screen;
mod view;

pub use layout::canvas_size;
pub use screen::render;
pub use view::OrientationView;

// The board-generic foundation, re-exported so the panel adapter and the screenshots example
// reach it through `orientation_display`.
pub use platform_display::{RenderError, SCREEN_SIZE};
