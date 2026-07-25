//! The breathing grid: nested-square frames that pulse and flip sign in a diagonal wave.
//!
//! The whole sketch is one grid of cells, each a pure function of the phase. Cell `(col, row)`
//! breathes by a single sine of a phase offset that is fixed for that cell; as the phase advances
//! every cell's breath sweeps, and because neighbouring cells carry a large fixed offset between
//! them the breaths run out of step in a diagonal wave across the grid. There is no state and no
//! history — the grid at phase `φ` is [`cells`]`(φ, table)` and nothing else.
//!
//! ## Provenance
//!
//! A faithful port of a 280-character *Dwitter*, kept to its coordinates so the picture is the
//! same creature:
//!
//! ```text
//! f=0
//! draw=_=>{
//!   f++||createCanvas(W=500,W); background(0)
//!   for(x=0;x<600;x+=50)for(y=0;y<600;y+=50){
//!     fill(W); rect(x,y,C=50*sin(x+y*2+f/30))
//!     fill(0); rect(x+C/4,y+C/4,C/2)
//!   }
//! }
//! ```
//!
//! The source draws, per cell, a white square of side `C = 50·sin(x + 2y + f/30)` and a black
//! square of half that side centred inside it — a square *frame*, whose thickness and sign
//! breathe with `C`. When `C` is negative the square is drawn inverted, so the frame flips across
//! its own corner. Two things move here, both because they are about the *shape of the picture*
//! rather than the shape of the glass:
//!
//! - **The grid is sized to the panel, not the source's 500×500.** The panel is a 135×240
//!   portrait, so the grid is [`COLS`]×[`ROWS`] cells; the display sizes a cell to the panel's
//!   width. The per-cell phase offset keeps the source's `x + 2y` in the source's *grid* units
//!   ([`OFFSET_STEP`]·`(col + 2·row)`), so the diagonal wave has exactly the same character as the
//!   original however the cells are sized on the glass.
//! - **The breath is `f32` and a table sine.** Every value is single-precision, for the ESP32's
//!   FPU, and the one sine a cell needs is read from the shared [`SinTable`].

use platform_numerics::SinTable;

/// How many cells across the grid is. Five columns fill the 135-column portrait panel at a
/// 27-pixel cell — dense enough for the diagonal wave to read, coarse enough that each frame is a
/// clear shape on the small glass. The display sizes the cell; the count is the artwork's, so it
/// lives here.
pub const COLS: u16 = 5;

/// How many cells down the grid is. Nine rows cover the 240-row portrait panel at the same cell
/// size, the block centred so the top and bottom rows graze the edges.
pub const ROWS: u16 = 9;

/// The phase offset between adjacent grid steps, in radians — the source's `x + 2y` with `x`, `y`
/// stepping by fifty. Kept in the source's grid units rather than the panel's pixels so the
/// diagonal wave is the same creature at any cell size: cell `(col, row)`'s offset is
/// `OFFSET_STEP · (col + 2·row)`. The value is large — many turns between neighbours — which is
/// exactly what makes adjacent breaths fall out of step into a wave rather than pulse as one.
pub const OFFSET_STEP: f32 = 50.0;

/// One cell of the grid at a phase: where it sits in the grid, and its signed breath.
///
/// Carried by value — a plain result, not an object. The cell's *pixel* rectangle (its origin and
/// side on the panel, and the black frame inside it) is the display crate's business; this crate
/// says only which cell it is and how far — and which way — it is breathing.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cell {
    /// The cell's column, `0..COLS`.
    pub col: u16,
    /// The cell's row, `0..ROWS`.
    pub row: u16,
    /// The signed breath, `sin(offset + φ)` in `[-1, 1]`: the fraction of a full cell the white
    /// square spans, its sign the source's inversion — negative draws the frame flipped across
    /// the cell's corner. The display scales this by the pixel cell size to get the square's side.
    pub breath: f32,
}

/// The signed breath of cell `(col, row)` at phase `φ`, through `table`: `sin(offset + φ)` in
/// `[-1, 1]`.
///
/// The offset `OFFSET_STEP · (col + 2·row)` is the source's `x + 2y`; the sine is read from the
/// table, not computed, so a whole grid of these is a handful of lookups a frame. The offset can
/// be a thousand radians at the far corner, which the table reduces onto the ring exactly — the
/// large offset is the point, not a hazard (see [`OFFSET_STEP`]).
pub fn amplitude(col: u16, row: u16, phi: f32, table: &SinTable) -> f32 {
    let offset: f32 = OFFSET_STEP * (col as f32 + 2.0 * row as f32);
    table.sin(offset + phi)
}

