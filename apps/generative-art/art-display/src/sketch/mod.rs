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
//! - [`fan`] — the folding, hue-by-distance triangles, projecting `fan_core`'s cells onto the panel
//!   and filling each in its colour.
//! - [`orbits`] — the comet of diamond blooms, tiling `orbits_core`'s grid of cells onto the panel
//!   and filling each lit one in its grey.
//! - [`willow`] — the gallery's one original piece: a curtain of hanging tendrils, plotting
//!   `willow_core`'s swaying points onto the panel in their foliage greens.

pub mod fan;
pub mod orbits;
pub mod plume;
pub mod squares;
pub mod willow;
