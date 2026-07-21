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
//! ## Built for the eye that has to read it
//!
//! The first version of this frame drew a **1-pixel** outline on the outermost ring. It was
//! correct and it was useless: on a 1.14″ panel behind a bezel, a hairline at the extreme edge
//! cannot be judged present-or-absent with any confidence, so it could not falsify anything.
//! Reported from the bench as "very thin, hard to see" — which is a failed instrument, not a
//! passed test.
//!
//! So this version asks only questions a person is reliably good at:
//!
//! - **Compare two thicknesses.** A thick [`FRAME`] band runs flush to all four edges. A
//!   translated window does not make it vanish, it makes it *lopsided* — thinner on one side,
//!   thicker opposite. Judging "are these two bands the same width?" is far easier than judging
//!   "is there a line here?", and it stays easy at a glance.
//! - **Name a colour.** Each corner carries a large filled square in its own colour. A missing
//!   or clipped square localises the shift immediately, and — because the four differ — naming
//!   which colour sits top-left states the rotation outright, with no reliance on reading text
//!   upside down.
//!
//! The two readings are independent: the bands answer "is the window aligned?", the corner
//! colours answer "is it the right way up?", and they fail for different reasons with different
//! fixes.
//!
//! Nothing here paints the interior. The band is drawn as four strips rather than as a white
//! fill with the middle blacked back in, because the fill-then-cover version flashes the whole
//! glass white between its two draws — visible as a flicker on every redraw, and indistinguishable
//! at a glance from the stale-memory artefact this instrument is meant to expose.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};

use crate::error::RenderError;
use crate::text::text_overlay;

/// Thickness of the frame band, in pixels.
///
/// Wide enough to read across a room and to make a lopsided frame obvious at a glance; narrow
/// enough that the interior still holds the labels on a 135 px axis. A shift smaller than this
/// still shows, because what is being compared is one band against the opposite one — a 3 px
/// translation turns 6 and 6 into 3 and 9, which is a visible difference of a third.
const FRAME: u32 = 6;

/// Side of each corner square, in pixels.
///
/// Deliberately much larger than [`FRAME`]: the corner squares are the coarse signal, meant to
/// be findable without looking for them, so that "one corner is missing" registers immediately.
const CORNER: u32 = 18;

/// Where the labels start, in from the frame.
const LABEL_INSET: i32 = FRAME as i32 + 4;

/// The corner colours, clockwise from the top-left **of the picture as drawn**.
///
/// Four different colours rather than four identical marks, because this doubles as the
/// which-way-up readout: a reader names the colour in one corner and the rotation follows,
/// without having to read text that may be upside down. Red leads because it is the one people
/// reach for first when asked "which corner".
const CORNERS: [(Rgb565, &str); 4] = [
    (Rgb565::RED, "RED"),
    (Rgb565::GREEN, "GRN"),
    (Rgb565::BLUE, "BLU"),
    (Rgb565::YELLOW, "YEL"),
];

