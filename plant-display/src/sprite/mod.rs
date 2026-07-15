//! Sprites: 20×20 palette-indexed creature animations, and how to draw one.
//!
//! The data in [`generated`] is a vendored copy of the ClaudePix animation library
//! (<https://claudepix.vercel.app/>) — see `kb/sources/claudepix.md` for provenance,
//! licence status, and how the frames were verified against the live site.
//!
//! ## Shape
//!
//! A [`Sprite`] is a palette plus an ordered list of [`Frame`]s, each holding for its
//! own number of milliseconds. Cells are **4-bit palette indices**, two to a byte, so a
//! frame is exactly [`PACKED_LEN`] bytes of `.rodata` — no heap, no decompression, and
//! nothing to initialise at boot.
//!
//! **Index 0 is transparent.** A sprite is drawn over whatever is beneath it; it paints
//! no background of its own. The generator asserts this of every upstream palette.
//!
//! ## Timing lives outside
//!
//! Nothing here knows what time it is. [`Sprite::frame_at`] maps an elapsed-milliseconds
//! value onto a frame, and that is the whole of the animation logic — a pure function of
//! elapsed time, testable on the host without a clock. Who owns the clock, and how often
//! the panel is repainted, is the imperative shell's problem, not this module's.

mod generated;
mod render;

// Each sprite is its own `static`, re-exported by name. A firmware that mentions only
// `sprite::IDLE_BLINK` links only that sprite's ~3 KB; naming [`ALL`] pulls in all 43 KB.
pub use generated::*;
pub use render::{draw, draw_onto};

use embedded_graphics::pixelcolor::Rgb565;

/// A sprite is this many cells on a side.
pub const SPRITE_SIZE: usize = 20;
/// Cells in one frame.
pub const CELLS_PER_FRAME: usize = SPRITE_SIZE * SPRITE_SIZE;
/// Bytes in one packed frame: two 4-bit palette indices per byte.
pub const PACKED_LEN: usize = CELLS_PER_FRAME / 2;
/// The palette index that means "paint nothing here".
pub const TRANSPARENT: u8 = 0;

/// One frame: how long it holds, and its packed 20×20 palette indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    hold_ms: u16,
    packed: &'static [u8; PACKED_LEN],
}

impl Frame {
    /// Build a frame. Called only by the generated data.
    pub const fn new(hold_ms: u16, packed: &'static [u8; PACKED_LEN]) -> Self {
        Frame { hold_ms, packed }
    }

    /// How long this frame stays on the glass.
    pub const fn hold_ms(&self) -> u16 {
        self.hold_ms
    }

    /// The palette index at `(x, y)`, or [`TRANSPARENT`] outside the sprite.
    ///
    /// The high nibble is the even column: cell `2i` is `packed[i] >> 4`.
    pub const fn index_at(&self, x: usize, y: usize) -> u8 {
        if x >= SPRITE_SIZE || y >= SPRITE_SIZE {
            return TRANSPARENT;
        }
        let cell: usize = y * SPRITE_SIZE + x;
        let byte: u8 = self.packed[cell / 2];
        if cell.is_multiple_of(2) {
            byte >> 4
        } else {
            byte & 0x0F
        }
    }
}

/// A named animation: a palette, and the frames that index into it.
#[derive(Clone, Copy, Debug)]
pub struct Sprite {
    slug: &'static str,
    category: &'static str,
    palette: &'static [Option<Rgb565>],
    frames: &'static [Frame],
}

impl Sprite {
    /// Build a sprite. Called only by the generated data.
    pub const fn new(
        slug: &'static str,
        category: &'static str,
        palette: &'static [Option<Rgb565>],
        frames: &'static [Frame],
    ) -> Self {
        Sprite {
            slug,
            category,
            palette,
            frames,
        }
    }

