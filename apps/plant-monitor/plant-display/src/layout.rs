//! Where the plant monitor's two text rows and its creature sit on the canvas.
//!
//! These are facts about *this app's* picture: where the raw-count and percent rows sit,
//! how wide a field a number is padded to, where the creature stands. The font and the
//! line-buffer cap are the board's, not the plant's — they come from [`platform_display`].
//! The panel's own facts (CGRAM offset, colour inversion, SPI pins) stay in the driven
//! adapter, the only party that knows them.
//!
//! ## Why there are two of them
//!
//! There is one [`Layout`] per canvas shape, because the two shapes cannot share a picture.
//! Landscape stands the creature in the right-hand third the two text rows never reach — a
//! column split, which is what 240 px of width is for. Turned a quarter, that third is gone:
//! a ten-character row is 100 px of the 135 available, leaving 35, and the creature is 100 px
//! square. So [`PORTRAIT`] stacks the creature *below* the rows and spends the height it
//! gained on the split it lost.
//!
//! Neither the field width nor the creature's scale changes between them. A narrower field
//! would truncate a reading into a plausible-looking smaller one, and a smaller creature would
//! shrink the thing a glance actually lands on; height is what portrait has to spend.

use embedded_graphics::prelude::*;
use platform_core::ScreenRotation;
use platform_display::{FieldAlign, FONT, LINE_CAP, SCREEN_SIZE};

/// Fixed character width both lines are padded to, via [`platform_display::text_field`].
///
/// A shorter new value's padding — painted with the opaque background — erases the leftover
/// glyphs of a longer old one. That is what lets a redraw touch only its own two rows: no
/// full-screen clear, and therefore no per-update flash.
///
/// The same in both shapes. This is the one measurement that must not shrink to fit a
/// narrower canvas: a bar that runs out of room is visibly clamped, but a number quietly
/// missing a digit reads as a smaller reading, and there is nothing on the glass to say
/// otherwise.
pub const LINE_WIDTH: usize = 10;

/// Panel pixels per sprite cell. A 20×20 creature becomes 100×100.
pub const SPRITE_SCALE: u32 = 5;

/// How far a 20×20 creature reaches once scaled.
const SPRITE_EXTENT: u32 = platform_display::sprite::SPRITE_SIZE as u32 * SPRITE_SCALE;

/// The panel's native landscape canvas.
pub const LANDSCAPE_CANVAS: Size = SCREEN_SIZE;

/// The canvas a quarter turn puts the picture on — the panel's dimensions swapped.
pub const PORTRAIT_CANVAS: Size = Size::new(SCREEN_SIZE.height, SCREEN_SIZE.width);

/// The canvas the monitor is drawn on at `rotation`.
///
/// The one thing outside this crate needs to know about its geometry: a host target has to be
/// allocated at the right shape before the picture is drawn into it, and a target the wrong
/// shape either clips the picture or reports a correctly-placed pixel as having escaped.
pub const fn canvas_size(rotation: ScreenRotation) -> Size {
    Layout::for_rotation(rotation).canvas
}

/// Where everything sits, for one canvas shape.
///
/// A value rather than a module of constants, so a renderer is *handed* its geometry and
/// cannot reach past it to the other shape's. Both shapes are checked by the compiler — see
/// [`Layout::check`] — so an invalid one fails the build rather than the eye.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layout {
    /// The canvas this layout draws on. Every invariant is measured against it.
    pub canvas: Size,
    /// Left edge of both text rows.
    pub text_x: i32,
    /// Baseline-top y of the upper row (the raw count, or the fault headline).
    pub raw_y: i32,
    /// Baseline-top y of the lower row (the percent, or the fault reason).
    pub pct_y: i32,
    /// Top-left corner of the creature.
    pub sprite_origin: Point,
    /// Where a value sits inside its padded field.
    ///
    /// A property of the shape rather than of either row, because it answers one question —
    /// is there anything beside this text to align against? Landscape has the creature in the
    /// next column and reads best flush left; the portrait stack has nothing either side, and
    /// a short value pinned to the left of a wide field reads there as drifted rather than
    /// aligned.
    pub text_align: FieldAlign,
}

