//! The gallery's sketches, each a rasteriser from an elapsed clock into the shared [`Frame`].
//!
//! One module per piece. The [`Gallery`](crate::Gallery) resets the frame and then hands it to
//! exactly one of these, chosen by the selected [`Sketch`](art_core::Sketch); the sketch plots
//! its picture and the gallery blits it. Keeping the rasterisers here, behind an exhaustive match
//! in the gallery, is what makes "a new sketch was added but never drawn" a compile error rather
//! than a blank screen.
//!
//! - [`plume`] — the feathered frond, ported from the standalone app: it projects
//!   `plume_core`'s 400×400 point field onto the panel.
//! - [`squares`] — the breathing grid of nested-square frames, rasterising `squares_core`'s cells
//!   into the panel.
//! - [`placeholder`] — the honest stand-in every not-yet-built piece draws: its title and
//!   "coming soon", so the gallery cycles distinct, truthful screens before all five exist.

pub mod placeholder;
pub mod plume;
pub mod squares;
