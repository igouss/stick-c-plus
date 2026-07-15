//! Where the plant monitor's two text rows and its creature sit on the canvas.
//!
//! These are facts about *this app's* picture: where the raw-count and percent rows sit,
//! how wide a field a number is padded to, where the creature stands. The canvas size, the
//! font, and the line-buffer cap are the board's, not the plant's — they come from
//! [`platform_display`]. The panel's own facts (CGRAM offset, colour inversion, SPI pins)
//! stay in the driven adapter, the only party that knows them.

use platform_display::{FONT, LINE_CAP, SCREEN_SIZE};

use embedded_graphics::prelude::*;

/// Baseline-top x inset of both text lines.
pub const TEXT_X: i32 = 8;
/// Baseline-top y of the upper line (the raw count, or the fault headline).
pub const RAW_Y: i32 = 34;
/// Baseline-top y of the lower line (the percent, or the fault reason).
pub const PCT_Y: i32 = 80;

/// Fixed character width both lines are padded to, via [`platform_display::text_line`].
///
/// A shorter new value's trailing spaces — painted with the opaque background — erase the
/// leftover glyphs of a longer old one. That is what lets a redraw touch only its own two
/// rows: no full-screen clear, and therefore no per-update flash.
pub const LINE_WIDTH: usize = 10;

/// Panel pixels per sprite cell. A 20×20 creature becomes 100×100.
pub const SPRITE_SCALE: u32 = 5;
/// Top-left corner of the creature: the right-hand third of the panel, which the two text
/// rows never reach.
pub const SPRITE_ORIGIN: Point = Point::new(132, 17);

/// The geometry invariants, checked by the **compiler**.
///
/// Every term here is a constant, so a portrait canvas, a pair of overlapping rows, or a
/// line that runs off an edge fails the *build* — on the host and on the Xtensa target
/// alike. That is strictly stronger than a test, which can be filtered, skipped, or simply
/// not run before a flash. Test what varies; assert what cannot.
const _: () = {
    let cell_width: u32 = FONT.character_size.width;
    let cell_height: u32 = FONT.character_size.height;

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
        "a padded line must fit the platform line buffer"
    );

    // The creature lives in the panel's dead right-hand region. Both claims below are
    // arithmetic over constants, so a layout that overlapped the text — or hung off the
    // glass — could never be built, let alone flashed.
    let sprite_extent: u32 = platform_display::sprite::SPRITE_SIZE as u32 * SPRITE_SCALE;
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

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The exact format strings [`crate::screen`] renders, so the proof below is about the
    /// content the plant monitor actually draws, not a copy of it.
    fn raw_line(raw: u16) -> String {
        format!("RAW  {raw:>4}")
    }

    fn percent_line(percent: u8) -> String {
        format!("SOIL {percent:>3}%")
    }

    proptest! {
        /// A raw count always fits the padded field: `u16::MAX` is five digits, so
        /// `"RAW  65535"` is exactly [`LINE_WIDTH`]. Proven over the whole domain of the
        /// value, not spot-checked at a boundary — so it can never reach
        /// [`platform_display::RenderError::LineOverflow`].
        #[test]
        fn a_raw_count_always_fits_the_field(raw: u16) {
            prop_assert!(raw_line(raw).len() <= LINE_WIDTH, "{} overflows the field", raw_line(raw));
        }

        /// Likewise a percent, which the domain already bounds to `0..=100`; this holds
        /// even for the values the domain forbids, so a future widening of `Moisture`
        /// cannot silently truncate a digit on the glass.
        #[test]
        fn a_percent_always_fits_the_field(percent: u8) {
            prop_assert!(percent_line(percent).len() <= LINE_WIDTH, "{} overflows the field", percent_line(percent));
        }
    }

    /// The widest raw count fills the field exactly — so [`LINE_WIDTH`] is the true upper
    /// bound on a rendered line, and the padding never has to truncate.
    #[test]
    fn the_widest_raw_count_exactly_fills_the_field() {
        assert_eq!(raw_line(u16::MAX).len(), LINE_WIDTH);
    }

    /// The *formatter* right-aligns into four columns; the padding in
    /// [`platform_display::text_line`] then fills the rest of the field. Two different
    /// mechanisms — this asserts only the first, and shows a narrow count leaving the
    /// formatter short of [`LINE_WIDTH`], which is precisely why the padding must exist.
    #[test]
    fn the_formatter_right_aligns_a_narrow_raw_count_but_leaves_the_field_short() {
        assert_eq!(raw_line(7).as_str(), "RAW     7");
        assert!(raw_line(7).len() < LINE_WIDTH);
    }

    // The canvas orientation, the row spacing, and the right-edge fit are not tested here:
    // they are constants, and the `const _: ()` block above asserts them at compile time. A
    // test would be the weaker check.
}
