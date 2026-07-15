#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # plant-display
//!
//! What the plant monitor's glass says, and nothing about how it is wired.
//!
//! This crate turns a [`plant_core::Observation`] into pixels on any
//! [`DrawTarget`](embedded_graphics::prelude::DrawTarget). It is the seam between the plant
//! domain and the graphics port: `DrawTarget` is the port, the on-target ST7789 panel
//! (`firmware` adapters) is one adapter for it, and a host framebuffer is another. The
//! board-generic parts it draws with — the ClaudePix sprite library, the fixed-width text
//! primitives, the colour self-test, the render error — live in [`platform_display`] and
//! are re-exported here, so this crate holds only the plant-specific *picture*.
//!
//! - [`render`] — the observation screen: the raw count and the percent (or the named
//!   reason there is neither), beside the creature that stands for the device's health.
//! - [`scene`] — the animation policy: which creature, and whether it moves. A healthy
//!   reading maps to a *motionless* one, so the render loop repaints nothing and the device
//!   may sleep; only the states an operator must notice are allowed to move.
//!
//! ## Why this is not inside the display adapter
//!
//! It was, until it needed to be *seen*. The layout lived beside the SPI bus and the CGRAM
//! offsets, so the only way to look at the screen was to flash a board. Pulling the picture
//! out from under the panel makes it renderable on the host: `cargo run -p plant-display
//! --example screenshots` writes a PNG of every state the glass can be in — drawn by *this*
//! code, the same code the panel runs, not a replica that would agree with whatever the
//! author believed.
//!
//! ## What a host render can and cannot prove
//!
//! It **can** prove the layout, the wording, the alignment, the colour each state is drawn
//! in, that a short value erases the longer one it replaces, and that nothing is clipped.
//! It **cannot** prove anything below [`DrawTarget`]: the MADCTL colour order, the CGRAM
//! offset, the inversion, or whether the backlight rail is powered. A host framebuffer
//! paints `Rgb565::RED` as red whatever the glass would do; only [`colour_bands`] on the
//! real panel can catch a channel swap.

mod layout;
pub mod scene;
mod screen;

pub use layout::{LINE_WIDTH, PCT_Y, RAW_Y, SPRITE_ORIGIN, SPRITE_SCALE, TEXT_X};
pub use scene::{frame_index, is_animated, scene, Motion, Scene};
pub use screen::{fault_label, render};

// The board-generic foundation, re-exported so existing consumers (the ST7789 adapter, the
// screenshots example) still reach it through `plant_display`.
pub use platform_display::{colour_bands, sprite, RenderError, SCREEN_SIZE};
