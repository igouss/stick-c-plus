//! Where the pomodoro timer's label, clock, and creature sit on the canvas.
//!
//! Facts about *this app's* picture. The font is the board's, from [`platform_display`]; the
//! panel's own facts stay in the driven adapter.
//!
//! ## Why there are two of them
//!
//! There is one [`Layout`] per canvas shape, because the two shapes cannot share a picture.
//! Landscape puts the creature in the right-hand region the two text rows never reach — a
//! column split, which is the only way three things fit across 240 px of width. Turned a
//! quarter, that column is gone: 135 px holds the ten-character label with 35 px to spare, and
//! nothing beside it. So [`PORTRAIT`] stacks instead, label over clock over creature, and
//! spends the height it gained on the split it lost.
//!
//! What does *not* change is the content: the same label field, the same `MM:SS`, the same
//! creature at the same scale. This is one timer held a different way up, not two timers.

use embedded_graphics::prelude::*;
use platform_core::ScreenRotation;
use platform_display::{FieldAlign, FONT, SCREEN_SIZE};

/// Fixed field the label is padded to, so a shorter label erases a longer one. `LONG BREAK`
/// is the widest at 10 characters — which fits the narrow canvas' thirteen columns, so
/// neither shape has to abbreviate a phase name.
pub const LABEL_WIDTH: usize = 10;

/// Fixed field the clock is padded to: `MM:SS` is five characters.
pub const CLOCK_WIDTH: usize = 5;

/// Panel pixels per sprite cell. A 20×20 creature becomes 80×80.
///
/// The same in both shapes. The creature is the thing a glance lands on, and shrinking it on
/// the narrow canvas would have bought margin at the cost of the one element that carries the
/// screen — height is what portrait has to spend, so it spends height instead.
pub const SPRITE_SCALE: u32 = 4;

/// How far a 20×20 creature reaches once scaled.
const SPRITE_EXTENT: u32 = platform_display::sprite::SPRITE_SIZE as u32 * SPRITE_SCALE;

/// The panel's native landscape canvas.
pub const LANDSCAPE_CANVAS: Size = SCREEN_SIZE;

/// The canvas a quarter turn puts the picture on — the panel's dimensions swapped.
pub const PORTRAIT_CANVAS: Size = Size::new(SCREEN_SIZE.height, SCREEN_SIZE.width);

/// The canvas the timer is drawn on at `rotation`.
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
    /// Top-left of the phase label.
    pub label_origin: Point,
    /// Top-left of the `MM:SS` clock.
    pub clock_origin: Point,
    /// Top-left of the creature.
    pub sprite_origin: Point,
    /// Where the label and clock sit inside their padded fields.
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

    /// The geometry invariants, evaluated by the **compiler** — a label that ran off an edge,
    /// a clock overlapping its label, or a creature hanging off the glass fails the *build*,
    /// on host and Xtensa alike, rather than being noticed on the glass.
    ///
    /// A `const fn` rather than a `const` block so that **every** layout is held to it: a
    /// third shape added later is one `const _: () = ITS_NAME.check();` away from the same
    /// proof, and one that skipped the call would be conspicuous.
    pub const fn check(&self) {
        let cell_w: u32 = FONT.character_size.width;
        let cell_h: u32 = FONT.character_size.height;
        let label_right: u32 = self.label_origin.x as u32 + cell_w * LABEL_WIDTH as u32;
        let clock_right: u32 = self.clock_origin.x as u32 + cell_w * CLOCK_WIDTH as u32;

        assert!(
            self.canvas.width == SCREEN_SIZE.width || self.canvas.width == SCREEN_SIZE.height,
            "the canvas is the panel, at one of its two ways up"
        );
        assert!(
            label_right <= self.canvas.width,
            "the widest label runs off the right edge"
        );
        assert!(
            clock_right <= self.canvas.width,
            "the clock runs off the right edge"
        );
        assert!(
            self.label_origin.y as u32 + cell_h <= self.clock_origin.y as u32,
            "the label and clock rows overlap"
        );
        assert!(
            self.clock_origin.y as u32 + cell_h <= self.canvas.height,
            "the clock row runs off the bottom edge"
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
            self.sprite_origin.x as u32 >= label_right
                && self.sprite_origin.x as u32 >= clock_right
                || self.sprite_origin.y as u32 >= self.clock_origin.y as u32 + cell_h,
            "the creature overlaps the text — it shares neither a clear column nor a clear row"
        );
    }
}

