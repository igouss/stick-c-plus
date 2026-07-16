//! The board-generic bar-sparkline — a strip of `0..=100` values as a filled graph.
//!
//! A scrolling usage graph is a column of bars: one value per column, each bar rising
//! from the bottom of the plot to its value's share of the height. This primitive
//! draws exactly that into any [`DrawTarget`], knowing nothing about *what* the values
//! mean — the host monitor's CPU and memory series, or any future app's — so it takes
//! a raw `&[Option<u8>]` the way [`text_line`](crate::text_line) takes
//! [`Arguments`](core::fmt::Arguments), never an app's domain type.
//!
//! ## A gap is not a zero
//!
//! Each column is an [`Option`]: `Some(v)` is a reading and draws a bar, `None` is a
//! *gap* — a sample that was never taken (a missing scrape), which is a different fact
//! from a reading of `0`. On a bar graph a `0` reading is a bare column, so a gap drawn
//! as `0` would be invisible against it. Instead a gap draws a single-pixel `gap`-colour
//! tick on the baseline: distinct from an empty `0` column (nothing there) and from a
//! real bar (which rises in `ink`), so "no data here" reads differently from "zero
//! here". Keeping the gap through to the pixels is why the caller passes `Option`s and
//! not a lossy `&[u8]` that had already flattened them.
//!
//! ## One window, erased in place
//!
//! Like [`draw_onto`](crate::sprite::draw_onto), the whole plot is painted with a
//! single [`fill_contiguous`](DrawTarget::fill_contiguous) streaming pixels row-major:
//! every pixel in `area` is written each call — `ink` inside a bar, the `gap` tick on a
//! gap's baseline, `background` elsewhere — so a bar that shrank between frames has its
//! old height overwritten with `background` in the same pass. That is what lets the
//! graph scroll without a clear and therefore without a flash, exactly as a shorter
//! `text_line` value erases the longer one it replaces. It also avoids the per-primitive
//! `Line`/`Polyline` draws that cost 400 address windows a frame on a real panel — the
//! measured 85 ms mistake [`draw_onto`](crate::sprite::draw_onto) documents.
//!
//! ## Integer only
//!
//! A bar's height is `value * area.height / 100`, and a pixel is ink when it lies in
//! the bottom `value/100` of the plot — tested without a division as
//! `(area.height - row) * 100 <= value * area.height`, so there is no float, no
//! `libm`, and no rounding to reason about.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

use crate::error::RenderError;

