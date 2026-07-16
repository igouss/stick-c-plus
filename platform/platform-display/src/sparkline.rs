//! The board-generic bar-sparkline — a strip of `0..=100` values as a filled graph.
//!
//! A scrolling usage graph is a column of bars: one value per column, each bar rising
//! from the bottom of the plot to its value's share of the height. This primitive
//! draws exactly that into any [`DrawTarget`], knowing nothing about *what* the values
//! mean — the host monitor's CPU and memory series, or any future app's — so it takes
//! a raw `&[u8]` the way [`text_line`](crate::text_line) takes
//! [`Arguments`](core::fmt::Arguments), never an app's domain type.
//!
//! ## One window, erased in place
//!
//! Like [`draw_onto`](crate::sprite::draw_onto), the whole plot is painted with a
//! single [`fill_contiguous`](DrawTarget::fill_contiguous) streaming pixels row-major:
//! every pixel in `area` is written each call — `ink` inside a bar, `background`
//! elsewhere — so a bar that shrank between frames has its old height overwritten with
//! `background` in the same pass. That is what lets the graph scroll without a clear
//! and therefore without a flash, exactly as a shorter `text_line` value erases the
//! longer one it replaces. It also avoids the per-primitive `Line`/`Polyline` draws
//! that cost 400 address windows a frame on a real panel — the measured 85 ms mistake
//! [`draw_onto`](crate::sprite::draw_onto) documents.
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

/// Draw a bar-sparkline of `values` (each `0..=100`, oldest first) into `area`.
///
/// One column per value, left to right, each bar rising from the bottom of `area`.
/// Values fill the columns from the left; once there are fewer values than columns the
/// remaining columns are painted `background` (an empty right edge the graph grows
/// into), and once the plot is full the whole width is bars. A value above `100` is
/// treated as a full-height bar.
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
    values: &[u8],
    ink: Rgb565,
    background: Rgb565,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let width: u32 = area.size.width;
    let height: u32 = area.size.height;
    if width == 0 || height == 0 {
        return Ok(());
    }

    // Columns that carry a bar: one per value, never past the plot's width.
    let columns: u32 = core::cmp::min(values.len() as u32, width);

    // Row-major, matching `Rectangle::points()`, so pixel N of this iterator lands on
    // point N of the area — the same ordering `draw_onto` relies on.
    let colours = (0..height).flat_map(move |row: u32| {
        (0..width).map(move |col: u32| {
            let is_ink: bool = col < columns && {
                let value: u32 = values[col as usize] as u32;
                // The bottom `value/100 * height` rows are ink. Cross-multiplied so the
                // test is exact integer arithmetic with no division. `height - row` is
                // in `1..=height` (row is `0..height`), so it never underflows.
                (height - row) * 100 <= value * height
            };
            if is_ink {
                ink
            } else {
                background
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

    /// A plot rectangle at the top-left of the canvas, `width`×`height`.
    fn plot(width: u32, height: u32) -> Rectangle {
        Rectangle::new(Point::new(0, 0), Size::new(width, height))
    }

    /// Draw `values` into a fresh framebuffer over a `width`×`height` plot, ink white on
    /// a black background, and hand it back for inspection.
    fn drawn(width: u32, height: u32, values: &[u8]) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        sparkline(
            &mut fb,
            plot(width, height),
            values,
            Rgb565::WHITE,
            Rgb565::BLACK,
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
        let fb: Framebuffer = drawn(1, 40, &[100]);
        assert_eq!(fb.lit_pixels(), 40);
        assert_eq!(fb.escaped(), 0);
    }

    /// One: a zero value paints nothing.
    #[test]
    fn a_zero_value_is_a_bare_column() {
        assert_eq!(drawn(1, 40, &[0]).lit_pixels(), 0);
    }

    /// One: a half value fills the bottom half — `50 * 40 / 100 = 20` rows.
    #[test]
    fn a_half_value_fills_half_the_height() {
        assert_eq!(drawn(1, 40, &[50]).lit_pixels(), 20);
    }

    /// Many: each column is filled to its own value, independently.
    #[test]
    fn many_values_fill_each_column_to_its_height() {
        // heights: 0, 50→5, 100→10  ⇒  0 + 5 + 10 = 15 lit pixels.
        let fb: Framebuffer = drawn(3, 10, &[0, 50, 100]);
        assert_eq!(fb.lit_pixels(), 15);
        assert_eq!(fb.escaped(), 0);
    }

    /// Fewer values than columns leave the right of the plot empty — the space the
    /// graph scrolls into. Two full bars over a five-wide plot light `2 * height`.
    #[test]
    fn a_short_series_leaves_the_right_empty() {
        let height: u32 = 10;
        let fb: Framebuffer = drawn(5, height, &[100, 100]);
        assert_eq!(fb.lit_pixels(), 2 * height as usize);
    }

    /// A value above 100 is clamped to a full-height bar, never taller than the plot.
    #[test]
    fn an_over_range_value_is_a_full_bar_not_an_overflow() {
        let fb: Framebuffer = drawn(1, 40, &[250]);
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
        sparkline(&mut fb, area, &[100], Rgb565::WHITE, Rgb565::BLACK).expect("tall bar");
        assert_eq!(fb.lit_pixels(), 40, "the spike is drawn");

        sparkline(&mut fb, area, &[10], Rgb565::WHITE, Rgb565::BLACK).expect("short bar");
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
        sparkline(&mut fb, area, &[100; 10], Rgb565::WHITE, Rgb565::BLACK).expect("draw");
        assert!(fb.escaped() > 0, "columns past the right edge must escape");
    }
}
