//! Where the face label, the three axis bars, and the angles sit on the canvas.
//!
//! Facts about *this app's* picture. The canvas size and the font are the board's, from
//! [`platform_display`]; the panel's own facts stay in the driven adapter.
//!
//! The screen is three stacked axis rows under a header. Each row is the same shape — a
//! one-letter axis name, a centre-zero bar, and the signed milli-g value — so the three read
//! as one instrument rather than three unrelated readouts, and their metrics cannot drift
//! apart because they are all derived from one [`Layout`].
//!
//! ## Why there are two of them
//!
//! There is one [`Layout`] per canvas shape, because the two shapes cannot share a picture.
//! Landscape gives 240 / 10 = **24 characters** a line; turned a quarter, the canvas is 135
//! wide, which is **13**. The header's label and angles fit side by side in twenty-four and
//! cannot in thirteen, so [`PORTRAIT`] stacks them, and the bars give up half their width to
//! keep the milli-g field whole. Everything else — the row shape, the field widths, the order
//! the axes stack in — is deliberately identical, so the readout is the same instrument held
//! a different way up rather than two instruments.

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use platform_core::ScreenRotation;
use platform_display::{FONT, SCREEN_SIZE};

/// How many axis rows there are — one per axis, in either shape.
pub const ROWS: i32 = 3;

/// Fixed field an axis' one-letter name is padded to.
pub const AXIS_NAME_WIDTH: usize = 1;

/// Fixed field the face label is padded to, so a shorter name erases a longer one.
/// `RIGHT EDGE` is the widest at 10 characters, and `NO SIGNAL` is 9 — both fit the narrow
/// canvas' thirteen columns, so neither shape has to abbreviate the one word on the glass a
/// reader actually reads.
pub const FACING_WIDTH: usize = 10;

/// Fixed field the angles are padded to: `P+90 R-180` is ten characters at its widest.
pub const ANGLES_WIDTH: usize = 10;

/// Fixed field a milli-g value is padded to. `-8000` is five characters at the widest full
/// scale the sensor is configured for; the sixth is headroom so a reading can never be
/// truncated into a plausible-looking smaller number.
///
/// This width is the one thing that does not shrink to fit a narrower canvas. A bar that runs
/// out of room is visibly clamped; a number quietly missing a digit reads as a smaller
/// reading, and there is nothing on the glass to say otherwise.
pub const VALUE_WIDTH: usize = 6;

/// The full-scale magnitude an axis bar deflects across, in milli-g.
///
/// One gravity, so the axis a resting board is lying on pins its bar to the end of the track
/// — the readout is at its most legible in exactly the pose it spends most of its time in.
/// A reading beyond one gravity clamps the *bar* (see
/// [`signed_bar`](platform_display::signed_bar)), but never the number printed beside it, so
/// a hard shake is still readable as a figure even once the bar has run out of room.
pub const BAR_FULL_SCALE_MG: i32 = platform_core::ONE_G_MG;

/// The panel's native landscape canvas.
pub const LANDSCAPE_CANVAS: Size = SCREEN_SIZE;

/// The canvas a quarter turn puts the picture on — the panel's dimensions swapped.
pub const PORTRAIT_CANVAS: Size = Size::new(SCREEN_SIZE.height, SCREEN_SIZE.width);