    /// The upstream preset name, e.g. `idle_blink`.
    pub const fn slug(&self) -> &'static str {
        self.slug
    }

    /// The upstream category, e.g. `Idle`.
    pub const fn category(&self) -> &'static str {
        self.category
    }

    /// The frames, in order.
    pub const fn frames(&self) -> &'static [Frame] {
        self.frames
    }

    /// The colour of a palette index, or `None` where the sprite is transparent.
    ///
    /// An index past the end of the palette is transparent rather than a panic: the
    /// generator proves no such index exists, so this is a belt to that brace and never
    /// a silent wrong colour.
    pub fn colour(&self, index: u8) -> Option<Rgb565> {
        self.palette.get(index as usize).copied().flatten()
    }

    /// How long one full loop of the animation takes.
    pub fn loop_ms(&self) -> u32 {
        self.frames
            .iter()
            .map(|f: &Frame| u32::from(f.hold_ms))
            .sum()
    }

    /// Which frame shows at `elapsed_ms` after the animation started, looping forever.
    ///
    /// The entire animation clock, as a pure function — no timer, no mutable cursor, no
    /// frame counter to drift. Callers ask "what should be on the glass now?" and get an
    /// answer that depends on nothing but the argument.
    ///
    /// The *index* rather than the frame, because a render loop compares it against what
    /// it last painted: two equal indices mean there is nothing to redraw.
    pub fn frame_index_at(&self, elapsed_ms: u64) -> usize {
        let loop_ms: u64 = u64::from(self.loop_ms());
        // A sprite always has frames and every hold is positive (the generator asserts
        // both), so `loop_ms` is never zero and this cannot divide by zero.
        let mut offset: u64 = elapsed_ms % loop_ms;
        let mut index: usize = 0;
        while offset >= u64::from(self.frames[index].hold_ms) {
            offset -= u64::from(self.frames[index].hold_ms);
            index += 1;
        }
        index
    }

    /// The frame showing at `elapsed_ms`. See [`Sprite::frame_index_at`].
    pub fn frame_at(&self, elapsed_ms: u64) -> &'static Frame {
        &self.frames[self.frame_index_at(elapsed_ms)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-built sprite, so the timing rules are tested against arithmetic a reader
    /// can do in their head rather than against 216 vendored frames.
    static TWO_FRAME_PACKED: [u8; PACKED_LEN] = [0x00; PACKED_LEN];
    static TWO_FRAMES: [Frame; 2] = [
        Frame::new(100, &TWO_FRAME_PACKED),
        Frame::new(250, &TWO_FRAME_PACKED),
    ];
    static PALETTE: [Option<Rgb565>; 2] = [None, Some(Rgb565::new(31, 0, 0))];
    static TWO: Sprite = Sprite::new("two", "Test", &PALETTE, &TWO_FRAMES);

    #[test]
    fn a_loop_lasts_the_sum_of_its_holds() {
        assert_eq!(TWO.loop_ms(), 350);
    }

    /// Zero: at t=0 the first frame shows.
    #[test]
    fn the_first_frame_shows_at_the_start() {
        assert_eq!(TWO.frame_at(0), &TWO_FRAMES[0]);
    }

    /// One: the boundary belongs to the NEXT frame, not the previous one. An off-by-one
    /// here shows as a frame that never appears, or one that appears twice as long.
    #[test]
    fn a_frame_holds_up_to_but_not_including_its_boundary() {
        assert_eq!(TWO.frame_at(99), &TWO_FRAMES[0]);
        assert_eq!(TWO.frame_at(100), &TWO_FRAMES[1]);
        assert_eq!(TWO.frame_at(349), &TWO_FRAMES[1]);
    }

    /// Many: the animation loops, and keeps looping far out.
    #[test]
    fn the_animation_wraps_at_the_end_of_a_loop() {
        assert_eq!(TWO.frame_at(350), &TWO_FRAMES[0]);
        assert_eq!(TWO.frame_at(450), &TWO_FRAMES[1]);
        assert_eq!(TWO.frame_at(350 * 1000 + 100), &TWO_FRAMES[1]);
    }

    /// The clock must not overflow or panic at the far end of a `u64` of milliseconds. A
    /// `Stale` creature animates for as long as the sampler stays dead, which is
    /// unbounded — so the elapsed value is a `u64`, and this is the value it must survive.
    #[test]
    fn the_animation_survives_the_far_end_of_the_millisecond_clock() {
        let frame: &Frame = TWO.frame_at(u64::MAX);
        assert!(TWO_FRAMES.contains(frame));
    }

    /// The index and the frame agree — the render loop compares indices, the renderer
    /// draws frames, and they must never disagree about which one is showing.
    #[test]
    fn the_frame_index_selects_the_frame() {
        assert_eq!(TWO.frame_index_at(0), 0);
        assert_eq!(TWO.frame_index_at(100), 1);
        assert_eq!(TWO.frame_at(100), &TWO_FRAMES[TWO.frame_index_at(100)]);
    }

    #[test]
    fn index_zero_is_transparent_and_has_no_colour() {
        assert_eq!(TWO.colour(TRANSPARENT), None);
    }

    #[test]
    fn an_index_past_the_palette_is_transparent_rather_than_a_panic() {
        assert_eq!(TWO.colour(15), None);
    }

    #[test]
    fn a_cell_outside_the_sprite_is_transparent() {
        assert_eq!(TWO_FRAMES[0].index_at(SPRITE_SIZE, 0), TRANSPARENT);
        assert_eq!(TWO_FRAMES[0].index_at(0, SPRITE_SIZE), TRANSPARENT);
    }

    /// Nibble order: cell 0 is the HIGH nibble of byte 0, cell 1 the low nibble. If this
    /// flips, every creature renders mirrored in pairs of columns.
    #[test]
    fn the_high_nibble_is_the_even_column() {
        static PACKED: [u8; PACKED_LEN] = {
            let mut p: [u8; PACKED_LEN] = [0; PACKED_LEN];
            p[0] = 0xA3;
            p
        };
        let frame: Frame = Frame::new(1, &PACKED);
        assert_eq!(frame.index_at(0, 0), 0xA);
        assert_eq!(frame.index_at(1, 0), 0x3);
    }

    // ---- The vendored data, pinned against its upstream source. ----
    //
    // These rows are transcribed from the `CREATURE` literal in the site's own
    // `creature-engine.js`, NOT from `gen/frames.json`. So they check the whole chain the
    // generator built — grid to nibbles to `index_at` — against something outside it.
    //
    // A picture cannot do this job. The creature is left-right symmetric almost
    // everywhere, so a transposed or mirrored decode still *looks* like a creature; the
    // contact sheet in `examples/sprites.rs` would show nothing wrong. Row 10 is the
    // asymmetric one (an arm nub on the left at column 3, on the right at column 17), so
    // it is the row that catches a mirror.

    /// `CREATURE[6]` — the eye row: body with an eye at columns 7 and 13.
    const BASE_ROW_6: [u8; SPRITE_SIZE] =
        [0, 0, 0, 0, 0, 1, 1, 2, 1, 1, 1, 1, 1, 2, 1, 1, 0, 0, 0, 0];
    /// `CREATURE[10]` — the arm-nub row, and the only asymmetric one.
    const BASE_ROW_10: [u8; SPRITE_SIZE] =
        [0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0];
    /// `CREATURE[14]` — the four legs.
    const BASE_ROW_14: [u8; SPRITE_SIZE] =
        [0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0];

    /// Read row `y` of a frame back out through the public accessor.
    fn row(frame: &Frame, y: usize) -> [u8; SPRITE_SIZE] {
        core::array::from_fn(|x: usize| frame.index_at(x, y))
    }

    /// `idle_breathe`'s first frame is the base creature held for 500 ms.
    #[test]
    fn the_base_creature_decodes_to_its_upstream_rows() {
        let first: &Frame = &IDLE_BREATHE.frames()[0];
        assert_eq!(first.hold_ms(), 500);
        assert_eq!(row(first, 6), BASE_ROW_6, "the eye row");
        assert_eq!(
            row(first, 10),
            BASE_ROW_10,
            "the arm-nub row (catches a mirror)"
        );
        assert_eq!(row(first, 14), BASE_ROW_14, "the leg row");
    }

    /// The eyes are a different palette entry from the body, and both are opaque. A
    /// palette that collapsed them would render an eyeless creature — which, on a 20×20
    /// sprite viewed at arm's length, is easy to miss.
    #[test]
    fn the_eye_and_the_body_are_distinct_opaque_colours() {
        let body: Option<Rgb565> = IDLE_BREATHE.colour(1);
        let eye: Option<Rgb565> = IDLE_BREATHE.colour(2);
        assert!(body.is_some() && eye.is_some());
        assert_ne!(body, eye);
    }

    #[test]
    fn the_library_holds_every_upstream_preset_under_a_unique_slug() {
        assert_eq!(ALL.len(), 13);
        let mut slugs: Vec<&str> = ALL.iter().map(|s: &&Sprite| s.slug()).collect();
        slugs.sort_unstable();
        let count: usize = slugs.len();
        slugs.dedup();
        assert_eq!(slugs.len(), count, "two sprites share a slug");
    }

    /// Every vendored sprite is well-formed: it has frames, each frame holds for a
    /// positive time, and no cell indexes past its palette.
    #[test]
    fn every_vendored_sprite_is_well_formed() {
        ALL.iter().for_each(|s: &&Sprite| {
            assert!(!s.frames().is_empty(), "{} has no frames", s.slug());
            assert!(s.loop_ms() > 0, "{} loops instantly", s.slug());
            s.frames().iter().for_each(|f: &Frame| {
                assert!(f.hold_ms() > 0, "{} has a zero hold", s.slug());
            });
        });
    }

    /// Index 0 is transparent in every vendored palette — the invariant `draw` relies on
    /// to compose a sprite over the screen instead of boxing it in black.
    #[test]
    fn index_zero_is_transparent_in_every_vendored_palette() {
        ALL.iter().for_each(|s: &&Sprite| {
            assert_eq!(s.colour(TRANSPARENT), None, "{} paints index 0", s.slug());
        });
    }
}
