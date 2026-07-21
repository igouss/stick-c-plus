//! The board-generic signed bar — one `-full..=+full` value as a bar growing from a centre.
//!
//! A [`sparkline`](crate::sparkline) answers "how has this value moved over time?". This
//! primitive answers a different question: "how far, and *which way*, is this one value from
//! zero right now?" — the shape a signed axis wants. The bar starts at the middle of `area`
//! and grows right for a positive value, left for a negative one, so the sign is legible as
//! direction before the eye reaches a number.
//!
//! Like the sparkline it knows nothing about *what* the value means: it takes a raw `value`
//! and the `full` scale that value saturates at, never an app's domain type.
//!
//! ## One window, erased in place
//!
//! The whole `area` is painted with a single
//! [`fill_contiguous`](DrawTarget::fill_contiguous) streaming pixels row-major: every pixel
//! is written each call — `ink` inside the bar, `axis` on the centre line, `background`
//! elsewhere — so a bar that shrank, or swung across zero, has its old extent overwritten in
//! the same pass. That is what lets a bar track a live sensor without a clear and therefore
//! without a flash, and it avoids the per-primitive draws that cost a measured 85 ms a frame
//! on the real panel (see [`draw_onto`](crate::sprite::draw_onto)).
//!
//! ## The centre line always shows
//!
//! Zero is drawn as the bare `axis` tick rather than as nothing at all. An axis that vanished
//! at zero would make "the sensor reads zero" and "the sensor stopped reporting" the same
//! picture, and those are different facts.
//!
//! ## Integer only
//!
//! The bar's extent is `|value| * half / full`, computed in `i64` so a large `full` scale
//! cannot overflow the multiply, and clamped to `half`. No float, no `libm`, no rounding to
//! reason about — the same discipline the sparkline holds to.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

use crate::error::RenderError;

