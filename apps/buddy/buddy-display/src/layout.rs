//! Where everything sits on the buddy's glass, for each of the panel's two shapes.
//!
//! Facts about *this app's* picture. The font and the canvas come from [`platform_display`];
//! the panel's own facts — the CGRAM offset, the colour order, the SPI pins — stay in the
//! driven adapter, the only party that knows them.
//!
//! ## One row grid, regions named by row
//!
//! Every screen the buddy draws — home, the pet pages, the info pages, the clock, an overlay —
//! is text on the *same* grid of rows: [`Layout::rows`] rows, [`Layout::row_y`] apart, starting
//! at [`Layout::top`]. Screens then differ only in which rows they claim. That is what keeps
//! eight screens from becoming eight sets of hand-placed constants that drift a pixel apart:
//! there is one grid, and a screen says *which rows*, never *which pixels*.
//!
//! The home screen claims the top rows for the creature and its status, and the rows from
//! [`Layout::band_first_row`] down for the transcript HUD — which is exactly the band the
//! approval screen takes over while a permission prompt is pending.
//!
//! ## Two shapes, one picture
//!
//! Landscape has 240 px of width, so the creature stands *beside* its status text — a column
//! split. Turned a quarter, that column is gone: 135 px is thirteen characters, and nothing
//! fits beside a 60 px creature. So the portrait layout stacks, and spends the height it gained
//! on the band it made narrower. What does not change is the content.

use embedded_graphics::prelude::*;
use platform_core::ScreenRotation;
use platform_display::{FieldAlign, FONT, LINE_CAP, SCREEN_SIZE};

/// Panel pixels per sprite cell: a 20×20 creature becomes 60×60.
///
/// Three rather than the four the pomodoro timer uses, and the difference is the HUD. The
/// buddy's glass has to hold a creature *and* a readable transcript; an 80 px creature leaves
/// two rows of text under it, and two rows cannot show a wrapped transcript entry and the older
/// one behind it. Sixty pixels is still the thing a glance lands on, and it buys the third row.
pub const SPRITE_SCALE: u32 = 3;

/// How far a 20×20 creature reaches once scaled.
pub const SPRITE_EXTENT: u32 = platform_display::sprite::SPRITE_SIZE as u32 * SPRITE_SCALE;

/// Rows in the status field beside (or below) the creature: the persona, and the link.
pub const STATUS_ROWS: usize = 2;

/// Baseline-to-baseline distance between rows: the font's cell height plus one pixel of air.
const ROW_PITCH: i32 = FONT.character_size.height as i32 + 1;

/// The panel's native landscape canvas.
pub const LANDSCAPE_CANVAS: Size = SCREEN_SIZE;

/// The canvas a quarter turn puts the picture on — the panel's dimensions swapped.
pub const PORTRAIT_CANVAS: Size = Size::new(SCREEN_SIZE.height, SCREEN_SIZE.width);

/// The canvas the buddy is drawn on at `rotation`.
///
/// The one thing outside this crate needs to know about its geometry: a host target has to be
/// allocated at the right shape before the picture is drawn into it, and a target of the wrong
/// shape either clips the picture or reports a correctly-placed pixel as having escaped.
pub const fn canvas_size(rotation: ScreenRotation) -> Size {
    Layout::for_rotation(rotation).canvas
}

/// Where everything sits, for one canvas shape.
///
/// A value rather than a module of constants, so a renderer is *handed* its geometry and cannot
/// reach past it into the other shape's. Both shapes are checked by the compiler — see
/// [`Layout::check`] — so an invalid one fails the build rather than the eye.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layout {
    /// The canvas this layout draws on. Every invariant is measured against it.
    pub canvas: Size,
    /// Text columns across the canvas — the width of a full-bleed field.
    pub cols: usize,
    /// Text rows down the canvas — the grid every screen claims from.
    pub rows: usize,
    /// The y of row 0.
    pub top: i32,
    /// The x of the first text column.
    pub left: i32,
    /// Top-left of the creature on the home screen.
    pub creature_origin: Point,
    /// Top-left of the status field beside (landscape) or below (portrait) the creature.
    pub status_origin: Point,
    /// Width of the status field, in characters.
    pub status_cols: usize,
    /// The first row of the transcript HUD — and of the approval screen that replaces it.
    pub band_first_row: usize,
    /// Where a value sits inside the fixed field it is padded to. Landscape has the creature's
    /// column to align against and reads best flush left; the portrait stack has nothing either
    /// side, where a left-flush short value reads as drifted rather than aligned.
    pub align: FieldAlign,
}

