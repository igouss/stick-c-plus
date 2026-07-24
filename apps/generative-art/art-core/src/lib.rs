#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # art-core
//!
//! The pure heart of the generative-art gallery: *which* sketch is on the glass, and how a
//! button advances it. No pixels and no framework — the picture a sketch makes is the display
//! crate's business, and this crate owns only the running order and the rule for stepping
//! through it.
//!
//! - [`Sketch`] — the gallery's running order as an enum of pieces. A `Sketch` is an identity,
//!   not a renderer: it names which generative piece is due, and the display turns that name
//!   plus a phase into a frame.
//! - [`Selector`] — the gallery's state: the current sketch and the wrapping [`advance`] a
//!   button press calls. The wrap lives here so the composition root wires a button to
//!   `advance` and nothing else, and so the cycle is proven on the host rather than discovered
//!   on the glass.

mod selector;
mod sketch;

pub use selector::Selector;
pub use sketch::Sketch;