/// The panel held horizontally: 240 px of width, twenty-four characters a line.
///
/// The two text rows sit at the left and the creature stands in the right-hand region they
/// never reach — a column split, which is what 240 px of width is for.
pub const LANDSCAPE: Layout = Layout {
    canvas: LANDSCAPE_CANVAS,
    label_origin: Point::new(10, 22),
    clock_origin: Point::new(10, 62),
    sprite_origin: Point::new(152, 30),
    text_align: FieldAlign::Left,
};

/// The panel held upright: 135 px of width, thirteen characters a line.
///
/// Thirteen columns cannot hold a ten-character label *and* an 80 px creature side by side, so
/// the picture stacks: label, clock, creature, down the 240 px this shape gained. Each element
/// is centred on its own width rather than sharing one left margin — with nothing to its right
/// to align against, a left-flush stack on a narrow canvas reads as though it had drifted.
///
/// Both text rows are [`FieldAlign::Centred`], so a `READY` sits in the middle of the same
/// ten-character field a `LONG BREAK` fills. Without that the short labels would hang off the
/// left of the stack while the clock below them sat centred — which is what the first draft
/// looked like, and it read as a bug rather than as a choice.
pub const PORTRAIT: Layout = Layout {
    canvas: PORTRAIT_CANVAS,
    label_origin: Point::new(17, 28),
    clock_origin: Point::new(42, 62),
    sprite_origin: Point::new(27, 124),
    text_align: FieldAlign::Centred,
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

/// Landscape splits by column and portrait by row — the one structural difference between the two
/// pictures, asserted so a later edit cannot quietly turn one into the other.
///
/// A build-time check, not a `#[test]`: both layouts are `const`, so this has exactly one answer
/// and knowing it at compile time is strictly stronger than learning it from a test run.
const _: () = {
    let cell_w: u32 = FONT.character_size.width;
    assert!(
        LANDSCAPE.sprite_origin.x as u32
            >= LANDSCAPE.label_origin.x as u32 + cell_w * LABEL_WIDTH as u32,
        "landscape stands the creature beside the text"
    );
    assert!(
        PORTRAIT.sprite_origin.y > PORTRAIT.clock_origin.y,
        "portrait stands the creature below the text"
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

    /// The creature is the same size in both shapes. It is the element a glance lands on, and
    /// the narrow canvas pays for it in height rather than by shrinking it.
    #[test]
    fn the_creature_is_the_same_size_in_both_shapes() {
        assert_eq!(SPRITE_EXTENT, 80);
        BOTH.iter().for_each(|layout: &Layout| {
            assert!(layout.sprite_origin.x as u32 + SPRITE_EXTENT <= layout.canvas.width);
            assert!(layout.sprite_origin.y as u32 + SPRITE_EXTENT <= layout.canvas.height);
        });
    }

    // The column/row split that used to be asserted here is now a `const _` block beside the two
    // layouts — the same claim, checked by the compiler instead of by a test run.

    /// The narrow canvas really is narrow: whatever the portrait layout does, it does inside
    /// thirteen columns. Stated as the number rather than derived, so a font change that
    /// silently bought room shows up here.
    #[test]
    fn the_portrait_canvas_holds_thirteen_characters() {
        assert_eq!(PORTRAIT.canvas.width / FONT.character_size.width, 13);
        assert_eq!(LANDSCAPE.canvas.width / FONT.character_size.width, 24);
    }

    /// The widest label still fits the narrow canvas — the check that decides whether a phase
    /// name has to be abbreviated in portrait, and it does not.
    #[test]
    fn the_widest_phase_name_fits_the_narrow_canvas() {
        assert_eq!("LONG BREAK".len(), LABEL_WIDTH);
        assert!(LABEL_WIDTH as u32 <= PORTRAIT.canvas.width / FONT.character_size.width);
    }

    /// Landscape aligns its text flush left against the creature's column; portrait, with
    /// nothing beside it, centres. The rule that keeps a short phase name from looking
    /// stranded on the narrow canvas.
    #[test]
    fn only_the_stacked_shape_centres_its_text() {
        assert_eq!(LANDSCAPE.text_align, FieldAlign::Left);
        assert_eq!(PORTRAIT.text_align, FieldAlign::Centred);
    }
}