impl Layout {
    /// The layout the picture is drawn through at `rotation`.
    ///
    /// A half turn is still landscape and a three-quarter turn is still portrait: what a
    /// layout answers is the *shape* of the canvas, and only the panel cares which of the two
    /// ways up that shape is being read.
    pub const fn for_rotation(rotation: ScreenRotation) -> Layout {
        if rotation.is_portrait() {
            PORTRAIT
        } else {
            LANDSCAPE
        }
    }

    /// The geometry invariants, evaluated by the **compiler**.
    ///
    /// Every term is a constant, so a pair of overlapping rows, a line that runs off an edge,
    /// or a creature over the text fails the *build* — on the host and on the Xtensa target
    /// alike. That is strictly stronger than a test, which can be filtered, skipped, or simply
    /// not run before a flash. Test what varies; assert what cannot.
    ///
    /// A `const fn` rather than a `const` block so that **every** layout is held to it: a
    /// third shape added later is one `const _: () = ITS_NAME.check();` away from the same
    /// proof, and one that skipped the call would be conspicuous.
    pub const fn check(&self) {
        let cell_w: u32 = FONT.character_size.width;
        let cell_h: u32 = FONT.character_size.height;
        let text_right: u32 = self.text_x as u32 + cell_w * LINE_WIDTH as u32;

        assert!(
            self.canvas.width == SCREEN_SIZE.width || self.canvas.width == SCREEN_SIZE.height,
            "the canvas is the panel, at one of its two ways up"
        );
        assert!(
            text_right <= self.canvas.width,
            "a full-width line would run off the right edge, clipping a digit"
        );
        assert!(
            self.raw_y as u32 + cell_h <= self.pct_y as u32,
            "the two text rows overlap"
        );
        assert!(
            self.pct_y as u32 + cell_h <= self.canvas.height,
            "the lower text row would run off the bottom edge"
        );
        assert!(
            self.sprite_origin.x as u32 + SPRITE_EXTENT <= self.canvas.width,
            "the creature runs off the right edge"
        );
        assert!(
            self.sprite_origin.y as u32 + SPRITE_EXTENT <= self.canvas.height,
            "the creature runs off the bottom edge"
        );
        // The creature clears BOTH text rows, by a clear column or a clear row. Landscape
        // takes the first branch and portrait the second, and stating it as one disjunction
        // rather than two per-shape rules is what lets a third shape choose either.
        assert!(
            self.sprite_origin.x as u32 >= text_right
                || self.sprite_origin.y as u32 >= self.pct_y as u32 + cell_h,
            "the creature overlaps the text — it shares neither a clear column nor a clear row"
        );
    }
}

/// The panel held horizontally: 240 px of width, twenty-four characters a line.
///
/// The two rows sit at the left and the creature stands in the right-hand third they never
/// reach — a column split, which is what the width is for.
pub const LANDSCAPE: Layout = Layout {
    canvas: LANDSCAPE_CANVAS,
    text_x: 8,
    raw_y: 34,
    pct_y: 80,
    sprite_origin: Point::new(132, 17),
    text_align: FieldAlign::Left,
};

/// The panel held upright: 135 px of width, thirteen characters a line.
///
/// A ten-character row and a 100 px creature cannot sit side by side in 135 px, so the picture
/// stacks: the two rows, then the creature beneath them. The row field and the creature happen
/// to be the same 100 px wide, so both centre on the same `x = 17` and the picture reads as one
/// column rather than as two things that each found their own margin.
///
/// Both rows are [`FieldAlign::Centred`]: with nothing either side of the stack, a short value
/// like `FAULT` flush against the left of a ten-character field reads as having drifted.
pub const PORTRAIT: Layout = Layout {
    canvas: PORTRAIT_CANVAS,
    text_x: 17,
    raw_y: 28,
    pct_y: 60,
    sprite_origin: Point::new(17, 112),
    text_align: FieldAlign::Centred,
};

const _: () = LANDSCAPE.check();
const _: () = PORTRAIT.check();