/// Draw `value` as a bar growing from the centre of `area`, saturating at `±full`.
///
/// A positive value grows right of centre, a negative one left; `full` is the magnitude that
/// fills the respective half, and anything beyond it is clamped to a full half-bar rather
/// than escaping the area. A `full` of zero draws the bare axis — an undefined scale has no
/// meaningful extent, so it reports nothing instead of dividing by zero.
///
/// Every pixel of `area` is written, so this both draws the bar and erases whatever it
/// replaced. `area` is drawn wherever the caller places it — a bar pushed off the canvas
/// escapes rather than clipping, the same contract as [`sparkline`](crate::sparkline);
/// keeping it on-screen is the caller's job.
pub fn signed_bar<D>(
    target: &mut D,
    area: Rectangle,
    value: i32,
    full: i32,
    ink: Rgb565,
    axis: Rgb565,
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

    // The centre column the bar grows from, and the widest extent either side of it.
    let centre: u32 = width / 2;
    let half: u32 = centre;

    // |value| as a share of the half-width, clamped so an over-range reading is a full
    // half-bar rather than a run past the edge. i64 throughout: `|value| * half` would
    // overflow an i32 for a large scale and a wide bar.
    let magnitude: i64 = i64::from(value).abs();
    let extent: u32 = match full {
        // An undefined scale has no extent — draw the axis alone rather than divide by zero.
        0 => 0,
        full => {
            let scaled: i64 = magnitude * i64::from(half) / i64::from(full).abs();
            scaled.min(i64::from(half)) as u32
        }
    };

    // The half-open column span the bar covers. A positive value runs right from the centre,
    // a negative one left; a zero-extent value covers nothing and leaves the bare axis.
    let (from, to): (u32, u32) = if value >= 0 {
        (centre, centre.saturating_add(extent))
    } else {
        (centre.saturating_sub(extent), centre)
    };

    // Row-major, matching `Rectangle::points()`, so pixel N of this iterator lands on point N
    // of the area — the same ordering the sparkline and `draw_onto` rely on.
    let colours = (0..height).flat_map(move |_row: u32| {
        (0..width).map(move |col: u32| {
            if col >= from && col < to {
                ink
            } else if col == centre {
                // The centre line, always drawn, so a zero reading is visibly a reading.
                axis
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

    /// The axis colour in these tests — distinct from the white ink, so the centre tick and
    /// the bar can be told apart pixel by pixel.
    const AXIS: Rgb565 = Rgb565::new(8, 16, 8);
    /// The scale every test measures against: one gravity in milli-g.
    const FULL: i32 = 1_000;

    /// A bar rectangle at the top-left of the canvas, `width`×`height`.
    fn bar_area(width: u32, height: u32) -> Rectangle {
        Rectangle::new(Point::new(0, 0), Size::new(width, height))
    }

    /// Draw `value` into a fresh framebuffer over a `width`×`height` bar, and hand it back.
    fn drawn(width: u32, height: u32, value: i32) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        signed_bar(
            &mut fb,
            bar_area(width, height),
            value,
            FULL,
            Rgb565::WHITE,
            AXIS,
            Rgb565::BLACK,
        )
        .expect("a framebuffer render cannot fail");
        fb
    }

    /// How many pixels carry the bar's ink specifically (not the axis tick).
    fn ink_pixels(fb: &Framebuffer) -> usize {
        fb.pixels()
            .iter()
            .filter(|colour: &&Rgb565| **colour == Rgb565::WHITE)
            .count()
    }

    /// Zero: a zero reading draws no bar — but still shows its axis, so "reads zero" and
    /// "reports nothing" are different pictures.
    #[test]
    fn a_zero_value_draws_only_the_axis() {
        let fb: Framebuffer = drawn(100, 8, 0);
        assert_eq!(ink_pixels(&fb), 0, "a zero value must draw no bar");
        assert_eq!(fb.lit_pixels(), 8, "the axis line must still be drawn");
        assert_eq!(fb.escaped(), 0);
    }

    /// One: a full positive reading fills the whole right half.
    #[test]
    fn a_full_positive_value_fills_the_right_half() {
        // 100 wide → centre at 50, half-width 50; 8 rows.
        assert_eq!(ink_pixels(&drawn(100, 8, FULL)), 50 * 8);
    }

    /// One: a full negative reading fills the whole left half — the same extent, mirrored.
    #[test]
    fn a_full_negative_value_fills_the_left_half() {
        assert_eq!(ink_pixels(&drawn(100, 8, -FULL)), 50 * 8);
    }

    /// The sign is *direction*, not just magnitude: equal and opposite readings are the same
    /// amount of ink in different places. A bar that ignored the sign would pass every
    /// pixel-count test above and still be wrong.
    #[test]
    fn opposite_values_paint_the_same_ink_on_opposite_sides() {
        let positive: Framebuffer = drawn(100, 8, FULL / 2);
        let negative: Framebuffer = drawn(100, 8, -FULL / 2);
        assert_eq!(ink_pixels(&positive), ink_pixels(&negative));
        assert_ne!(
            positive.pixels(),
            negative.pixels(),
            "a positive and a negative reading must not paint the same picture"
        );
    }

    /// Many: the extent is proportional — a half reading is half the half-width.
    #[test]
    fn a_half_value_fills_half_of_its_side() {
        assert_eq!(ink_pixels(&drawn(100, 8, FULL / 2)), 25 * 8);
    }

    /// An over-range reading is clamped to a full half-bar, never past the edge.
    #[test]
    fn an_over_range_value_is_clamped_and_does_not_escape() {
        let fb: Framebuffer = drawn(100, 8, FULL * 9);
        assert_eq!(ink_pixels(&fb), 50 * 8);
        assert_eq!(fb.escaped(), 0);
    }

    /// The extreme reading saturates rather than overflowing the scaling multiply.
    #[test]
    fn an_extreme_value_does_not_overflow_the_scaling() {
        let fb: Framebuffer = drawn(100, 8, i32::MIN);
        assert_eq!(ink_pixels(&fb), 50 * 8);
        assert_eq!(fb.escaped(), 0);
    }

    /// A zero scale draws the bare axis instead of dividing by zero.
    #[test]
    fn a_zero_scale_draws_only_the_axis() {
        let mut fb: Framebuffer = Framebuffer::new();
        signed_bar(
            &mut fb,
            bar_area(100, 8),
            500,
            0,
            Rgb565::WHITE,
            AXIS,
            Rgb565::BLACK,
        )
        .expect("a framebuffer render cannot fail");
        assert_eq!(ink_pixels(&fb), 0);
        assert_eq!(fb.lit_pixels(), 8);
    }

    /// The erase-in-place guarantee, and the whole reason this is one `fill_contiguous`: a
    /// bar that swings across zero must have its old side overwritten, not left behind.
    #[test]
    fn a_bar_swinging_across_zero_erases_its_old_side() {
        let mut fb: Framebuffer = Framebuffer::new();
        let area: Rectangle = bar_area(100, 8);
        let paint = |fb: &mut Framebuffer, value: i32| {
            signed_bar(fb, area, value, FULL, Rgb565::WHITE, AXIS, Rgb565::BLACK)
                .expect("a framebuffer render cannot fail");
        };

        paint(&mut fb, FULL);
        assert_eq!(ink_pixels(&fb), 50 * 8, "the positive bar is drawn");

        paint(&mut fb, -FULL / 2);
        assert_eq!(
            ink_pixels(&fb),
            25 * 8,
            "the old positive bar was not erased — the axis would smear as the board turns"
        );
    }

    /// A bar placed off the right edge escapes rather than silently clipping — the caller,
    /// not this primitive, owns keeping it on the canvas.
    #[test]
    fn a_bar_off_the_canvas_escapes() {
        let mut fb: Framebuffer = Framebuffer::new();
        let area: Rectangle = Rectangle::new(
            Point::new(SCREEN_SIZE.width as i32 - 2, 0),
            Size::new(40, 4),
        );
        signed_bar(
            &mut fb,
            area,
            FULL,
            FULL,
            Rgb565::WHITE,
            AXIS,
            Rgb565::BLACK,
        )
        .expect("draw");
        assert!(fb.escaped() > 0, "columns past the right edge must escape");
    }
}