/// The canvas the readout is drawn on at `rotation`.
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
    /// Top-left of the resting-face label.
    pub facing_origin: Point,
    /// Top-left of the pitch/roll readout.
    pub angles_origin: Point,
    /// Top-left of the first axis row; the rows below it are one [`row_pitch`](Self::row_pitch)
    /// apart.
    pub first_row_origin: Point,
    /// Vertical distance between one axis row and the next.
    pub row_pitch: i32,
    /// Left edge of the bars, clear of the axis names.
    pub bar_left: i32,
    /// The bars' width. Even, so the centre axis lands on a whole column and the two halves
    /// are the same size — an odd width would make a positive reading one pixel longer than
    /// the equal negative one, and the eye catches that on a bar that swings.
    pub bar_width: u32,
    /// The bars' height — shorter than a text line, so a bar sits visually inside its row.
    pub bar_height: u32,
    /// How far a bar is dropped inside its row, to centre it against the row's text.
    pub bar_inset_y: i32,
    /// Left edge of an axis' milli-g value, clear of the bars.
    pub value_left: i32,
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

    /// The top-left of axis row `index`, counting from zero.
    pub const fn row_origin(&self, index: i32) -> Point {
        Point::new(
            self.first_row_origin.x,
            self.first_row_origin.y + index * self.row_pitch,
        )
    }

    /// The bar rectangle of axis row `index`.
    pub const fn row_bar(&self, index: i32) -> Rectangle {
        Rectangle::new(
            Point::new(self.bar_left, self.row_origin(index).y + self.bar_inset_y),
            Size::new(self.bar_width, self.bar_height),
        )
    }

    /// The top-left of the milli-g value of axis row `index`.
    pub const fn row_value_origin(&self, index: i32) -> Point {
        Point::new(self.value_left, self.row_origin(index).y)
    }

    /// The geometry invariants, evaluated by the **compiler** — a label that ran off an edge,
    /// a bar that collided with its value, or a third row that fell off the bottom fails the
    /// *build*, on host and Xtensa alike, rather than being noticed on the glass.
    ///
    /// A `const fn` rather than a `const` block so that **every** layout is held to it: a
    /// third shape added later is one `const _: () = ITS_NAME.check();` away from the same
    /// proof, and one that skipped the call would be conspicuous.
    pub const fn check(&self) {
        let cell_w: u32 = FONT.character_size.width;
        let cell_h: u32 = FONT.character_size.height;
        let last_row_y: i32 = self.first_row_origin.y + (ROWS - 1) * self.row_pitch;

        assert!(
            self.canvas.width == SCREEN_SIZE.width || self.canvas.width == SCREEN_SIZE.height,
            "the canvas is the panel, at one of its two ways up"
        );
        assert!(
            self.facing_origin.x as u32 + cell_w * FACING_WIDTH as u32 <= self.canvas.width,
            "the widest face label runs off the right edge"
        );
        assert!(
            self.angles_origin.x as u32 + cell_w * ANGLES_WIDTH as u32 <= self.canvas.width,
            "the angles readout runs off the right edge"
        );
        assert!(
            self.facing_origin.y as u32 + cell_h <= self.angles_origin.y as u32
                || self.facing_origin.x as u32 + cell_w * FACING_WIDTH as u32
                    <= self.angles_origin.x as u32,
            "the face label and the angles overlap — they share neither a clear row nor a clear column"
        );
        assert!(
            self.angles_origin.y as u32 + cell_h <= self.first_row_origin.y as u32,
            "the header overlaps the first axis row"
        );
        assert!(
            self.first_row_origin.x as u32 + cell_w * AXIS_NAME_WIDTH as u32
                <= self.bar_left as u32,
            "an axis name runs into its bar"
        );
        assert!(
            self.bar_left as u32 + self.bar_width <= self.value_left as u32,
            "a bar runs into its milli-g value"
        );
        assert!(
            self.value_left as u32 + cell_w * VALUE_WIDTH as u32 <= self.canvas.width,
            "a milli-g value runs off the right edge"
        );
        assert!(
            self.bar_width.is_multiple_of(2),
            "an odd bar width puts the centre axis off-centre, so the two halves differ in length"
        );
        assert!(
            self.bar_inset_y as u32 + self.bar_height <= cell_h,
            "a bar is taller than the row it sits in, so it would overlap the row below"
        );
        assert!(
            self.row_pitch as u32 >= cell_h,
            "the axis rows are closer together than a line of text is tall"
        );
        assert!(
            last_row_y as u32 + cell_h <= self.canvas.height,
            "the last axis row runs off the bottom edge"
        );
    }
}

/// The panel held horizontally: 240 px of width, twenty-four characters a line.
///
/// The header fits on one row — the face label at the left, the angles at the right — and the
/// bars have 130 px of track to swing across.
pub const LANDSCAPE: Layout = Layout {
    canvas: LANDSCAPE_CANVAS,
    facing_origin: Point::new(6, 4),
    angles_origin: Point::new(126, 4),
    first_row_origin: Point::new(6, 36),
    row_pitch: 32,
    bar_left: 26,
    bar_width: 130,
    bar_height: 14,
    bar_inset_y: 3,
    value_left: 164,
};