/// Draw a bar-sparkline of `values` (each `Some(0..=100)` or a `None` gap, oldest first)
/// into `area`.
///
/// One column per value, left to right. A `Some(v)` draws a bar rising from the bottom of
/// `area` to `v`'s share of the height (a value above `100` is a full-height bar); a `None`
/// draws only a single `gap`-colour pixel on the baseline, so a gap is visible yet distinct
/// from both a bar and a bare `Some(0)` column. Values fill the columns from the left; once
/// there are fewer values than columns the remaining columns are painted `background` (an
/// empty right edge the graph grows into), and once the plot is full the whole width is used.
///
/// `values.len()` is expected to be `<= area.size.width` (the caller's invariant — the
/// host monitor pins its graph width to the history capacity at compile time); any
/// excess values past the width are simply not drawn.
///
/// Every pixel of `area` is written, so this both draws the graph and erases whatever
/// it replaced. `area` is drawn wherever the caller places it — a plot pushed off the
/// canvas escapes rather than clipping, the same contract as
/// [`draw_onto`](crate::sprite::draw_onto); keeping it on-screen is the caller's job.
pub fn sparkline<D>(
    target: &mut D,
    area: Rectangle,
    values: &[Option<u8>],
    ink: Rgb565,
    background: Rgb565,
    gap: Rgb565,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let width: u32 = area.size.width;
    let height: u32 = area.size.height;
    if width == 0 || height == 0 {
        return Ok(());
    }

    // Columns that carry a value: one per value, never past the plot's width.
    let columns: u32 = core::cmp::min(values.len() as u32, width);
    // The baseline row a gap tick sits on — the bottom-most row of the plot.
    let baseline: u32 = height - 1;

    // Row-major, matching `Rectangle::points()`, so pixel N of this iterator lands on
    // point N of the area — the same ordering `draw_onto` relies on.
    let colours = (0..height).flat_map(move |row: u32| {
        (0..width).map(move |col: u32| {
            if col >= columns {
                return background; // past the values — the empty right edge
            }
            match values[col as usize] {
                // The bottom `value/100 * height` rows are ink. Cross-multiplied so the
                // test is exact integer arithmetic with no division. `height - row` is
                // in `1..=height` (row is `0..height`), so it never underflows.
                Some(value) if (height - row) * 100 <= u32::from(value) * height => ink,
                // A gap: a single baseline tick, so "no data" is neither a bar nor the
                // bare column a `Some(0)` leaves.
                None if row == baseline => gap,
                _ => background,
            }
        })
    });

    target
        .fill_contiguous(&area, colours)
        .map_err(RenderError::Draw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Framebuffer;
    use crate::SCREEN_SIZE;

    /// A gap tick's colour in these tests — any non-background colour distinct from the
    /// white bar ink, so a gap and a bar can be told apart pixel by pixel.
    const GAP: Rgb565 = Rgb565::new(8, 16, 8);

    /// A plot rectangle at the top-left of the canvas, `width`×`height`.
    fn plot(width: u32, height: u32) -> Rectangle {
        Rectangle::new(Point::new(0, 0), Size::new(width, height))
    }

    /// Draw `values` into a fresh framebuffer over a `width`×`height` plot, ink white on
    /// a black background with a [`GAP`] tick colour, and hand it back for inspection.
    fn drawn(width: u32, height: u32, values: &[Option<u8>]) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        sparkline(
            &mut fb,
            plot(width, height),
            values,
            Rgb565::WHITE,
            Rgb565::BLACK,
            GAP,
        )
        .expect("a framebuffer render cannot fail");
        fb
    }

    /// Zero: an empty series paints only background — no ink, nothing off-canvas.
    #[test]
    fn an_empty_series_draws_no_bars() {
        let fb: Framebuffer = drawn(120, 40, &[]);
        assert_eq!(fb.lit_pixels(), 0);
        assert_eq!(fb.escaped(), 0);
    }

    /// One: a single full-height value fills its whole column.
    #[test]
    fn a_full_value_fills_its_column() {
        let fb: Framebuffer = drawn(1, 40, &[Some(100)]);
        assert_eq!(fb.lit_pixels(), 40);
        assert_eq!(fb.escaped(), 0);
    }

    /// One: a zero value paints nothing.
    #[test]
    fn a_zero_value_is_a_bare_column() {
        assert_eq!(drawn(1, 40, &[Some(0)]).lit_pixels(), 0);
    }

    /// One: a half value fills the bottom half — `50 * 40 / 100 = 20` rows.
    #[test]
    fn a_half_value_fills_half_the_height() {
        assert_eq!(drawn(1, 40, &[Some(50)]).lit_pixels(), 20);
    }

    /// A gap is not a zero: a `None` leaves a single baseline tick, where a `Some(0)`
    /// leaves the column bare — so "no data" is visibly distinct from "zero", the whole
    /// reason the primitive keeps the `Option` instead of flattening a gap to `0`.
    #[test]
    fn a_gap_is_distinct_from_a_zero() {
        let gap: Framebuffer = drawn(1, 40, &[None]);
        let zero: Framebuffer = drawn(1, 40, &[Some(0)]);
        assert_eq!(gap.lit_pixels(), 1, "a gap draws exactly one baseline tick");
        assert_eq!(zero.lit_pixels(), 0, "a zero draws nothing");
        assert_ne!(
            gap.pixels(),
            zero.pixels(),
            "a gap must not render the same as a zero"
        );
    }

    /// The gap tick sits on the baseline (the bottom row), not floating in the plot.
    #[test]
    fn a_gap_tick_is_on_the_baseline() {
        let height: u32 = 40;
        let fb: Framebuffer = drawn(1, height, &[None]);
        let lit: Vec<usize> = (0..fb.pixels().len())
            .filter(|i: &usize| fb.pixels()[*i] != Rgb565::BLACK)
            .collect();
        assert_eq!(lit.len(), 1);
        // One-wide plot at the origin, so the pixel's index is its row; the baseline is
        // the last row, `height - 1`.
        let row: u32 = lit[0] as u32 / SCREEN_SIZE.width;
        assert_eq!(row, height - 1, "the gap tick must be on the baseline row");
    }

    /// Many: each column is filled to its own value, independently.
    #[test]
    fn many_values_fill_each_column_to_its_height() {
        // heights: 0, 50→5, 100→10  ⇒  0 + 5 + 10 = 15 lit pixels.
        let fb: Framebuffer = drawn(3, 10, &[Some(0), Some(50), Some(100)]);
        assert_eq!(fb.lit_pixels(), 15);
        assert_eq!(fb.escaped(), 0);
    }

    /// Fewer values than columns leave the right of the plot empty — the space the
    /// graph scrolls into. Two full bars over a five-wide plot light `2 * height`.
    #[test]
    fn a_short_series_leaves_the_right_empty() {
        let height: u32 = 10;
        let fb: Framebuffer = drawn(5, height, &[Some(100), Some(100)]);
        assert_eq!(fb.lit_pixels(), 2 * height as usize);
    }

    /// A value above 100 is clamped to a full-height bar, never taller than the plot.
    #[test]
    fn an_over_range_value_is_a_full_bar_not_an_overflow() {
        let fb: Framebuffer = drawn(1, 40, &[Some(250)]);
        assert_eq!(fb.lit_pixels(), 40);
        assert_eq!(fb.escaped(), 0);
    }

    /// The erase-in-place guarantee, and the whole reason this is one `fill_contiguous`
    /// and not a set of `Line`s: a spike that recedes must have its tall bar overwritten
    /// with background, not left behind. Draw 100, then 10 over the same plot; only the
    /// short bar's `10 * 40 / 100 = 4` pixels may remain.
    #[test]
    fn a_receding_spike_erases_the_taller_bar() {
        let mut fb: Framebuffer = Framebuffer::new();
        let area: Rectangle = plot(1, 40);
        sparkline(
            &mut fb,
            area,
            &[Some(100)],
            Rgb565::WHITE,
            Rgb565::BLACK,
            GAP,
        )
        .expect("tall bar");
        assert_eq!(fb.lit_pixels(), 40, "the spike is drawn");

        sparkline(
            &mut fb,
            area,
            &[Some(10)],
            Rgb565::WHITE,
            Rgb565::BLACK,
            GAP,
        )
        .expect("short bar");
        assert_eq!(
            fb.lit_pixels(),
            4,
            "the tall bar was not erased — the graph would smear as it scrolls"
        );
    }

    /// A plot placed off the right edge escapes rather than silently clipping — the
    /// caller, not this primitive, owns keeping the graph on the canvas.
    #[test]
    fn a_plot_off_the_canvas_escapes() {
        let mut fb: Framebuffer = Framebuffer::new();
        let area: Rectangle = Rectangle::new(
            Point::new(SCREEN_SIZE.width as i32 - 2, 0),
            Size::new(10, 5),
        );
        sparkline(
            &mut fb,
            area,
            &[Some(100); 10],
            Rgb565::WHITE,
            Rgb565::BLACK,
            GAP,
        )
        .expect("draw");
        assert!(fb.escaped() > 0, "columns past the right edge must escape");
    }
}
