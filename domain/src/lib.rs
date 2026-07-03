#![cfg_attr(not(test), no_std)]
//! # led-core
//!
//! Framework-free LED-strip animation core for the M5StickC Plus, inspired by
//! [NightDriverStrip](https://github.com/PlummersSoftwareLLC/NightDriverStrip).
//!
//! Pure `no_std`, zero dependencies, no allocation: just colors, effects, and
//! the [ports](ports) the firmware wires to real hardware. Everything here is
//! deterministic and host-testable — the Xtensa side only supplies a clock and
//! a pixel sink.
//!
//! ## Hexagon
//! - **Entities**: [`Rgb`], [`Hsv`] — the palette primitives.
//! - **Entities / policy**: [`Effect`] implementations ([`SolidColor`],
//!   [`Rainbow`]) — pure functions from *time* to a *frame of pixels*.
//! - **Control**: [`Animator`] — the render use case (sample time → render →
//!   push), depending only on ports.
//! - **Ports**: [`Clock`], [`LedOutput`] — interfaces the firmware implements.

pub mod animator;
pub mod color;
pub mod effect;
pub mod ports;

pub use animator::Animator;
pub use color::{hsv_to_rgb, Hsv, Rgb};
pub use effect::{Effect, Rainbow, SolidColor};
pub use ports::{Clock, LedOutput};
