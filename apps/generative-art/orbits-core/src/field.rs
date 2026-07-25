//! The picture: the grid of grey cells the sketch is, assembled from the blooms and the grain.
//!
//! This is where the source's `fill(c * noise(x, y)); rect(x, y, 10)` lives — the grey a cell wears
//! is its [`Column::bloom`](crate::Column) times its [`noise`](crate::noise) grain, clamped where the
//! source saturates. The grid is [`COLS`] by [`COLS`] cells of [`STEP`] source-units each, tiling the
//! `SOURCE`-square canvas exactly. [`for_each_cell`] walks it **column by column**: the frame's
//! centres are folded once, then each column is folded once (its x-distance and dead orbits resolved,
//! see [`Column`](crate::Column)) and read down every row — the render loop's shape, offered as a
//! sink so the display can fill each cell's rectangle without the domain owning any pixels.

use crate::noise::noise;
use crate::orbit::{centres, Centre, Column, ORBITS};

/// The side of one grid cell in source units — the source's `x += 10` / `rect(x, y, 10)`. The cells
/// tile the canvas edge to edge; the display scales a cell onto its pixels.
pub const STEP: u16 = 10;

/// The grid is [`COLS`] cells across and [`COLS`] cells down — `SOURCE / STEP = 50`, the source's
/// `for(x=0;x<W;x+=10)`. Held as the count so the display can iterate it and a test can pin it.
pub const COLS: u16 = 50;

/// The brightness at which the source's grey saturates to white: p5's single-argument `fill` reads
/// its value on `[0, 255]`, so a `c * noise` above `255` is clamped there.
const WHITE_LEVEL: f32 = 255.0;

/// The grey the cell at `(col, row)` wears in `[0, 1]`, given its already-folded [`Column`]: its
/// bloom times its grain, scaled from the source's `[0, 255]` fill range and clamped at white.
///
/// `column.bloom(y) * noise` is the source's `c * noise(x, y)`; dividing by [`WHITE_LEVEL`] carries it
/// from p5's `0..255` grey onto the `0..1` a colour ramp wants, and the clamp is p5's own saturation
/// of an over-range `fill`. A cell no diamond reaches has a zero bloom and so is exactly `0.0`, which
/// the display reads as "leave it the ground" and skips.
fn shade(col: u16, row: u16, column: &Column) -> f32 {
    let y: f32 = (row * STEP) as f32;
    let lit: f32 = column.bloom(y) * noise(col, row);
    let level: f32 = lit / WHITE_LEVEL;
    if level > 1.0 {
        1.0
    } else {
        level
    }
}

/// Walk the grid cells in the column band `cols` at `frame`, handing each `(col, row, grey)` to
/// `sink`.
///
/// Computes the frame's thirty orbit [`centres`] **once**, then walks column by column: each column
/// is folded **once** into a [`Column`] — its x-distances resolved and its unreachable orbits
/// dropped — and read down every row, so a cell pays only the y-part of the surviving orbits. `cols`
/// is the band of columns to walk (all rows of each), so a display that crops the width can skip the
/// cells it will never show and pay the bloom only for what lands on the panel; a caller wanting the
/// whole grid passes `0..COLS`. The band is clamped to the grid, so an out-of-range request draws no
/// cell off the edge. The cells arrive column-major, exactly the source's `for(x)for(y)` order. The
/// sink is the seam to the display: it fills the cell's projected rectangle in the grey's colour, or
/// skips a `0.0`. The domain owns no pixels and no framebuffer; it owns only which cells there are
/// and how bright.
pub fn for_each_cell(frame: f32, cols: core::ops::Range<u16>, mut sink: impl FnMut(u16, u16, f32)) {
    let cs: [Centre; ORBITS] = centres(frame);
    let lo: u16 = cols.start.min(COLS);
    let hi: u16 = cols.end.min(COLS);
    for col in lo..hi {
        let column: Column = Column::at((col * STEP) as f32, &cs);
        for row in 0..COLS {
            sink(col, row, shade(col, row, &column));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orbit::SOURCE;

    /// The grid tiles the canvas exactly: [`COLS`] cells of [`STEP`] units span the whole `SOURCE`
    /// square, with no remainder — the invariant that keeps a projected cell's rectangle butting its
    /// neighbour's with no seam.
    #[test]
    fn the_grid_tiles_the_canvas() {
        assert_eq!(COLS as f32 * STEP as f32, SOURCE);
    }

    /// One cell's grey through the whole assembly: fold its column, then shade it.
    fn grey_at(col: u16, row: u16, cs: &[Centre; ORBITS]) -> f32 {
        shade(col, row, &Column::at((col * STEP) as f32, cs))
    }

    /// Every grey is a valid brightness in `[0, 1]` — the clamp holds even where several diamonds
    /// pile onto one cell, so the display never has an out-of-range colour to encode.
    #[test]
    fn every_grey_is_a_brightness() {
        let cs: [Centre; ORBITS] = centres(60.0);
        let sample: [(u16, u16); 3] = [(0, 0), (25, 25), (49, 49)];
        assert!(sample
            .into_iter()
            .all(|(col, row): (u16, u16)| (0.0..=1.0).contains(&grey_at(col, row, &cs))));
    }

    /// Zero: a cell no diamond reaches is exactly the ground — grey `0.0`, so the display leaves it
    /// black and the comet erases itself cleanly each frame.
    #[test]
    fn an_unreached_cell_is_dark() {
        // Pin every centre at the origin; the far corner cell is outside every diamond.
        let cs: [Centre; ORBITS] = core::array::from_fn(|_| Centre { x: 0.0, y: 0.0 });
        assert_eq!(grey_at(COLS - 1, COLS - 1, &cs), 0.0);
    }

    /// Many: [`for_each_cell`] over the whole width visits the whole grid — one call per cell,
    /// `COLS * COLS` of them, so a display asking for everything is offered every cell exactly once.
    #[test]
    fn the_full_walk_visits_every_cell() {
        let mut visits: u32 = 0;
        for_each_cell(0.0, 0..COLS, |_col: u16, _row: u16, _grey: f32| visits += 1);
        assert_eq!(visits, COLS as u32 * COLS as u32);
    }

    /// One column band: walking a band visits only that band's cells, every row of it — the crop
    /// lever a width-cropping display pulls to skip the bloom for cells it will never show.
    #[test]
    fn a_band_visits_only_its_columns() {
        let mut visits: u32 = 0;
        for_each_cell(0.0, 10..40, |_col: u16, _row: u16, _grey: f32| visits += 1);
        assert_eq!(visits, 30 * COLS as u32);
    }

    /// An over-wide band is clamped to the grid, so a caller cannot drive a walk off the edge into a
    /// column that does not exist.
    #[test]
    fn an_over_wide_band_is_clamped() {
        let mut max_col: u16 = 0;
        for_each_cell(0.0, 0..COLS + 100, |col: u16, _row: u16, _grey: f32| {
            max_col = max_col.max(col)
        });
        assert_eq!(max_col, COLS - 1);
    }
}