impl Layout {
    /// The layout the picture is drawn through at `rotation`.
    ///
    /// A half turn is still landscape and a three-quarter turn is still portrait: a layout
    /// answers the *shape* of the canvas, and only the panel cares which of the two ways up
    /// that shape is being read.
    pub const fn for_rotation(rotation: ScreenRotation) -> Layout {
        if rotation.is_portrait() {
            PORTRAIT
        } else {
            LANDSCAPE
        }
    }

    /// The y of text row `index`. Rows past [`rows`](Self::rows) are off the canvas; a caller
    /// takes at most that many.
    pub const fn row_y(&self, index: usize) -> i32 {
        self.top + ROW_PITCH * index as i32
    }

    /// The top-left of text row `index`, at the layout's left margin.
    pub const fn row_origin(&self, index: usize) -> Point {
        Point::new(self.left, self.row_y(index))
    }

    /// The top-left of status row `index` — the field beside (landscape) or below (portrait)
    /// the creature. Its own little grid, because it is placed against the *creature* rather
    /// than against the canvas, and so does not sit on the screen-wide row grid.
    pub const fn status_row(&self, index: usize) -> Point {
        Point::new(
            self.status_origin.x,
            self.status_origin.y + ROW_PITCH * index as i32,
        )
    }

    /// The same layout, one character in on every side — the grid an overlay's panel draws on.
    ///
    /// An overlay is drawn *on top of* the screen beneath it, and this inset is what says so: it
    /// leaves a one-character frame of the screen underneath showing on all four sides, which is
    /// the cue that reads as "something is over this" rather than "this is now a different
    /// screen". Held to the same [`check`](Self::check) as the layouts it derives from.
    pub const fn inset(&self) -> Layout {
        Layout {
            cols: self.cols - 2,
            rows: self.rows - 2,
            top: self.top + ROW_PITCH,
            left: self.left + FONT.character_size.width as i32,
            ..*self
        }
    }

    /// The panel an overlay fills: the inset grid's rows, plus a character of padding around
    /// them, in canvas pixels.
    pub const fn panel(&self) -> (Point, Size) {
        let inset: Layout = self.inset();
        let cell_w: u32 = FONT.character_size.width;
        let cell_h: u32 = FONT.character_size.height;
        let at: Point = Point::new(
            inset.left - cell_w as i32 / 2,
            inset.top - cell_h as i32 / 2,
        );
        let size: Size = Size::new(
            cell_w * inset.cols as u32 + cell_w,
            (inset.row_y(inset.rows - 1) + cell_h as i32 + cell_h as i32 / 2 - at.y) as u32,
        );
        (at, size)
    }

    /// How many rows the transcript HUD (or the approval screen) has.
    pub const fn band_rows(&self) -> usize {
        self.rows - self.band_first_row
    }

    /// How many characters wide a band line is: the full width less the column the scroll
    /// indicator stands in.
    pub const fn band_cols(&self) -> usize {
        self.cols - 1
    }

    /// The x of the scroll indicator's track — the column the band gives up.
    pub const fn scroll_x(&self) -> i32 {
        self.left + FONT.character_size.width as i32 * self.band_cols() as i32
    }

