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
//! - [`Gallery`] — the renderer. Holds the sine table and the offscreen frame across frames;
//!   [`render`](Gallery::render) dispatches on the selected sketch, draws it into the frame, and
//!   blits the frame in one window.
//! - [`GalleryView`] — the app state the render loop shows. It carries the *selected sketch*, so
//!   a button press that changes the sketch changes the view's [`anchor`](platform_core::Animated::anchor)
//!   and the render loop resets the animation clock — the new piece starts from the beginning of
//!   its own motion rather than mid-breath.
//! - [`canvas_size`] — the shape of the canvas a given rotation draws on.
//! - [`FRAME_MS`] — the gallery's animation cadence, so the view and the composition root agree
//!   on how one repaint maps to one frame.
//!
//! ## One transaction a frame, not five thousand
//!
//! Every sketch is a full-screen picture that changes each frame. Drawing each cell or dot
//! straight onto the panel would be thousands of tiny addressed SPI writes a frame — unwatchable.
//! So each sketch is plotted into one offscreen [`Frame`](frame::Frame) of `Rgb565` in RAM, and
//! the whole frame is streamed to the panel as a **single** contiguous window. The picture
//! crosses the wire the way a video frame does: one fill, not thousands of pokes. Rewriting the
//! whole frame each time is also what makes the animation self-erasing — nothing of the last
//! frame survives, so a moving picture never smears.
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

mod frame;
mod gallery;
mod sketch;
mod view;

pub use gallery::{canvas_size, Gallery, GROUND_COLOUR, PLUME_COLOUR};
pub use view::{GalleryView, FRAME_MS};

// The board-generic foundation, re-exported so the panel adapter and the screenshots example
// reach it through `art_display`.
pub use platform_display::{RenderError, SCREEN_SIZE};
