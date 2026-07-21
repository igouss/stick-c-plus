//! The rotation bring-up self-test — a picture that makes a wrong CGRAM window visible.
//!
//! Sibling of [`colour_bands`](crate::colour_bands), and it exists for the same reason: there
//! is a class of fault that lives *below* [`DrawTarget`], where no host render can reach it. A
//! framebuffer paints at the coordinates it is given; the glass paints wherever the
//! controller's address window happens to point. If that window is off by a few pixels — the
//! wrong CGRAM offset for the orientation currently set — every host test still passes and the
//! picture on the glass is shifted, or trailed by a stripe of whatever was in controller memory
//! before.
//!
//! So this frame is built to fail loudly rather than subtly. Its whole content is placed
//! against the canvas *edges*, because an offset error is a translation and a translation is
//! only obvious against a boundary. A picture centred in the canvas would slide several pixels
//! and look fine.
//!
//! It answers two questions at once, and they are genuinely different:
//!
//! - **Is the window aligned?** — the border. It is drawn on the outermost pixel ring of the
//!   canvas, so a correct window shows four complete edges flush with the bezel. A wrong one
//!   clips an edge away and opens a gap on the opposite side.
//! - **Is the picture the right way up?** — the `UP` marker and the rotation's name, drawn hard
//!   against the top edge and offset to one side. "Correct" means they are at the top *as the
//!   board is being held*, which is a question only a person holding it can answer.
//!
//! The corner ticks are the tie-breaker between "shifted" and "one edge is just hard to see":
//! four ticks means four corners are on the glass.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle};

use crate::error::RenderError;
use crate::text::text_overlay;

/// How long each corner tick is, in pixels, along both axes.
///
/// Short enough to read as a corner mark rather than as part of the border, long enough to
/// survive the few-pixel shift this test is looking for — an offset error that moved the
/// picture by less than this would leave the tick still visibly attached to its corner.
const TICK: i32 = 10;

/// Inset of the `UP` label from the top edge.
///
/// Deliberately small. The label's job is to be *near the boundary*, because that is where a
/// translation shows; a comfortably centred label would slide with the picture and still look
/// placed.
const LABEL_INSET: i32 = 4;