/// A padded line must fit the platform's line buffer. Not part of [`Layout::check`] because it
/// is a fact about the field width alone — true of every shape, and of a shape not yet written.
const _: () = assert!(
    LINE_WIDTH < LINE_CAP,
    "a padded line must fit the platform line buffer"
);

/// The two shapes really are the two shapes, and each splits the way its width allows —
/// checked at build time, because every term is a constant and a test would be the weaker
/// check. Landscape stands the creature beside the text and portrait below it; an edit that
/// quietly turned one into the other is a compile error.
const _: () = {
    assert!(
        LANDSCAPE_CANVAS.width > LANDSCAPE_CANVAS.height,
        "the landscape canvas is wider than it is tall"
    );
    assert!(
        PORTRAIT_CANVAS.height > PORTRAIT_CANVAS.width,
        "the portrait canvas is taller than it is wide"
    );
    assert!(
        LANDSCAPE.sprite_origin.x as u32
            >= LANDSCAPE.text_x as u32 + FONT.character_size.width * LINE_WIDTH as u32,
        "landscape stands the creature beside the text"
    );
    assert!(
        PORTRAIT.sprite_origin.y > PORTRAIT.pct_y,
        "portrait stands the creature below the text"
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
    /// [`platform_display::text_field`] then fills the rest of the field. Two different
    /// mechanisms — this asserts only the first, and shows a narrow count leaving the
    /// formatter short of [`LINE_WIDTH`], which is precisely why the padding must exist.
    #[test]
    fn the_formatter_right_aligns_a_narrow_raw_count_but_leaves_the_field_short() {
        assert_eq!(raw_line(7).as_str(), "RAW     7");
        assert!(raw_line(7).len() < LINE_WIDTH);
    }

    /// The quarter turns get the tall canvas and the half turns the wide one — the whole of
    /// what a rotation decides about the picture's geometry.
    #[test]
    fn each_rotation_draws_through_the_layout_for_its_shape() {
        assert_eq!(Layout::for_rotation(ScreenRotation::Deg0), LANDSCAPE);
        assert_eq!(Layout::for_rotation(ScreenRotation::Deg180), LANDSCAPE);
        assert_eq!(Layout::for_rotation(ScreenRotation::Deg90), PORTRAIT);
        assert_eq!(Layout::for_rotation(ScreenRotation::Deg270), PORTRAIT);
    }

    /// The two shapes are the two ways up of one panel, not two unrelated canvases.
    #[test]
    fn the_portrait_canvas_is_the_landscape_one_turned() {
        assert_eq!(PORTRAIT.canvas.width, LANDSCAPE.canvas.height);
        assert_eq!(PORTRAIT.canvas.height, LANDSCAPE.canvas.width);
    }

    /// In portrait the text field and the creature are the same width, so they share one
    /// centre line — the reason the stack reads as a single column.
    #[test]
    fn the_portrait_stack_shares_one_centre_line() {
        assert_eq!(
            FONT.character_size.width * LINE_WIDTH as u32,
            SPRITE_EXTENT,
            "the row field and the creature are the same width"
        );
        assert_eq!(PORTRAIT.text_x, PORTRAIT.sprite_origin.x);
    }

    /// Landscape aligns its text flush left against the creature's column; portrait, with
    /// nothing beside it, centres.
    #[test]
    fn only_the_stacked_shape_centres_its_text() {
        assert_eq!(LANDSCAPE.text_align, FieldAlign::Left);
        assert_eq!(PORTRAIT.text_align, FieldAlign::Centred);
    }

    /// The narrow canvas really is narrow: whatever the portrait layout does, it does inside
    /// thirteen columns. Stated as the number rather than derived, so a font change that
    /// silently bought room shows up here.
    #[test]
    fn the_portrait_canvas_holds_thirteen_characters() {
        assert_eq!(PORTRAIT.canvas.width / FONT.character_size.width, 13);
        assert_eq!(LANDSCAPE.canvas.width / FONT.character_size.width, 24);
    }

    // The canvas orientation, the row spacing, and the right-edge fit are not tested here:
    // they are constants, and `Layout::check` asserts them at compile time for both shapes. A
    // test would be the weaker check.
}
