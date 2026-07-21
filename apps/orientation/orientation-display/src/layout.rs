//! Where the face label, the three axis bars, and the angles sit on the canvas.
//!
//! Facts about *this app's* picture. The canvas size and the font are the board's, from
//! [`platform_display`]; the panel's own facts stay in the driven adapter.
//!
//! The screen is three stacked axis rows under a header. Each row is the same shape — a
//! one-letter axis name, a centre-zero bar, and the signed milli-g value — so the three read
//! as one instrument rather than three unrelated readouts, and their metrics cannot drift
//! apart because they are all derived from the constants below.

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use platform_display::{FONT, SCREEN_SIZE};

/// Top-left of the resting-face label, top-left of the screen.
pub const FACING_ORIGIN: Point = Point::new(6, 4);
/// Fixed field the face label is padded to, so a shorter name erases a longer one.
/// `RIGHT EDGE` is the widest at 10 characters.
pub const FACING_WIDTH: usize = 10;

/// Top-left of the pitch/roll readout, sharing the header row with the face label.
pub const ANGLES_ORIGIN: Point = Point::new(126, 4);
/// Fixed field the angles are padded to: `P+90 R-180` is ten characters at its widest.
pub const ANGLES_WIDTH: usize = 10;

/// Top-left of the first axis row; the rows below it are one [`ROW_PITCH`] apart.
pub const FIRST_ROW_ORIGIN: Point = Point::new(6, 36);
/// Vertical distance between one axis row and the next.
pub const ROW_PITCH: i32 = 32;
/// How many axis rows there are — one per axis.
pub const ROWS: i32 = 3;

/// Fixed field an axis' one-letter name is padded to.
pub const AXIS_NAME_WIDTH: usize = 1;

/// Left edge of the bars, clear of the axis names.
pub const BAR_LEFT: i32 = 26;
/// The bars' width. Even, so the centre axis lands on a whole column and the two halves are
/// the same size — an odd width would make a positive reading one pixel longer than the equal
/// negative one, and the eye catches that on a bar that swings.
pub const BAR_WIDTH: u32 = 130;
/// The bars' height — shorter than a text line, so a bar sits visually inside its row.
pub const BAR_HEIGHT: u32 = 14;
/// How far a bar is dropped inside its row, to centre it against the row's text.
pub const BAR_INSET_Y: i32 = 3;

/// Left edge of an axis' milli-g value, clear of the bars.
pub const VALUE_LEFT: i32 = 164;
/// Fixed field a milli-g value is padded to. `-8000` is five characters at the widest full
/// scale the sensor is configured for; the sixth is headroom so a reading can never be
/// truncated into a plausible-looking smaller number.
pub const VALUE_WIDTH: usize = 6;

/// The full-scale magnitude an axis bar deflects across, in milli-g.
///
/// One gravity, so the axis a resting board is lying on pins its bar to the end of the track
/// — the readout is at its most legible in exactly the pose it spends most of its time in.
/// A reading beyond one gravity clamps the *bar* (see
/// [`signed_bar`](platform_display::signed_bar)), but never the number printed beside it, so
/// a hard shake is still readable as a figure even once the bar has run out of room.
pub const BAR_FULL_SCALE_MG: i32 = platform_core::ONE_G_MG;

/// The top-left of axis row `index`, counting from zero.
pub const fn row_origin(index: i32) -> Point {
    Point::new(FIRST_ROW_ORIGIN.x, FIRST_ROW_ORIGIN.y + index * ROW_PITCH)
}

/// The bar rectangle of axis row `index`.
pub const fn row_bar(index: i32) -> Rectangle {
    Rectangle::new(
        Point::new(BAR_LEFT, row_origin(index).y + BAR_INSET_Y),
        Size::new(BAR_WIDTH, BAR_HEIGHT),
    )
}

