#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # art-display
//!
//! What the generative-art gallery's glass shows, and nothing about how it is wired.
//!
//! This crate turns the selected [`Sketch`](art_core::Sketch) plus the elapsed animation clock
//! into pixels on any [`DrawTarget`](embedded_graphics::prelude::DrawTarget) — the same code
//! drives the on-target ST7789 panel and a host framebuffer. It is the seam between the gallery
//! domain ([`art_core`]) and the graphics port: the domain says *which* piece is on the glass and
//! the [`Selector`](art_core::Selector) says when it changes; this crate decides what each piece
//! looks like and how the picture reaches the panel.
//!
//! - [`Gallery`] — the renderer. Holds only the frond-compute port across frames;
//!   [`paint_into`](Gallery::paint_into) dispatches on the selected sketch and plots it into a
//!   [`Canvas`] the caller supplies, so the one full-screen buffer lives at the composition root,
//!   not here.
//! - [`Canvas`] — the drawing-surface port a sketch plots into: [`Frame`] is the host adapter (an
//!   `Rgb565` buffer it blits to any [`DrawTarget`](embedded_graphics::prelude::DrawTarget)); the
//!   firmware supplies a wire-order adapter that streams straight over DMA. The port is `Rgb565`, so
//!   the panel's byte order never reaches this crate.
//! - [`FrondCompute`] — the port under the plume's evaluation: turn an animation phase into the
//!   frond's point cloud. [`SerialFrond`] is the one-core default; the firmware injects a two-core
//!   implementation via [`Gallery::with_frond`], a strictly faster route to the identical cloud.
//! - [`GalleryView`] — the app state the render loop shows. It carries the *selected sketch*, so
//!   a button press that changes the sketch changes the view's [`anchor`](platform_core::Animated::anchor)
//!   and the render loop resets the animation clock — the new piece starts from the beginning of
//!   its own motion rather than mid-breath.
//! - [`canvas_size`] — the shape of the canvas a given rotation draws on.
//! - [`REPAINT_MS`] — the gallery's repaint-cadence ceiling, so the composition root drives the
//!   loop as fast as a frame's work allows rather than at a fixed rate.
//!
//! ## One transaction a frame, not six thousand
//!
//! Every sketch is a full-screen picture that changes each frame. Drawing each cell or dot
//! straight onto the panel would be thousands of tiny addressed SPI writes a frame — unwatchable.
//! So each sketch is plotted into one full-screen [`Canvas`] in RAM, and the whole canvas is
//! streamed to the panel as a **single** contiguous window. The picture crosses the wire the way a
//! video frame does: one fill, not thousands of pokes. Rewriting the whole canvas each time is also
//! what makes the animation self-erasing — nothing of the last frame survives, so a moving picture
//! never smears.
//!
//! ## Not every piece is built yet
//!
//! The gallery's running order names five sketches; the plume is ported and the other four are
//! still to come. Rather than hide the unbuilt ones, the render draws each an **honest named
//! placeholder** — its title and "coming soon" — so the button genuinely cycles five distinct
//! screens on the glass, and a placeholder is never mistaken for a finished piece. Each sketch's
//! own commit replaces its placeholder with its real rasterisation.
//!
//! Like every screen here, this is renderable on the host: `cargo run -p art-display
//! --example gallery-screenshots` writes a PNG of each sketch — drawn by the same code the panel
//! runs.

extern crate alloc;

mod canvas;
mod frame;
mod frond;
mod gallery;
mod sketch;
mod view;

pub use canvas::Canvas;
pub use frame::Frame;
pub use frond::{FrondCompute, SerialFrond};
pub use gallery::{canvas_size, Gallery, GROUND_COLOUR, PLUME_COLOUR};
pub use view::{GalleryView, REPAINT_MS};

// The frond's point type, re-exported so a composition root that supplies a parallel
// [`FrondCompute`] can name the cloud it fills without depending on `plume-core` directly.
pub use plume_core::FieldPoint;

// The board-generic foundation, re-exported so the panel adapter and the screenshots example
// reach it through `art_display`.
pub use platform_display::{RenderError, SCREEN_SIZE};

// The pixel type the [`Canvas`] port speaks — re-exported so a composition root building a wire-order
// [`Canvas`] adapter can name its colour without depending on `embedded-graphics` directly.
pub use embedded_graphics::pixelcolor::Rgb565;
