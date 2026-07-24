//! The board's canvas geometry, and the two text primitives every app draws through.
//!
//! Facts about the *picture* on this panel, not about the panel: how wide the rotated
//! canvas is, and how a fixed-width field is drawn so a shorter value erases the longer
//! one it replaces. The panel's own facts — the CGRAM offset, the colour inversion, the
//! SPI pins — stay in the driven adapter, the only party that knows them.
//!
//! Neither primitive assumes a layout: they take an explicit `origin` and (for
//! [`text_line`]) a field `width`, so the plant monitor's two rows and the pomodoro
//! timer's clock are the same primitive placed differently — their metrics cannot drift.

use core::fmt::Write as _;

use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle, MonoTextStyleBuilder};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Baseline, Text};

use crate::error::RenderError;

/// The drawable canvas, in landscape — what a [`DrawTarget`] sees.
///
/// The M5StickC Plus panel is natively 135×240 (portrait); the adapter rotates it 90°,
/// so everything drawn here works in 240×135. This is the single source of truth for that
/// size: the adapter derives its own native `display_size` by swapping these axes back, so
/// the two can never drift apart.
pub const SCREEN_SIZE: Size = Size::new(240, 135);

/// The one font every text line is drawn in — 10×20 monospace ASCII. Exposed so an app's
/// layout arithmetic (row spacing, right-edge fit) references the *same* glyph metrics the
/// renderer uses, rather than a copy that could disagree.
pub const FONT: MonoFont<'static> = FONT_10X20;

/// Stack capacity of a rendered line: the widest field any app pads to, plus headroom,
/// so a line is built with no heap allocation on the render path.
///
/// Thirty-two bytes, which is the panel's full landscape width (24 columns of the 10×20 font)
/// plus headroom. It was 16 while no app padded a field wider than half the glass; the buddy's
/// transcript HUD draws edge-to-edge, and a field that spans the panel is the widest any layout
/// on this board can ever ask for. Raising it only widens what is accepted — a `width` that fit
/// before still fits — and costs sixteen bytes of stack on the render path.
pub const LINE_CAP: usize = 32;

/// A stack-allocated line buffer. The render path formats into this instead of a heap
/// `String`, so a redraw allocates nothing on the ESP32's scarce SRAM.
type Line = heapless::String<LINE_CAP>;

/// Where a value sits inside the fixed field it is padded to.
///
/// The field is the same pixels either way — that is what erases the previous value — so this
/// only decides where the padding goes, not how much of it there is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FieldAlign {
    /// Flush left, all the padding trailing. The right choice when something sits to the
    /// field's right to align against, which is how every landscape layout on this board is
    /// built.
    #[default]
    Left,
    /// Centred, the padding split either side.
    ///
    /// For a stacked layout — a narrow canvas, nothing beside the field — where a left-flush
    /// short value inside a wide field reads as having drifted rather than as being aligned.
    /// An odd remainder goes to the right, so a field's contents never shift by a pixel
    /// between two values of the same length.
    Centred,
}

/// Draw one baseline-top text line at `origin`, padded to `width` characters with an
/// opaque black background so it overwrites its whole field in place.
///
/// Flush left; see [`text_field`] to place the value inside the field.
pub fn text_line<D>(
    target: &mut D,
    origin: Point,
    color: Rgb565,
    width: usize,
    content: core::fmt::Arguments<'_>,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_field(target, origin, color, width, FieldAlign::Left, content)
}

