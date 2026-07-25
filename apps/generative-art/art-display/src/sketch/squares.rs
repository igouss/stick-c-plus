//! The squares sketch: the breathing grid, sized to the panel and drawn as nested frames.
//!
//! A faithful move of the source *Dwitter* onto the glass. The picture is a pure function of the
//! elapsed clock: [`squares_core::phase`] turns the clock into a phase, [`squares_core::cells`]
//! turns the phase into the grid's breathing cells, and this module lays each cell onto the panel
//! as the source draws it — a white square whose side is the cell's breath, with a black square
//! half its size centred inside, leaving a square frame whose thickness and sign breathe.
//!
//! ## Fitting the grid to the glass
//!
//! The domain owns the grid's *structure* ([`COLS`](squares_core::COLS) ×
//! [`ROWS`](squares_core::ROWS) cells); this module owns its *pixels*. The cell is sized to fill
//! the panel's width — [`COLS`](squares_core::COLS) columns across the canvas — and the
//! [`ROWS`](squares_core::ROWS)-tall block is centred vertically, grazing the top and bottom
//! edges. The breath the domain hands back is a fraction in `[-1, 1]`; scaled by the pixel cell
//! size it becomes the white square's signed side, so a fully-open cell fills its cell exactly and
//! a negative one draws the frame flipped across its corner, just as the source's negative-width
//! `rect` does.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::{RgbColor, Size};
use squares_core::{cells, phase, Cell, SinTable, COLS, ROWS};

use crate::canvas::Canvas;
use platform_core::Tick;

/// The colour the breathing frames are drawn in — white, faithful to the source's `fill(W)`.
pub const SQUARE_COLOUR: Rgb565 = Rgb565::WHITE;

/// The colour of the square carved out of each frame's middle — the panel's black, the same
/// ground the canvas was flooded with, so the black square is a true hole in the white one.
pub const HOLE_COLOUR: Rgb565 = Rgb565::BLACK;

/// Draw the breathing grid at `elapsed` into `canvas`, which the caller has reset to the ground.
///
/// Sizes a cell to fill the canvas width across [`COLS`](squares_core::COLS) columns, centres the
/// [`ROWS`](squares_core::ROWS)-tall block, then draws each cell the domain yields: a white square
/// of the breath's signed side, then a black square half its size centred inside — the source's
/// two `rect`s, a breathing frame. Generic over the [`Canvas`], so the grid lands identically in a
/// host [`Frame`](crate::Frame) for the goldens and in the firmware's wire-order buffer on glass.
///
/// The cells arrive column-major, and each is drawn as it arrives, so a negative-breath square
/// that spills over its neighbour is layered exactly as the source's `for(x)for(y)` layers it.
pub fn render<C: Canvas>(canvas: &mut C, table: &SinTable, elapsed: Tick, size: Size) {
    let phi: f32 = phase(elapsed);
    let cell: i32 = size.width as i32 / COLS as i32;
    // Centre the grid block: the columns fill the width (centred if they do not divide it evenly),
    // and the taller-than-panel row block is centred so its ends graze the top and bottom edges.
    let origin_x: i32 = (size.width as i32 - COLS as i32 * cell) / 2;
    let origin_y: i32 = (size.height as i32 - ROWS as i32 * cell) / 2;

    cells(phi, table).for_each(|c: Cell| {
        let px: i32 = origin_x + c.col as i32 * cell;
        let py: i32 = origin_y + c.row as i32 * cell;
        let side: f32 = c.breath * cell as f32;
        // The white frame, then the black hole half its size centred inside it (offset a quarter
        // of the side on each axis) — the source's `rect(x,y,C)` and `rect(x+C/4,y+C/4,C/2)`.
        fill_square(canvas, px as f32, py as f32, side, SQUARE_COLOUR);
        fill_square(
            canvas,
            px as f32 + side / 4.0,
            py as f32 + side / 4.0,
            side / 2.0,
            HOLE_COLOUR,
        );
    });
}

/// Fill the axis-aligned square with a corner at `(x, y)` and signed `side` in `colour`.
///
/// A negative `side` extends up and left of the corner rather than down and right — the source's
/// negative-width `rect`, which is how the frame flips as its breath crosses zero. The bounds are
/// normalised so either sign fills the same pixels a positive square of that span would, and each
/// pixel goes through [`Canvas::set`], which clips anything off the canvas — a square flung past
/// the edge simply draws less.
fn fill_square<C: Canvas>(canvas: &mut C, x: f32, y: f32, side: f32, colour: Rgb565) {
    let x0: i32 = libm::roundf(x.min(x + side)) as i32;
    let x1: i32 = libm::roundf(x.max(x + side)) as i32;
    let y0: i32 = libm::roundf(y.min(y + side)) as i32;
    let y1: i32 = libm::roundf(y.max(y + side)) as i32;
    for row in y0..y1 {
        for col in x0..x1 {
            canvas.set(col, row, colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Frame;
    use crate::gallery::{canvas_size, GROUND_COLOUR};
    use platform_core::ScreenRotation;
    use platform_display::testing::Framebuffer;

    /// The portrait quarter turn the gallery is pinned to.
    const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

    /// Blit the grid at `elapsed` into a fresh portrait framebuffer — the whole host path.
    fn painted(elapsed: Tick) -> Framebuffer {
        let table: SinTable = SinTable::new();
        let canvas: Size = canvas_size(PORTRAIT);
        let mut frame: Frame = Frame::new();
        frame.reset(canvas, GROUND_COLOUR);
        render(&mut frame, &table, elapsed, canvas);
        let mut fb: Framebuffer = Framebuffer::sized(canvas);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        fb
    }

    /// One: the grid puts white ink on the glass — the frames are drawn, not skipped.
    #[test]
    fn the_grid_paints_pixels() {
        assert!(painted(0).lit_pixels() > 0);
    }

    /// Nothing the grid draws escapes the canvas — every cell, even a negative-breath one that
    /// spills over its neighbour, is clipped to the panel by [`Canvas::set`].
    #[test]
    fn nothing_escapes_the_canvas() {
        // A phase where breaths are large and several cells are inverted, so spill is at its worst.
        assert_eq!(painted(squares_core::PERIOD_MS / 3).escaped(), 0);
    }

    /// Many: three moments of the grid paint three distinct pictures — the sketch actually
    /// breathes, it does not freeze.
    #[test]
    fn the_grid_animates() {
        let a: Framebuffer = painted(0);
        let b: Framebuffer = painted(squares_core::PERIOD_MS / 4);
        let c: Framebuffer = painted(squares_core::PERIOD_MS / 2);
        assert_ne!(a.pixels(), b.pixels());
        assert_ne!(b.pixels(), c.pixels());
        assert_ne!(a.pixels(), c.pixels());
    }
}