/// The top-left of the milli-g value of axis row `index`.
pub const fn row_value_origin(index: i32) -> Point {
    Point::new(VALUE_LEFT, row_origin(index).y)
}

/// The geometry invariants, checked by the **compiler** — a label that ran off an edge, a bar
/// that collided with its value, or a third row that fell off the bottom would fail the
/// *build*, on host and Xtensa alike, rather than being noticed on the glass.
const _: () = {
    let cell_w: u32 = FONT.character_size.width;
    let cell_h: u32 = FONT.character_size.height;
    let last_row_y: i32 = FIRST_ROW_ORIGIN.y + (ROWS - 1) * ROW_PITCH;

    assert!(
        SCREEN_SIZE.width > SCREEN_SIZE.height,
        "the canvas is landscape once rotated"
    );
    assert!(
        FACING_ORIGIN.x as u32 + cell_w * FACING_WIDTH as u32 <= ANGLES_ORIGIN.x as u32,
        "the widest face label runs into the angles readout"
    );
    assert!(
        ANGLES_ORIGIN.x as u32 + cell_w * ANGLES_WIDTH as u32 <= SCREEN_SIZE.width,
        "the angles readout runs off the right edge"
    );
    assert!(
        ANGLES_ORIGIN.y as u32 + cell_h <= FIRST_ROW_ORIGIN.y as u32,
        "the header row overlaps the first axis row"
    );
    assert!(
        FIRST_ROW_ORIGIN.x as u32 + cell_w * AXIS_NAME_WIDTH as u32 <= BAR_LEFT as u32,
        "an axis name runs into its bar"
    );
    assert!(
        BAR_LEFT as u32 + BAR_WIDTH <= VALUE_LEFT as u32,
        "a bar runs into its milli-g value"
    );
    assert!(
        VALUE_LEFT as u32 + cell_w * VALUE_WIDTH as u32 <= SCREEN_SIZE.width,
        "a milli-g value runs off the right edge"
    );
    assert!(
        BAR_WIDTH.is_multiple_of(2),
        "an odd bar width puts the centre axis off-centre, so the two halves differ in length"
    );
    assert!(
        BAR_INSET_Y as u32 + BAR_HEIGHT <= cell_h,
        "a bar is taller than the row it sits in, so it would overlap the row below"
    );
    assert!(
        ROW_PITCH as u32 >= cell_h,
        "the axis rows are closer together than a line of text is tall"
    );
    assert!(
        last_row_y as u32 + cell_h <= SCREEN_SIZE.height,
        "the last axis row runs off the bottom edge"
    );
};

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows step evenly down the screen — the property the whole three-row instrument
    /// depends on, and the one a hand-written per-row constant would eventually break.
    #[test]
    fn the_rows_are_evenly_spaced() {
        assert_eq!(row_origin(0).y, FIRST_ROW_ORIGIN.y);
        assert_eq!(row_origin(1).y - row_origin(0).y, ROW_PITCH);
        assert_eq!(row_origin(2).y - row_origin(1).y, ROW_PITCH);
    }

    /// Every row's bar and value share one horizontal geometry — only the row's height
    /// differs, so the three bars line up as a single instrument.
    #[test]
    fn every_row_shares_one_horizontal_geometry() {
        (0..ROWS).for_each(|index: i32| {
            assert_eq!(row_bar(index).top_left.x, BAR_LEFT);
            assert_eq!(row_bar(index).size, Size::new(BAR_WIDTH, BAR_HEIGHT));
            assert_eq!(row_value_origin(index).x, VALUE_LEFT);
        });
    }

    /// The bar's centre lands on a whole column, so a positive and an equal negative reading
    /// really are mirror images rather than differing by a pixel.
    #[test]
    fn the_bar_centre_lands_on_a_whole_column() {
        assert!(BAR_WIDTH.is_multiple_of(2));
        assert_eq!(BAR_WIDTH / 2 * 2, BAR_WIDTH);
    }
}
