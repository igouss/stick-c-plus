//! Canvas geometry, and the one text primitive both screens are built from.
//!
//! These are facts about the *picture*, not about the panel: how wide the canvas is
//! once rotated, where the two text rows sit, how wide a field a number is padded
//! to. The panel's own facts — the CGRAM offset, the colour inversion, the SPI
//! pins — stay in the driven adapter, which is the only party that knows them.

use core::fmt::Write as _;

use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::{MonoTextStyle, MonoTextStyleBuilder};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Baseline, Text};

use crate::error::RenderError;

/// The drawable canvas, in landscape — what a [`DrawTarget`] sees.
///
/// The M5StickC Plus panel is natively 135×240 (portrait); the adapter rotates it
/// 90°, so everything drawn here works in 240×135. This is the single source of
/// truth for that size: the adapter derives its own native `display_size` by
/// swapping these axes back, so the two can never drift apart.
pub const SCREEN_SIZE: Size = Size::new(240, 135);

/// Baseline-top x inset of both text lines.
pub const TEXT_X: i32 = 8;
/// Baseline-top y of the upper line (the raw count, or the fault headline).
pub const RAW_Y: i32 = 34;
/// Baseline-top y of the lower line (the percent, or the fault reason).
pub const PCT_Y: i32 = 80;

/// Fixed character width both lines are padded to.
///
/// A shorter new value's trailing spaces — painted with the opaque background —
/// erase the leftover glyphs of a longer old one. That is what lets a redraw touch
/// only its own two rows: no full-screen clear, and therefore no per-update flash.
pub const LINE_WIDTH: usize = 10;

/// Stack capacity of a rendered line: [`LINE_WIDTH`] plus headroom, so a line is
/// built with no heap allocation on the render path.
const LINE_CAP: usize = 16;

/// Panel pixels per sprite cell. A 20×20 creature becomes 100×100.
pub const SPRITE_SCALE: u32 = 5;
/// Top-left corner of the creature: the right-hand third of the panel, which the two
/// text rows never reach.
pub const SPRITE_ORIGIN: Point = Point::new(132, 17);

/// A stack-allocated line buffer. The render path formats into this instead of a
/// heap `String`, so a redraw allocates nothing on the ESP32's scarce SRAM.
type Line = heapless::String<LINE_CAP>;

/// The geometry invariants, checked by the **compiler**.
///
/// Every term here is a constant, so a portrait canvas, a pair of overlapping rows, or
/// a line that runs off an edge fails the *build* — on the host and on the Xtensa
/// target alike. That is strictly stronger than a test, which can be filtered, skipped,
/// or simply not run before a flash. Test what varies; assert what cannot.
const _: () = {
    let cell_width: u32 = FONT_10X20.character_size.width;
    let cell_height: u32 = FONT_10X20.character_size.height;

    assert!(
        SCREEN_SIZE.width > SCREEN_SIZE.height,
        "the canvas is rotated 90° from the panel's native portrait: it must be landscape"
    );
    assert!(
        TEXT_X as u32 + cell_width * LINE_WIDTH as u32 <= SCREEN_SIZE.width,
        "a full-width line would run off the right edge, clipping a digit"
    );
    assert!(
        RAW_Y as u32 + cell_height <= PCT_Y as u32,
        "the two text rows overlap"
    );
    assert!(
        PCT_Y as u32 + cell_height <= SCREEN_SIZE.height,
        "the lower text row would run off the bottom edge"
    );
    assert!(
        LINE_WIDTH < LINE_CAP,
        "a padded line must fit its own buffer"
    );

    // The creature lives in the panel's dead right-hand region. Both claims below are
    // arithmetic over constants, so a layout that overlapped the text — or hung off the
    // glass — could never be built, let alone flashed.
    let sprite_extent: u32 = crate::sprite::SPRITE_SIZE as u32 * SPRITE_SCALE;
    let text_right: u32 = TEXT_X as u32 + cell_width * LINE_WIDTH as u32;
    assert!(
        SPRITE_ORIGIN.x as u32 >= text_right,
        "the creature overlaps the widest text line"
    );
    assert!(
        SPRITE_ORIGIN.x as u32 + sprite_extent <= SCREEN_SIZE.width,
        "the creature runs off the right edge"
    );
    assert!(
        SPRITE_ORIGIN.y as u32 + sprite_extent <= SCREEN_SIZE.height,
        "the creature runs off the bottom edge"
    );
};

