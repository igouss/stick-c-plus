//! Where the pomodoro timer's label, clock, and creature sit on the canvas.
//!
//! Facts about *this app's* picture. The canvas size and the font are the board's, from
//! [`platform_display`]; the panel's own facts stay in the driven adapter.

use platform_display::{FONT, SCREEN_SIZE};

use embedded_graphics::prelude::*;

/// Top-left of the phase label (the upper row).
pub const LABEL_ORIGIN: Point = Point::new(10, 22);
/// Fixed field the label is padded to, so a shorter label erases a longer one. `LONG BREAK`
/// is the widest at 10 characters.
pub const LABEL_WIDTH: usize = 10;

/// Top-left of the `MM:SS` clock (the lower row).
pub const CLOCK_ORIGIN: Point = Point::new(10, 62);
/// Fixed field the clock is padded to: `MM:SS` is five characters.
pub const CLOCK_WIDTH: usize = 5;

/// Panel pixels per sprite cell. A 20×20 creature becomes 80×80.
pub const SPRITE_SCALE: u32 = 4;
/// Top-left of the creature: the right-hand region the text rows never reach.
pub const SPRITE_ORIGIN: Point = Point::new(152, 30);

/// The geometry invariants, checked by the **compiler** — a label or clock that ran off an
/// edge, or a creature that overlapped the text, would fail the *build*, on host and Xtensa
/// alike.
const _: () = {
    let cell_w: u32 = FONT.character_size.width;
    let cell_h: u32 = FONT.character_size.height;
    let sprite_extent: u32 = platform_display::sprite::SPRITE_SIZE as u32 * SPRITE_SCALE;

    assert!(
        SCREEN_SIZE.width > SCREEN_SIZE.height,
        "the canvas is landscape once rotated"
    );
    assert!(
        LABEL_ORIGIN.x as u32 + cell_w * LABEL_WIDTH as u32 <= SPRITE_ORIGIN.x as u32,
        "the widest label runs into the creature"
    );
    assert!(
        LABEL_ORIGIN.y as u32 + cell_h <= CLOCK_ORIGIN.y as u32,
        "the label and clock rows overlap"
    );
    assert!(
        CLOCK_ORIGIN.y as u32 + cell_h <= SCREEN_SIZE.height,
        "the clock row runs off the bottom edge"
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