/// Paint the rotation frame: a full-canvas border, corner ticks, an `UP` marker hard against
/// the top edge, and `label` naming the rotation being shown.
///
/// Read the result on the glass, with the board held so `label`'s rotation *should* be upright:
///
/// - **A complete rectangle flush with all four bezel edges, `UP` at the top** — this
///   orientation's window and rotation are both right.
/// - **The border clipped on one edge, with a gap on the opposite edge** — the CGRAM offset is
///   wrong for this orientation. The picture is being written to the wrong part of controller
///   memory; the *rotation* may still be correct.
/// - **A stripe of noise or of the previous picture down one edge** — same fault, worse: the
///   window is not merely offset but is exposing controller memory the frame never wrote.
/// - **A clean, complete border but `UP` pointing at the wall, floor, or the wrong side** —
///   the window is right and the *rotation mapping* is wrong. This is the one that means the
///   phase between the app's rotation and the panel's is off by a quarter turn or its sign.
///
/// Those last two are the ones worth separating carefully: a bad offset and a bad rotation
/// have completely different fixes, and the border tells them apart.
pub fn rotation_frame<D>(target: &mut D, label: &str) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let size: Size = target.bounding_box().size;
    let (w, h): (i32, i32) = (size.width as i32, size.height as i32);
    let white: PrimitiveStyle<Rgb565> = PrimitiveStyle::with_stroke(Rgb565::WHITE, 1);

    // Start from a known-black canvas. Without this, a shifted window would leave the previous
    // frame's border on the glass and a reader could not tell which of the two they were
    // looking at — the stale edge is exactly the symptom being hunted.
    target.clear(Rgb565::BLACK).map_err(RenderError::Draw)?;

    // The border, on the outermost pixel ring. The full canvas size is right: an
    // embedded-graphics `Rectangle` spans `top_left ..= top_left + size - 1`, so this lands on
    // rows/columns `0` and `w-1`/`h-1` — the actual edges. Insetting it by a pixel "for safety"
    // would defeat the whole test, since a border that does not touch the boundary cannot show
    // a translation against it.
    Rectangle::new(Point::zero(), size)
        .into_styled(white)
        .draw(target)
        .map_err(RenderError::Draw)?;

    // Corner ticks, drawn *inward* along both axes from each corner, in green so they read as
    // separate from the white border even where they overlap it.
    let green: PrimitiveStyle<Rgb565> = PrimitiveStyle::with_stroke(Rgb565::GREEN, 1);
    let corners: [(Point, Point, Point); 4] = [
        // (corner, along-x end, along-y end)
        (Point::new(0, 0), Point::new(TICK, 0), Point::new(0, TICK)),
        (
            Point::new(w - 1, 0),
            Point::new(w - 1 - TICK, 0),
            Point::new(w - 1, TICK),
        ),
        (
            Point::new(0, h - 1),
            Point::new(TICK, h - 1),
            Point::new(0, h - 1 - TICK),
        ),
        (
            Point::new(w - 1, h - 1),
            Point::new(w - 1 - TICK, h - 1),
            Point::new(w - 1, h - 1 - TICK),
        ),
    ];
    corners
        .into_iter()
        .try_for_each(|(corner, along_x, along_y): (Point, Point, Point)| {
            Line::new(corner, along_x)
                .into_styled(green)
                .draw(target)
                .map_err(RenderError::Draw)?;
            Line::new(corner, along_y)
                .into_styled(green)
                .draw(target)
                .map_err(RenderError::Draw)
        })?;

    // `UP` against the top edge, and the rotation's name just below it. Both left-inset rather
    // than centred: a centred label is symmetric under a half turn, and a half turn is one of
    // the failures this is meant to catch.
    text_overlay(
        target,
        Point::new(LABEL_INSET + 2, LABEL_INSET),
        Rgb565::WHITE,
        "UP",
    )?;
    text_overlay(
        target,
        Point::new(LABEL_INSET + 2, LABEL_INSET + 14),
        Rgb565::CYAN,
        label,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Framebuffer;
    use crate::SCREEN_SIZE;

    fn painted() -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        rotation_frame(&mut fb, "DEG0").expect("a framebuffer render cannot fail");
        fb
    }

    /// The border reaches all four edges of the canvas. This is the property the whole test
    /// rests on: draw it anywhere short of the boundary and an offset error stops being
    /// visible. A host framebuffer cannot check the *panel's* window, but it can check that
    /// what we asked for actually touches the edges.
    #[test]
    fn the_border_touches_all_four_edges() {
        let fb: Framebuffer = painted();
        let (w, h): (u32, u32) = (SCREEN_SIZE.width, SCREEN_SIZE.height);

        // A pixel on each edge, taken at the midpoint so it is border rather than corner tick.
        assert_ne!(fb.pixel(w / 2, 0), Rgb565::BLACK, "top edge unpainted");
        assert_ne!(
            fb.pixel(w / 2, h - 1),
            Rgb565::BLACK,
            "bottom edge unpainted"
        );
        assert_ne!(fb.pixel(0, h / 2), Rgb565::BLACK, "left edge unpainted");
        assert_ne!(
            fb.pixel(w - 1, h / 2),
            Rgb565::BLACK,
            "right edge unpainted"
        );
    }

    /// All four corners carry a tick, so a reader counting corners on the glass is counting
    /// something the render actually drew.
    #[test]
    fn every_corner_carries_a_tick() {
        let fb: Framebuffer = painted();
        let (w, h): (u32, u32) = (SCREEN_SIZE.width, SCREEN_SIZE.height);
        // A few pixels in from each corner along the x axis: tick territory, and green.
        assert_eq!(fb.pixel(3, 0), Rgb565::GREEN);
        assert_eq!(fb.pixel(w - 4, 0), Rgb565::GREEN);
        assert_eq!(fb.pixel(3, h - 1), Rgb565::GREEN);
        assert_eq!(fb.pixel(w - 4, h - 1), Rgb565::GREEN);
    }

    /// The picture is not symmetric under a half turn. If it were, the on-glass test could not
    /// distinguish "upright" from "upside down" — which is one of the two things it is for.
    #[test]
    fn the_frame_is_asymmetric_under_a_half_turn() {
        let fb: Framebuffer = painted();
        let (w, h): (u32, u32) = (SCREEN_SIZE.width, SCREEN_SIZE.height);
        // The UP/label block sits in the top-left. Its 180° image is the bottom-right, which
        // must be empty — otherwise the two orientations would look alike.
        let top_left_ink: usize = (0..40u32)
            .flat_map(|x: u32| (0..30u32).map(move |y: u32| (x, y)))
            .filter(|&(x, y): &(u32, u32)| fb.pixel(x, y) != Rgb565::BLACK)
            .count();
        let bottom_right_ink: usize = (w - 40..w)
            .flat_map(|x: u32| (h - 30..h).map(move |y: u32| (x, y)))
            .filter(|&(x, y): &(u32, u32)| fb.pixel(x, y) != Rgb565::BLACK)
            .count();
        assert!(
            top_left_ink > bottom_right_ink * 2,
            "the frame must not look the same upside down (top-left {top_left_ink} ink vs \
             bottom-right {bottom_right_ink})"
        );
    }
}