/// Every cell of the grid at phase `φ`: [`amplitude`] over the whole [`COLS`]×[`ROWS`] grid.
///
/// Yielded column-major (a column's rows before the next column), the order the source's
/// `for(x)for(y)` loops draw in — which matters because a negative-breath square is drawn inverted
/// and spills over its neighbour, so the later cell must land on top exactly as the source layers
/// them. Borrows `table` for the life of the iterator, the render loop's shape: build the table
/// once, sweep it each frame.
pub fn cells<'a>(phi: f32, table: &'a SinTable) -> impl Iterator<Item = Cell> + 'a {
    (0..COLS).flat_map(move |col: u16| {
        (0..ROWS).map(move |row: u16| Cell {
            col,
            row,
            breath: amplitude(col, row, phi, table),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The breath is a raw sine, so it agrees with an `f64` reference to within the table's own
    /// tolerance — the fidelity the whole port rests on. Looser than the table's `1e-3` only to
    /// leave margin for the `f32` rounding of a thousand-radian offset.
    const TOLERANCE: f32 = 2e-3;

    /// The `f64` ground truth: the source's `sin(x + 2y + f)` computed in full precision, the
    /// value the table-and-`f32` [`amplitude`] is checked against.
    fn reference(col: u16, row: u16, phi: f64) -> f64 {
        (OFFSET_STEP as f64 * (col as f64 + 2.0 * row as f64) + phi).sin()
    }

    /// Zero: at phase zero the origin cell's breath is `sin(0)` — zero. The grid opens closed.
    #[test]
    fn the_origin_cell_is_closed_at_phase_zero() {
        assert!(amplitude(0, 0, 0.0, &SinTable::new()).abs() < TOLERANCE);
    }

    /// One: a single cell breathes as the phase advances — the same cell at two phases is two
    /// different breaths, so a grid that ignored `φ` could never pass.
    #[test]
    fn a_cell_breathes_with_the_phase() {
        let table: SinTable = SinTable::new();
        assert_ne!(amplitude(2, 3, 0.0, &table), amplitude(2, 3, 1.0, &table));
    }

    /// Many: the grid is exactly [`COLS`]×[`ROWS`] cells, each at its own place — counted, and
    /// checked that every grid position appears once.
    #[test]
    fn the_grid_is_every_cell_once() {
        let table: SinTable = SinTable::new();
        let cells: alloc::vec::Vec<Cell> = cells(0.0, &table).collect();
        assert_eq!(cells.len(), COLS as usize * ROWS as usize);
        for col in 0..COLS {
            for row in 0..ROWS {
                let hits: usize = cells
                    .iter()
                    .filter(|c: &&Cell| c.col == col && c.row == row)
                    .count();
                assert_eq!(hits, 1, "cell ({col}, {row}) must appear exactly once");
            }
        }
    }

    /// The grid is yielded column-major — a column's rows in order before the next column — so the
    /// display layers overlapping inverted squares exactly as the source's `for(x)for(y)` does.
    #[test]
    fn the_grid_is_column_major() {
        let table: SinTable = SinTable::new();
        let first_three: alloc::vec::Vec<(u16, u16)> = cells(0.0, &table)
            .take(3)
            .map(|c: Cell| (c.col, c.row))
            .collect();
        assert_eq!(first_three, [(0, 0), (0, 1), (0, 2)]);
    }

    proptest! {
        /// Many: across the whole grid and a full turn of phase, the table-and-`f32` breath tracks
        /// the `f64` reference within tolerance. A regression in the table reduction or the offset
        /// arithmetic would drift a cell's breath and band the wave; this pins it everywhere the
        /// animation drives it.
        #[test]
        fn the_breath_tracks_the_reference(
            col in 0u16..COLS,
            row in 0u16..ROWS,
            phi in 0.0f32..core::f32::consts::TAU,
        ) {
            let got: f32 = amplitude(col, row, phi, &SinTable::new());
            let want: f64 = reference(col, row, phi as f64);
            prop_assert!((got as f64 - want).abs() < TOLERANCE as f64, "{got} vs {want}");
        }
    }
}