/// The panel held upright: 135 px of width, thirteen characters a line.
///
/// Thirteen columns cannot hold a ten-character label beside a ten-character angles field, so
/// the header stacks onto two rows. Nor can they hold a name, a full-width bar and a
/// six-character value: name (1) + value (6) is seven columns of the thirteen, which leaves
/// the bar the 56 px between them against landscape's 130. The bar is the part that gives,
/// because a shorter bar is still an honest bar; see [`VALUE_WIDTH`] for the part that does
/// not. The margins are correspondingly thin — 2 px a side rather than 6 — because on this
/// canvas a margin is a column of digits.
///
/// Height is the one thing this shape has to spare — 240 px for five occupied rows — so the
/// header sits clear of the instrument and the rows breathe at a wider pitch than landscape's.
pub const PORTRAIT: Layout = Layout {
    canvas: PORTRAIT_CANVAS,
    facing_origin: Point::new(2, 8),
    angles_origin: Point::new(2, 34),
    first_row_origin: Point::new(2, 92),
    row_pitch: 48,
    bar_left: 16,
    bar_width: 56,
    bar_height: 14,
    bar_inset_y: 3,
    value_left: 74,
};

const _: () = LANDSCAPE.check();
const _: () = PORTRAIT.check();

/// The two shapes really are the two shapes, checked at build time so that a layout claiming a
/// canvas the panel cannot present is a compile error rather than a picture off the edge.
const _: () = {
    assert!(
        LANDSCAPE_CANVAS.width > LANDSCAPE_CANVAS.height,
        "the landscape canvas is wider than it is tall"
    );
    assert!(
        PORTRAIT_CANVAS.height > PORTRAIT_CANVAS.width,
        "the portrait canvas is taller than it is wide"
    );
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Both shapes, so a rule stated once is checked against each of them.
    const BOTH: [Layout; 2] = [LANDSCAPE, PORTRAIT];

    /// The rows step evenly down the screen — the property the whole three-row instrument
    /// depends on, and the one a hand-written per-row constant would eventually break.
    #[test]
    fn the_rows_are_evenly_spaced() {
        BOTH.iter().for_each(|layout: &Layout| {
            assert_eq!(layout.row_origin(0).y, layout.first_row_origin.y);
            assert_eq!(
                layout.row_origin(1).y - layout.row_origin(0).y,
                layout.row_pitch
            );
            assert_eq!(
                layout.row_origin(2).y - layout.row_origin(1).y,
                layout.row_pitch
            );
        });
    }

    /// Every row's bar and value share one horizontal geometry — only the row's height
    /// differs, so the three bars line up as a single instrument.
    #[test]
    fn every_row_shares_one_horizontal_geometry() {
        BOTH.iter().for_each(|layout: &Layout| {
            (0..ROWS).for_each(|index: i32| {
                assert_eq!(layout.row_bar(index).top_left.x, layout.bar_left);
                assert_eq!(
                    layout.row_bar(index).size,
                    Size::new(layout.bar_width, layout.bar_height)
                );
                assert_eq!(layout.row_value_origin(index).x, layout.value_left);
            });
        });
    }

    /// The bar's centre lands on a whole column, so a positive and an equal negative reading
    /// really are mirror images rather than differing by a pixel.
    #[test]
    fn the_bar_centre_lands_on_a_whole_column() {
        BOTH.iter().for_each(|layout: &Layout| {
            assert!(layout.bar_width.is_multiple_of(2));
            assert_eq!(layout.bar_width / 2 * 2, layout.bar_width);
        });
    }

    /// The two shapes are the two ways up of one panel, not two unrelated canvases.
    #[test]
    fn the_portrait_canvas_is_the_landscape_one_turned() {
        assert_eq!(PORTRAIT.canvas.width, LANDSCAPE.canvas.height);
        assert_eq!(PORTRAIT.canvas.height, LANDSCAPE.canvas.width);
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

    /// The field the milli-g values print into is the same six characters in both shapes.
    /// Narrowing it on the narrow canvas is the one saving that would put a wrong *number* on
    /// the glass, so it is asserted rather than assumed.
    #[test]
    fn the_milli_g_field_is_the_same_width_in_both_shapes() {
        let widest: usize = "-8000".len();
        assert!(VALUE_WIDTH > widest, "the widest reading needs headroom");
        BOTH.iter().for_each(|layout: &Layout| {
            let cell_w: u32 = FONT.character_size.width;
            assert!(
                layout.value_left as u32 + cell_w * VALUE_WIDTH as u32 <= layout.canvas.width,
                "the six-character field does not fit this canvas"
            );
        });
    }

    /// The narrow canvas really is narrow: whatever the portrait layout does, it does inside
    /// thirteen columns. Stated as the number rather than derived, so a font change that
    /// silently bought room shows up here.
    #[test]
    fn the_portrait_canvas_holds_thirteen_characters() {
        assert_eq!(PORTRAIT.canvas.width / FONT.character_size.width, 13);
        assert_eq!(LANDSCAPE.canvas.width / FONT.character_size.width, 24);
    }
}