/// Paint the rotation frame: a thick band flush to all four edges, four coloured corner
/// squares, and `label` naming the rotation being drawn.
///
/// Read the glass with the board held so `label`'s rotation *should* be upright:
///
/// **Is the window aligned?** Look at the band, not the corners.
///
/// - **Even thickness all the way round** — this orientation's CGRAM window is right.
/// - **Thinner on one side, thicker on the opposite side** — the window is offset by that
///   difference. The rotation may still be perfectly correct; these are separate faults.
/// - **A corner square clipped, or missing entirely** — the same fault, larger.
/// - **A stripe of noise or of the previous picture along an edge** — the window is not merely
///   offset but is exposing controller memory this frame never wrote.
///
/// **Is it the right way up?** Look at the corners, not the text.
///
/// - **`RED` top-left, `GREEN` top-right, `BLUE` bottom-right, `YELLOW` bottom-left** — the
///   rotation mapping is right.
/// - **Any other arrangement** — the mapping is out, and *which* colour sits top-left says by
///   how much: green means a quarter turn one way, yellow a quarter the other, blue a half.
///
/// That last line is why the colours are ordered rather than arbitrary. A reader who reports
/// only "red is bottom-right" has still given a complete and unambiguous answer.
pub fn rotation_frame<D>(target: &mut D, label: &str) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let size: Size = target.bounding_box().size;

    // Start black, then paint the band as four strips. The obvious alternative — fill the whole
    // canvas white and lay black back over the interior — is two draws instead of five and was
    // written that way first. It flashes: between the two fills the entire glass is white, and
    // at redraw rates a person is watching, that reads as a flicker on every frame. Reported
    // from the bench, and it matters twice over, because a flash is exactly what a stale-memory
    // stripe looks like — the artefact this instrument exists to make visible.
    //
    // So no pixel of the interior is ever painted. Only the band is touched.
    target.clear(Rgb565::BLACK).map_err(RenderError::Draw)?;

    let band: PrimitiveStyle<Rgb565> = PrimitiveStyle::with_fill(Rgb565::WHITE);
    let inner_h: u32 = size.height.saturating_sub(FRAME * 2);
    let strips: [(Point, Size); 4] = [
        // Top and bottom run the full width; the sides fill only what is left between them, so
        // no pixel is drawn twice and the corners have no seam.
        (Point::zero(), Size::new(size.width, FRAME)),
        (
            Point::new(0, (size.height - FRAME) as i32),
            Size::new(size.width, FRAME),
        ),
        (Point::new(0, FRAME as i32), Size::new(FRAME, inner_h)),
        (
            Point::new((size.width - FRAME) as i32, FRAME as i32),
            Size::new(FRAME, inner_h),
        ),
    ];
    strips
        .into_iter()
        .try_for_each(|(at, extent): (Point, Size)| {
            Rectangle::new(at, extent)
                .into_styled(band)
                .draw(target)
                .map_err(RenderError::Draw)
        })?;

    // The corner squares sit hard in the corners, overlapping the band. Clockwise from
    // top-left, matching CORNERS, so the colour order *is* the rotation readout.
    let placements: [Point; 4] = [
        Point::new(0, 0),
        Point::new((size.width - CORNER) as i32, 0),
        Point::new((size.width - CORNER) as i32, (size.height - CORNER) as i32),
        Point::new(0, (size.height - CORNER) as i32),
    ];
    CORNERS.iter().zip(placements).try_for_each(
        |((colour, _), at): (&(Rgb565, &str), Point)| {
            Rectangle::new(at, Size::new(CORNER, CORNER))
                .into_styled(PrimitiveStyle::with_fill(*colour))
                .draw(target)
                .map_err(RenderError::Draw)
        },
    )?;

    // The labels, inside the band on black. `UP` names the edge it sits against; the rotation
    // name is the stop being shown. Left-inset rather than centred — a centred label is
    // symmetric under a half turn, and a half turn is one of the errors being hunted.
    text_overlay(
        target,
        Point::new(LABEL_INSET + CORNER as i32, LABEL_INSET),
        Rgb565::WHITE,
        "UP",
    )?;
    text_overlay(
        target,
        Point::new(LABEL_INSET, LABEL_INSET + 20),
        Rgb565::CYAN,
        label,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Framebuffer;
    use crate::SCREEN_SIZE;

    /// Note what these tests *cannot* reach: whether the interior is painted on the way to the
    /// final picture. Fill-then-cover and four-strips leave a framebuffer in byte-identical
    /// states, so the flicker that motivated the second is invisible here — it exists only in
    /// the sequence of writes, which a framebuffer does not record. It was found on the glass
    /// and can only be re-found there. Same shape of blind spot as the sub-tick sleep.
    fn painted() -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        rotation_frame(&mut fb, "DEG0").expect("a framebuffer render cannot fail");
        fb
    }

    /// The band reaches every edge and is the thickness it claims to be, on all four sides.
    ///
    /// This is the property the whole instrument rests on. Draw the band short of the boundary
    /// and a translation stops being visible against it; draw it thinner than `FRAME` and the
    /// lopsidedness the reader is asked to judge is not there to judge.
    #[test]
    fn the_band_is_full_thickness_on_all_four_edges() {
        let fb: Framebuffer = painted();
        let (w, h): (u32, u32) = (SCREEN_SIZE.width, SCREEN_SIZE.height);
        // Sample along the middle of each edge, clear of the corner squares.
        let (mx, my): (u32, u32) = (w / 2, h / 2);

        (0..FRAME).for_each(|d: u32| {
            assert_eq!(fb.pixel(mx, d), Rgb565::WHITE, "top band thin at depth {d}");
            assert_eq!(
                fb.pixel(mx, h - 1 - d),
                Rgb565::WHITE,
                "bottom band thin at depth {d}"
            );
            assert_eq!(
                fb.pixel(d, my),
                Rgb565::WHITE,
                "left band thin at depth {d}"
            );
            assert_eq!(
                fb.pixel(w - 1 - d, my),
                Rgb565::WHITE,
                "right band thin at depth {d}"
            );
        });

        // ...and it stops there, so the interior is genuinely black and the band's width is
        // the number a reader is comparing against the opposite side.
        assert_eq!(
            fb.pixel(mx, FRAME),
            Rgb565::BLACK,
            "band thicker than FRAME"
        );
    }

    /// Every corner carries its own colour, in the documented clockwise order. A reader naming
    /// one colour and its position gives an unambiguous rotation, so this pins the contract
    /// that reading depends on.
    #[test]
    fn the_four_corners_carry_their_documented_colours() {
        let fb: Framebuffer = painted();
        let (w, h): (u32, u32) = (SCREEN_SIZE.width, SCREEN_SIZE.height);
        let probe: u32 = CORNER / 2;

        assert_eq!(fb.pixel(probe, probe), Rgb565::RED, "top-left");
        assert_eq!(fb.pixel(w - 1 - probe, probe), Rgb565::GREEN, "top-right");
        assert_eq!(
            fb.pixel(w - 1 - probe, h - 1 - probe),
            Rgb565::BLUE,
            "bottom-right"
        );
        assert_eq!(
            fb.pixel(probe, h - 1 - probe),
            Rgb565::YELLOW,
            "bottom-left"
        );
    }

    /// No two corners share a colour — the property that makes "which colour is top-left?" a
    /// complete answer rather than a partial one.
    #[test]
    fn no_two_corners_share_a_colour() {
        CORNERS
            .iter()
            .enumerate()
            .for_each(|(index, (colour, name)): (usize, &(Rgb565, &str))| {
                let duplicates: usize = CORNERS
                    .iter()
                    .filter(|(other, _): &&(Rgb565, &str)| other == colour)
                    .count();
                assert_eq!(duplicates, 1, "corner {index} ({name}) shares its colour");
            });
    }

    /// The picture is not symmetric under a half turn: the top-left and bottom-right corners
    /// differ. Were they alike, the instrument could not tell upright from upside down — which
    /// is half of what it is for.
    #[test]
    fn the_frame_is_asymmetric_under_a_half_turn() {
        let fb: Framebuffer = painted();
        let (w, h): (u32, u32) = (SCREEN_SIZE.width, SCREEN_SIZE.height);
        let probe: u32 = CORNER / 2;
        assert_ne!(
            fb.pixel(probe, probe),
            fb.pixel(w - 1 - probe, h - 1 - probe),
            "the frame must not look the same upside down"
        );
    }

    /// The interior survives on the narrow axis: the band and the corner squares must not eat
    /// the whole canvas in portrait, or the labels would have nowhere to go.
    #[test]
    fn the_interior_survives_the_narrow_axis() {
        let narrow: Size = Size::new(SCREEN_SIZE.height, SCREEN_SIZE.width);
        let mut fb: Framebuffer = Framebuffer::sized(narrow);
        rotation_frame(&mut fb, "DEG90").expect("a framebuffer render cannot fail");
        assert_eq!(
            fb.escaped(),
            0,
            "the portrait frame drew outside its own canvas"
        );
    }
}