    /// The geometry invariants, evaluated by the **compiler** — a field that ran off an edge, a
    /// creature overlapping its status text, or a band with no rows in it fails the *build*, on
    /// host and Xtensa alike, rather than being noticed on the glass.
    ///
    /// A `const fn` rather than a `const` block so that **every** layout is held to it: a third
    /// shape added later is one `const _: () = ITS_NAME.check();` away from the same proof, and
    /// one that skipped the call would be conspicuous.
    pub const fn check(&self) {
        let cell_w: u32 = FONT.character_size.width;
        let cell_h: u32 = FONT.character_size.height;

        assert!(
            self.canvas.width == SCREEN_SIZE.width || self.canvas.width == SCREEN_SIZE.height,
            "the canvas is the panel, at one of its two ways up"
        );
        assert!(
            self.left as u32 + cell_w * self.cols as u32 <= self.canvas.width,
            "a full-width field runs off the right edge"
        );
        assert!(
            self.cols < LINE_CAP,
            "a full-width field is wider than the text primitive's line buffer"
        );
        assert!(self.rows > 0, "the row grid has no rows");
        assert!(
            self.row_y(self.rows - 1) as u32 + cell_h <= self.canvas.height,
            "the last row runs off the bottom edge"
        );
        assert!(
            self.band_first_row < self.rows,
            "the transcript band starts past the last row"
        );

        // The creature stands clear of the canvas and of the status field it sits next to.
        assert!(
            self.creature_origin.x as u32 + SPRITE_EXTENT <= self.canvas.width,
            "the creature runs off the right edge"
        );
        assert!(
            self.creature_origin.y as u32 + SPRITE_EXTENT <= self.canvas.height,
            "the creature runs off the bottom edge"
        );
        assert!(
            self.status_origin.x as u32 + cell_w * self.status_cols as u32 <= self.canvas.width,
            "the status field runs off the right edge"
        );
        assert!(
            self.status_cols < LINE_CAP,
            "the status field is wider than the text primitive's line buffer"
        );
        assert!(
            self.status_row(STATUS_ROWS - 1).y as u32 + cell_h <= self.canvas.height,
            "the last status row runs off the bottom edge"
        );
        assert!(
            self.status_row(STATUS_ROWS - 1).y as u32 + cell_h
                <= self.row_y(self.band_first_row) as u32,
            "the status field runs into the transcript band"
        );
        // The status shares either a clear column with the creature or a clear row below it.
        // Stated as one disjunction rather than two per-shape rules, so a third shape may
        // choose either — which is the whole difference between the two shapes here.
        assert!(
            self.status_origin.x >= self.creature_origin.x + SPRITE_EXTENT as i32
                || self.status_origin.y >= self.creature_origin.y + SPRITE_EXTENT as i32,
            "the status text overlaps the creature — neither beside it nor below it"
        );
        // The band clears the creature, whatever the status does.
        assert!(
            self.row_y(self.band_first_row) >= self.creature_origin.y + SPRITE_EXTENT as i32,
            "the transcript band overlaps the creature"
        );
    }
}

/// The panel held horizontally: 240 px, twenty-four characters a line, six rows.
///
/// The creature stands at the left with its status text beside it — a column split, which is
/// what 240 px of width is for — and the bottom three rows are the transcript HUD.
pub const LANDSCAPE: Layout = Layout {
    canvas: LANDSCAPE_CANVAS,
    cols: 24,
    rows: 6,
    top: 4,
    left: 0,
    creature_origin: Point::new(4, 4),
    status_origin: Point::new(70, 8),
    status_cols: 16,
    band_first_row: 3,
    align: FieldAlign::Left,
};

/// The panel held upright: 135 px, thirteen characters a line, eleven rows.
///
/// Thirteen columns cannot hold a 60 px creature *and* its status side by side, so the picture
/// stacks — creature, status, then a six-row transcript band, down the 240 px this shape
/// gained. Each field is centred on its own width: with nothing to the right to align against,
/// a left-flush stack on a narrow canvas reads as though it had drifted.
pub const PORTRAIT: Layout = Layout {
    canvas: PORTRAIT_CANVAS,
    cols: 13,
    rows: 11,
    top: 4,
    left: 2,
    creature_origin: Point::new(37, 4),
    status_origin: Point::new(2, 67),
    status_cols: 13,
    band_first_row: 5,
    align: FieldAlign::Centred,
};

