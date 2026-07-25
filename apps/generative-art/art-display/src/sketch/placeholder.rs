//! The honest placeholder every not-yet-built sketch draws: its title and "coming soon".
//!
//! The gallery's running order names five pieces; only the plume is built so far. Rather than show
//! a blank or — worse — a convincing fake for a piece that does not exist, an unbuilt sketch draws
//! this: its own title in an accent colour, and the words *coming soon* beneath. So the button
//! genuinely cycles five distinct screens on the glass from the first commit, and no one mistakes
//! a placeholder for a finished piece (the `goldens-cannot-see-the-picture` lesson: a green test
//! has shipped a fabricated screen here before — this one is fabricated *on purpose* and says so).
//!
//! Each sketch's own commit replaces its placeholder arm in the [`Gallery`](crate::Gallery)'s
//! dispatch with its real rasterisation; when the last lands, this module has no callers left.

use art_core::Sketch;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_display::{text_field, FieldAlign};

use crate::canvas::Canvas;

/// The dim grey the "coming soon" line is drawn in — quieter than the accent title above it.
const SUBTITLE_COLOUR: Rgb565 = Rgb565::new(10, 20, 10);

/// The field width, in characters, both lines are centred within: the full 135-column portrait
/// canvas at the 10-pixel font is thirteen glyphs.
const FIELD_WIDTH: usize = 13;

/// The top of the title line, chosen so the two-line block sits near the middle of the 240-row
/// portrait canvas.
const TITLE_TOP: i32 = 100;

/// The top of the "coming soon" line, one line height and a gap below the title.
const SUBTITLE_TOP: i32 = 125;

/// The sketch's title as it appears on the placeholder, and the accent colour it is drawn in.
///
/// A total match over [`Sketch`], so adding a piece to the running order without giving it a title
/// is a compile error, not a blank line on the glass. The plume has a title here for totality
/// though it is never placeheld — the [`Gallery`](crate::Gallery)'s dispatch draws it for real.
fn title(sketch: Sketch) -> (&'static str, Rgb565) {
    match sketch {
        Sketch::Plume => ("PLUME", Rgb565::WHITE),
        Sketch::Squares => ("SQUARES", Rgb565::CYAN),
        Sketch::Fan => ("FAN", Rgb565::MAGENTA),
        Sketch::Orbits => ("ORBITS", Rgb565::YELLOW),
        Sketch::Willow => ("WILLOW", Rgb565::GREEN),
    }
}

/// Draw `sketch`'s placeholder into `frame`, which the caller has already reset to the ground.
///
/// Generic over the [`Canvas`], so a placeholder draws the same into a host [`Frame`](crate::Frame)
/// and the firmware's wire-order buffer. Drawing into a canvas cannot fail below the graphics port
/// (its error is [`Infallible`](core::convert::Infallible)); the only errors `text_field` can return
/// are a field or line that does not fit, which would be a layout mistake fixed here rather than a
/// condition to handle — so they are asserted, not propagated.
pub fn render<C: Canvas>(frame: &mut C, sketch: Sketch, canvas: Size) {
    let (name, accent): (&str, Rgb565) = title(sketch);
    let centre_x: i32 = (canvas.width as i32 - FIELD_WIDTH as i32 * 10) / 2;

    text_field(
        frame,
        Point::new(centre_x, TITLE_TOP),
        accent,
        FIELD_WIDTH,
        FieldAlign::Centred,
        format_args!("{name}"),
    )
    .expect("the title fits its field");
    text_field(
        frame,
        Point::new(centre_x, SUBTITLE_TOP),
        SUBTITLE_COLOUR,
        FIELD_WIDTH,
        FieldAlign::Centred,
        format_args!("coming soon"),
    )
    .expect("the subtitle fits its field");
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

    /// Blit `sketch`'s placeholder into a fresh portrait framebuffer.
    fn painted(sketch: Sketch) -> Framebuffer {
        let canvas: Size = canvas_size(PORTRAIT);
        let mut frame: Frame = Frame::new();
        frame.reset(canvas, GROUND_COLOUR);
        render(&mut frame, sketch, canvas);
        let mut fb: Framebuffer = Framebuffer::sized(canvas);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        fb
    }

    /// One: a placeholder puts ink on the glass — the title and subtitle are drawn, not skipped.
    #[test]
    fn a_placeholder_paints_pixels() {
        assert!(painted(Sketch::Squares).lit_pixels() > 0);
    }

    /// Many: the four unbuilt sketches draw four distinct screens, so the button visibly moves
    /// between them rather than showing one repeated "coming soon".
    #[test]
    fn the_unbuilt_sketches_are_all_distinct() {
        let unbuilt: [Sketch; 4] = [Sketch::Squares, Sketch::Fan, Sketch::Orbits, Sketch::Willow];
        for (i, &a) in unbuilt.iter().enumerate() {
            for &b in &unbuilt[i + 1..] {
                assert_ne!(
                    painted(a).pixels(),
                    painted(b).pixels(),
                    "{a:?} and {b:?} drew the same placeholder"
                );
            }
        }
    }

    /// Nothing the placeholder draws escapes the canvas — the centred fields sit inside the panel,
    /// so no glyph is clipped off an edge onto a neighbour.
    #[test]
    fn no_placeholder_text_escapes_the_canvas() {
        assert_eq!(painted(Sketch::Orbits).escaped(), 0);
    }
}