/// Draw one baseline-top text line at `y`, padded to [`LINE_WIDTH`] with an opaque
/// black background so it overwrites its whole row in place.
///
/// The single drawing primitive of this crate: both the observation screen and the
/// colour-check labels go through it, so their metrics cannot disagree.
pub fn text_line<D>(
    target: &mut D,
    y: i32,
    color: Rgb565,
    content: core::fmt::Arguments<'_>,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut line: Line = Line::new();
    line.write_fmt(content)
        .map_err(|_| RenderError::LineOverflow)?;
    // Pad to a fixed field so a shorter value fully erases the longer one it
    // replaces. `push` only fails at LINE_CAP, which LINE_WIDTH is well inside.
    while line.len() < LINE_WIDTH && line.push(' ').is_ok() {}

    let style: MonoTextStyle<'_, Rgb565> = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(color)
        .background_color(Rgb565::BLACK)
        .build();
    Text::with_baseline(line.as_str(), Point::new(TEXT_X, y), style, Baseline::Top)
        .draw(target)
        .map_err(RenderError::Draw)?;
    Ok(())
}

/// Draw a text line with a *transparent* background, over whatever is beneath.
///
/// The colour-check labels sit on top of their bands, so they must not paint a black
/// box around themselves — unlike [`text_line`], whose opaque background is the
/// whole mechanism by which a value erases its predecessor.
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
        .font(&FONT_10X20)
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
    use proptest::prelude::*;

    /// The exact format strings [`crate::screen`] renders. Kept here beside
    /// `LINE_CAP` so the proof below is about the buffer, not about a copy of it.
    fn raw_line(raw: u16) -> Line {
        let mut line: Line = Line::new();
        write!(line, "RAW  {raw:>4}").expect("the raw line must fit");
        line
    }

    fn percent_line(percent: u8) -> Line {
        let mut line: Line = Line::new();
        write!(line, "SOIL {percent:>3}%").expect("the percent line must fit");
        line
    }

    proptest! {
        /// [`RenderError::LineOverflow`] is unreachable for a raw count: `u16::MAX`
        /// is five digits, so `"RAW  65535"` is exactly [`LINE_WIDTH`]. Proven over
        /// the whole domain of the value, not spot-checked at a boundary.
        #[test]
        fn a_raw_count_always_fits_the_line_buffer(raw: u16) {
            let line: Line = raw_line(raw);
            prop_assert!(line.len() <= LINE_CAP, "{line} overflows the {LINE_CAP}-byte buffer");
        }

        /// Likewise a percent, which the domain already bounds to `0..=100`; this
        /// holds even for the values the domain forbids, so a future widening of
        /// `Moisture` cannot silently truncate a digit on the glass.
        #[test]
        fn a_percent_always_fits_the_line_buffer(percent: u8) {
            let line: Line = percent_line(percent);
            prop_assert!(line.len() <= LINE_CAP, "{line} overflows the {LINE_CAP}-byte buffer");
        }
    }

    /// The widest raw count fills the field exactly — so [`LINE_WIDTH`] is the true
    /// upper bound on a rendered line, and the padding never has to truncate.
    #[test]
    fn the_widest_raw_count_exactly_fills_the_field() {
        assert_eq!(raw_line(u16::MAX).len(), LINE_WIDTH);
    }

    /// The *formatter* right-aligns into four columns; the padding in [`text_line`]
    /// then fills the rest of the field. Two different mechanisms — this asserts only
    /// the first, and shows a narrow count leaving the formatter short of
    /// [`LINE_WIDTH`], which is precisely why the padding must exist.
    #[test]
    fn the_formatter_right_aligns_a_narrow_raw_count_but_leaves_the_field_short() {
        assert_eq!(raw_line(7).as_str(), "RAW     7");
        assert!(raw_line(7).len() < LINE_WIDTH);
    }

    // The canvas orientation, the row spacing, and the right-edge fit are not tested
    // here: they are constants, and the `const _: ()` block above asserts them at
    // compile time. A test would be the weaker check.
}