/// Draw one baseline-top text line at `origin`, padded to `width` characters with an
/// opaque black background so it overwrites its whole field in place, with the value
/// placed inside that field by `align`.
///
/// The opaque background is the whole mechanism by which a shorter new value's padding
/// erases the leftover glyphs of a longer old one — so a redraw touches only its own
/// field, with no full-screen clear and therefore no flash. **That is why the padding is
/// split rather than moved** for [`FieldAlign::Centred`]: the drawn string still spans the
/// whole field, so a centred `DONE` erases every pixel a `LONG BREAK` had left behind. A
/// centring that repositioned a shorter string would leave the ends of the longer one on the
/// glass.
///
/// `width` must be less than [`LINE_CAP`]; content that would exceed [`LINE_CAP`] is refused
/// as [`RenderError::LineOverflow`] rather than silently truncated.
///
/// A value wider than `width` is refused as [`RenderError::FieldOverflow`]. The field is the
/// caller's claim on a rectangle — what a backdrop leaves a hole for, and what a golden pins —
/// so a value that does not fit would be glyphs painted outside the claim, over a neighbour or
/// past the edge of a panel onto the screen beneath it. Refusing puts that mistake at the call
/// site, where the field width is chosen, instead of on the glass.
pub fn text_field<D>(
    target: &mut D,
    origin: Point,
    color: Rgb565,
    width: usize,
    align: FieldAlign,
    content: core::fmt::Arguments<'_>,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut line: Line = Line::new();
    // Two paths rather than one, and the split is deliberate: the left-flush case formats
    // STRAIGHT into the line buffer, exactly as this function did before centring existed.
    //
    // Measuring it is what forced the shape. A single path that formatted into a scratch
    // buffer and then copied it in — needed only to learn the value's length before placing
    // it — cost `host-monitor` 44 bytes, and `host-monitor` draws nothing but left-flush text
    // and opts into no rotation at all. `align` is a constant at every call site that cares,
    // so this match folds and the branch not taken is never generated. See the epic's
    // no-penalty requirement in docs/plans/screen-rotation-handoff.md.
    match align {
        FieldAlign::Left => {
            line.write_fmt(content)
                .map_err(|_| RenderError::LineOverflow)?;
            if line.len() > width {
                return Err(RenderError::FieldOverflow);
            }
        }
        FieldAlign::Centred => {
            let mut value: Line = Line::new();
            value
                .write_fmt(content)
                .map_err(|_| RenderError::LineOverflow)?;
            if value.len() > width {
                return Err(RenderError::FieldOverflow);
            }
            // Half the slack leads, and the remainder falls to the trailing pad below — so an
            // odd gap widens the right side rather than shifting the value off the glyph grid.
            let lead: usize = width.saturating_sub(value.len()) / 2;
            // Each `push` only fails at LINE_CAP, which a sane `width` is well inside.
            while line.len() < lead && line.push(' ').is_ok() {}
            line.push_str(value.as_str())
                .map_err(|_| RenderError::LineOverflow)?;
        }
    }
    // Pad to a fixed field so a shorter value fully erases the longer one it replaces.
    while line.len() < width && line.push(' ').is_ok() {}

    let style: MonoTextStyle<'_, Rgb565> = MonoTextStyleBuilder::new()
        .font(&FONT)
        .text_color(color)
        .background_color(Rgb565::BLACK)
        .build();
    Text::with_baseline(line.as_str(), origin, style, Baseline::Top)
        .draw(target)
        .map_err(RenderError::Draw)?;
    Ok(())
}

