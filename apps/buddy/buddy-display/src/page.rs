//! A page: a title and a run of text rows, filling the canvas.
//!
//! The pet screen's how-to, all six info pages, and the two text overlays are the same picture
//! with different words — a heading, then lines, then blank rows down to the bottom edge so the
//! previous page erases in place. Writing that once is what keeps nine pages from becoming nine
//! subtly different margins.
//!
//! The body is [`wrap`]ped, so a page is written as sentences and lands correctly on both the
//! twenty-four-column canvas and the thirteen-column one — the same words, re-flowed, rather
//! than two hand-broken copies that drift apart the first time one is edited.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_display::{text_field, RenderError};

use crate::layout::Layout;
use crate::palette;
use crate::wrap::wrap;

/// The row the title sits on.
const TITLE_ROW: usize = 0;

/// The first row of the body.
const BODY_ROW: usize = 1;

/// Draw a titled page of wrapped text, blanking the rest of the canvas.
///
/// `body` is one run of text; `\n` starts a new line so a page can be a short list without the
/// caller doing its own wrapping.
pub fn render<D>(
    target: &mut D,
    layout: &Layout,
    title: &str,
    colour: Rgb565,
    body: &str,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_field(
        target,
        layout.row_origin(TITLE_ROW),
        colour,
        layout.cols,
        layout.align,
        format_args!("{title}"),
    )?;
    lines(target, layout, BODY_ROW, layout.rows - BODY_ROW, body)
}

/// Draw `body` into `rows` rows starting at `first`, wrapped, and blank whatever is left.
///
/// Public so the pet screen's stats page — which owns its own top rows for the meters — reuses
/// the same blanking discipline for the rows below them.
pub fn lines<D>(
    target: &mut D,
    layout: &Layout,
    first: usize,
    rows: usize,
    body: &str,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let wrapped = body
        .split('\n')
        .flat_map(|paragraph: &str| wrap(paragraph, layout.cols))
        .take(rows);

    let mut used: usize = 0;
    for (row, line) in wrapped.enumerate() {
        text_field(
            target,
            layout.row_origin(first + row),
            palette::DIM,
            layout.cols,
            layout.align,
            format_args!("{line}"),
        )?;
        used = row + 1;
    }
    blank(target, layout, first + used, rows - used)
}

/// Paint `rows` empty full-width fields from `first` — the erase-in-place that lets a page
/// replace a longer one without a full-screen clear, and therefore without a flash.
pub fn blank<D>(
    target: &mut D,
    layout: &Layout,
    first: usize,
    rows: usize,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    (0..rows).try_for_each(|row: usize| {
        text_field(
            target,
            layout.row_origin(first + row),
            palette::PRIMARY,
            layout.cols,
            layout.align,
            format_args!(""),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{LANDSCAPE, PORTRAIT};
    use platform_display::testing::Framebuffer;

    fn painted(layout: &Layout, title: &str, body: &str) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::sized(layout.canvas);
        render(&mut fb, layout, title, palette::TITLE, body)
            .expect("a framebuffer render cannot fail");
        fb
    }

    /// Zero: a page with no body is still a page — the title reaches the glass.
    #[test]
    fn a_page_with_no_body_still_shows_its_title() {
        assert!(painted(&LANDSCAPE, "ABOUT", "").lit_pixels() > 0);
    }

    /// One, and many: the title and the body both reach the glass.
    #[test]
    fn the_title_and_the_body_both_reach_the_glass() {
        assert_ne!(
            painted(&LANDSCAPE, "ABOUT", "").pixels(),
            painted(&LANDSCAPE, "ABOUT", "a desk pet").pixels()
        );
        assert_ne!(
            painted(&LANDSCAPE, "ABOUT", "a desk pet").pixels(),
            painted(&LANDSCAPE, "BUTTONS", "a desk pet").pixels()
        );
    }

    /// A shorter page erases the longer one it replaced, with no full-screen clear.
    #[test]
    fn a_shorter_page_erases_the_longer_one() {
        let mut fb: Framebuffer = Framebuffer::new();
        render(
            &mut fb,
            &LANDSCAPE,
            "ABOUT",
            palette::TITLE,
            "a great deal of text that will certainly fill several rows of this small panel",
        )
        .expect("a framebuffer render cannot fail");
        render(&mut fb, &LANDSCAPE, "ABOUT", palette::TITLE, "short")
            .expect("a framebuffer render cannot fail");
        assert_eq!(fb.pixels(), painted(&LANDSCAPE, "ABOUT", "short").pixels());
    }

    /// A body longer than the page is cut at the last row rather than drawn off the bottom.
    #[test]
    fn an_overlong_body_stays_on_both_canvases() {
        let flood: &str = "one two three four five six seven eight nine ten eleven twelve \
                           thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty";
        assert_eq!(painted(&LANDSCAPE, "FLOOD", flood).escaped(), 0);
        assert_eq!(painted(&PORTRAIT, "FLOOD", flood).escaped(), 0);
    }

    /// An explicit newline starts a new row, so a page can be a short list without the caller
    /// wrapping it by hand.
    #[test]
    fn a_newline_starts_a_new_row() {
        assert_ne!(
            painted(&LANDSCAPE, "BUTTONS", "A allow B deny").pixels(),
            painted(&LANDSCAPE, "BUTTONS", "A allow\nB deny").pixels()
        );
    }
}