const _: () = LANDSCAPE.check();
const _: () = PORTRAIT.check();
// The overlay grids are layouts too, and are held to the same invariants — so an inset that
// pushed its last row off the bottom edge fails the build rather than the eye.
const _: () = LANDSCAPE.inset().check();
const _: () = PORTRAIT.inset().check();

/// The two shapes really are the two shapes, checked at build time so a layout claiming a
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

    /// The column count each shape claims is the column count its canvas actually holds —
    /// stated against the font metrics, so a font change cannot silently leave a layout
    /// claiming a column that is no longer there.
    #[test]
    fn each_shape_claims_the_columns_its_canvas_holds() {
        assert_eq!(LANDSCAPE.cols, 24);
        assert_eq!(PORTRAIT.cols, 13);
        BOTH.iter().for_each(|layout: &Layout| {
            let held: u32 = layout.canvas.width / FONT.character_size.width;
            assert!(layout.cols as u32 <= held);
        });
    }

    /// The band has rows in both shapes — the portrait stack has more of them, which is what it
    /// spends its extra height on.
    #[test]
    fn both_shapes_have_a_transcript_band_and_portrait_has_the_deeper_one() {
        assert_eq!(LANDSCAPE.band_rows(), 3);
        assert_eq!(PORTRAIT.band_rows(), 6);
        assert!(PORTRAIT.band_rows() > LANDSCAPE.band_rows());
    }

    /// The band gives exactly one column to the scroll indicator, and the indicator stands
    /// inside the canvas.
    #[test]
    fn the_scroll_indicator_takes_one_column_inside_the_canvas() {
        BOTH.iter().for_each(|layout: &Layout| {
            assert_eq!(layout.band_cols(), layout.cols - 1);
            assert!((layout.scroll_x() as u32) < layout.canvas.width);
        });
    }

    /// Landscape splits by column and portrait by row — the one structural difference between
    /// the two pictures, asserted so a later edit cannot quietly turn one into the other.
    #[test]
    fn landscape_splits_by_column_and_portrait_by_row() {
        assert!(
            LANDSCAPE.status_origin.x >= LANDSCAPE.creature_origin.x + SPRITE_EXTENT as i32,
            "landscape stands the status beside the creature"
        );
        assert!(
            PORTRAIT.status_origin.y >= PORTRAIT.creature_origin.y + SPRITE_EXTENT as i32,
            "portrait stands the status below the creature"
        );
    }

    /// Only the stacked shape centres its text — the rule that keeps a short value from looking
    /// stranded on the narrow canvas.
    #[test]
    fn only_the_stacked_shape_centres_its_text() {
        assert_eq!(LANDSCAPE.align, FieldAlign::Left);
        assert_eq!(PORTRAIT.align, FieldAlign::Centred);
    }

    /// The creature is the same size in both shapes: it is the element a glance lands on, and
    /// the narrow canvas pays for it in height rather than by shrinking it.
    #[test]
    fn the_creature_is_the_same_size_in_both_shapes() {
        assert_eq!(SPRITE_EXTENT, 60);
        BOTH.iter().for_each(|layout: &Layout| {
            assert!(layout.creature_origin.x as u32 + SPRITE_EXTENT <= layout.canvas.width);
            assert!(layout.creature_origin.y as u32 + SPRITE_EXTENT <= layout.canvas.height);
        });
    }

    /// The rows really are a uniform grid, and the last one is on the glass — the invariant
    /// every screen's "claim rows n..m" depends on.
    #[test]
    fn the_row_grid_is_uniform_and_ends_on_the_canvas() {
        assert_eq!(LANDSCAPE.row_y(0), LANDSCAPE.top);
        assert_eq!(LANDSCAPE.row_y(2) - LANDSCAPE.row_y(1), ROW_PITCH);
        BOTH.iter().for_each(|layout: &Layout| {
            let last_bottom: u32 =
                layout.row_y(layout.rows - 1) as u32 + FONT.character_size.height;
            assert!(last_bottom <= layout.canvas.height);
        });
    }
}