/// Draw a text line with a *transparent* background, over whatever is beneath.
///
/// The colour-check labels sit on top of their bands, so they must not paint a black box
/// around themselves — unlike [`text_line`], whose opaque background is the whole mechanism
/// by which a value erases its predecessor.
pub fn text_overlay<D>(
    target: &mut D,
    at: Point,
    color: Rgb565,
    label: &str,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let style: MonoTextStyle<'_, Rgb565> = MonoTextStyleBuilder::new()
        .font(&FONT)
        .text_color(color)
        .build();
    Text::with_baseline(label, at, style, Baseline::Top)
        .draw(target)
        .map_err(RenderError::Draw)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Framebuffer;

    /// The field both alignments are measured in, wide enough that a short value leaves a
    /// visible gap either side.
    const WIDTH: usize = 10;

    /// A single origin, so two renders differ only by what was asked of them.
    const ORIGIN: Point = Point::new(0, 0);

    fn painted(align: FieldAlign, value: &str) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        text_field(
            &mut fb,
            ORIGIN,
            Rgb565::WHITE,
            WIDTH,
            align,
            format_args!("{value}"),
        )
        .expect("a framebuffer render cannot fail");
        fb
    }

    /// The x of the leftmost lit pixel — where the value actually starts on the glass.
    fn ink_starts_at(fb: &Framebuffer) -> u32 {
        let width: u32 = fb.size().width;
        (0..width)
            .find(|x: &u32| (0..fb.size().height).any(|y: u32| fb.pixel(*x, y) != Rgb565::BLACK))
            .expect("the value put ink on the glass")
    }

    /// Centring moves a short value in by exactly half its slack, in whole character cells.
    ///
    /// Measured as the *difference* between the two alignments rather than against the origin:
    /// `FONT_10X20` gives its glyphs a left side bearing, so even a flush-left `D` starts a
    /// pixel or so inside its cell. The gap between the two renders is free of that offset,
    /// because both pay it.
    #[test]
    fn centring_moves_a_short_value_in_by_half_its_slack() {
        let value: &str = "DONE";
        let cells: u32 = (WIDTH - value.len()) as u32 / 2;
        assert_eq!(cells, 3, "a four-character value in a ten-wide field");
        assert_eq!(
            ink_starts_at(&painted(FieldAlign::Centred, value))
                - ink_starts_at(&painted(FieldAlign::Left, value)),
            cells * FONT.character_size.width,
            "centring should shift the value by whole cells, so it stays on the glyph grid"
        );
    }

    /// A value that exactly fills its field is drawn identically either way — there is no
    /// padding left to distribute, so the alignment cannot move it.
    #[test]
    fn a_value_that_fills_its_field_is_placed_identically_by_both() {
        let full: &str = "LONG BREAK";
        assert_eq!(full.len(), WIDTH);
        assert_eq!(
            painted(FieldAlign::Left, full).pixels(),
            painted(FieldAlign::Centred, full).pixels()
        );
    }

    /// One character past the field is refused, in both alignments — the boundary, because a
    /// field that holds `WIDTH` and refuses `WIDTH + 1` is the whole contract, and the
    /// off-by-one is the one that actually happens.
    ///
    /// This is the bug that shipped: a twelve-character title in an eleven-column panel was
    /// drawn in full, its last glyph landing outside the panel and onto the screen underneath.
    /// It was fixed once by shortening that one string, which left every other caller able to do
    /// the same thing. Refusing here is what makes it not happen again.
    #[test]
    fn a_value_wider_than_its_field_is_refused_in_either_alignment() {
        let over: &str = "LONG BREAKS";
        assert_eq!(over.len(), WIDTH + 1);
        for align in [FieldAlign::Left, FieldAlign::Centred] {
            let mut fb: Framebuffer = Framebuffer::new();
            let refused = text_field(
                &mut fb,
                ORIGIN,
                Rgb565::WHITE,
                WIDTH,
                align,
                format_args!("{over}"),
            );
            assert!(matches!(refused, Err(RenderError::FieldOverflow)));
            assert_eq!(fb.lit_pixels(), 0, "a refused field paints nothing at all");
        }
    }

    /// A zero-width field holds nothing, so every value overflows it — the degenerate case a
    /// layout arithmetic slip produces, and the one that must not paint at the origin anyway.
    #[test]
    fn a_field_of_no_columns_refuses_every_value() {
        let mut fb: Framebuffer = Framebuffer::new();
        let refused = text_field(
            &mut fb,
            ORIGIN,
            Rgb565::WHITE,
            0,
            FieldAlign::Left,
            format_args!("X"),
        );
        assert!(matches!(refused, Err(RenderError::FieldOverflow)));
        assert_eq!(fb.lit_pixels(), 0);
    }

    /// **The reason the padding is split rather than moved.** A centred short value must
    /// still erase every pixel of the longer value it replaces, so overpainting one with the
    /// other leaves exactly the short value's own picture — no fragment of the long one
    /// surviving at either end.
    #[test]
    fn a_centred_short_value_erases_the_longer_one_it_replaces() {
        let mut fb: Framebuffer = Framebuffer::new();
        let paint = |fb: &mut Framebuffer, value: &str| {
            text_field(
                fb,
                ORIGIN,
                Rgb565::WHITE,
                WIDTH,
                FieldAlign::Centred,
                format_args!("{value}"),
            )
            .expect("a framebuffer render cannot fail");
        };
        paint(&mut fb, "LONG BREAK");
        paint(&mut fb, "DONE");
        assert_eq!(
            fb.pixels(),
            painted(FieldAlign::Centred, "DONE").pixels(),
            "a fragment of the longer value survived the overpaint"
        );
    }

    /// `text_line` is the left-flush case of `text_field` and nothing more — stated as a test
    /// so the two cannot drift apart.
    #[test]
    fn text_line_is_the_left_flush_field() {
        let mut fb: Framebuffer = Framebuffer::new();
        text_line(&mut fb, ORIGIN, Rgb565::WHITE, WIDTH, format_args!("DONE"))
            .expect("a framebuffer render cannot fail");
        assert_eq!(fb.pixels(), painted(FieldAlign::Left, "DONE").pixels());
    }

    /// Content wider than the line buffer is refused rather than truncated, in both
    /// alignments — a truncated reading on the glass looks like a real, smaller value.
    #[test]
    fn content_beyond_the_line_buffer_is_refused_not_truncated() {
        let mut fb: Framebuffer = Framebuffer::new();
        // Derived from the cap rather than written out, so raising [`LINE_CAP`] cannot leave
        // this test quietly asserting nothing.
        let too_long: alloc::string::String = "x".repeat(LINE_CAP + 1);
        assert!(too_long.len() > LINE_CAP);
        assert!(matches!(
            text_field(
                &mut fb,
                ORIGIN,
                Rgb565::WHITE,
                WIDTH,
                FieldAlign::Centred,
                format_args!("{too_long}")
            ),
            Err(RenderError::LineOverflow)
        ));
    }
}
