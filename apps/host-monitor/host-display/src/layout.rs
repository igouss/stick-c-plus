//! Where the host monitor's two labelled graphs and its creature sit on the canvas.
//!
//! Facts about *this app's* picture: where each label row and graph plot sit, how wide
//! a field the percentages are padded to, where the creature stands. The canvas size
//! and the font are the board's, not this app's — they come from [`platform_display`].
//! The panel's own facts (offset, inversion, SPI pins) stay in the driven adapter.

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use platform_display::{sprite::SPRITE_SIZE, FONT, SCREEN_SIZE};

/// Baseline-top x inset of both label rows and the left edge of both graphs.
pub const TEXT_X: i32 = 6;

/// Fixed character width both label rows are padded to, via [`platform_display::text_line`].
///
/// `"CPU 100%"` is the widest label, at exactly eight characters; padding a shorter value
/// (`"CPU   5%"`) to this width erases the digits it replaces, so a redraw touches only
/// its own row with no full-screen clear and therefore no flash.
pub const LABEL_WIDTH: usize = 8;

/// Baseline-top y of the CPU label row (above the CPU graph).
pub const CPU_LABEL_Y: i32 = 0;

/// Baseline-top y of the memory label row (above the memory graph).
pub const MEM_LABEL_Y: i32 = 68;

/// One graph column per retained sample — the plot is exactly the history's width.
pub const GRAPH_WIDTH: u32 = 120;
/// The height of each plot, in pixels — the resolution of a percentage (`100/40` ≈ 2.5 %/px).
pub const GRAPH_HEIGHT: u32 = 40;

/// The CPU graph plot — the upper of the two, below the CPU label.
pub const CPU_GRAPH: Rectangle = Rectangle {
    top_left: Point::new(TEXT_X, 22),
    size: Size::new(GRAPH_WIDTH, GRAPH_HEIGHT),
};

/// The memory graph plot — the lower of the two, below the memory label.
pub const MEM_GRAPH: Rectangle = Rectangle {
    top_left: Point::new(TEXT_X, 90),
    size: Size::new(GRAPH_WIDTH, GRAPH_HEIGHT),
};

/// Panel pixels per sprite cell. A 20×20 creature becomes 100×100.
pub const SPRITE_SCALE: u32 = 5;

/// Top-left corner of the creature: the right-hand region the graphs and labels never reach.
pub const SPRITE_ORIGIN: Point = Point::new(132, 17);

/// The geometry invariants, checked by the **compiler**.
///
/// Every term is a constant, so a portrait canvas, overlapping rows, a graph that runs
/// off an edge or into the creature, or a plot whose width has drifted from the history
/// capacity all fail the *build* — on the host and on the Xtensa target alike. That is
/// strictly stronger than a test, which can be filtered or simply not run.
const _: () = {
    let cell_width: u32 = FONT.character_size.width;
    let cell_height: u32 = FONT.character_size.height;
    let sprite_extent: u32 = SPRITE_SIZE as u32 * SPRITE_SCALE;

    assert!(
        SCREEN_SIZE.width > SCREEN_SIZE.height,
        "the canvas is rotated 90° from the panel's native portrait: it must be landscape"
    );
    assert!(
        GRAPH_WIDTH as usize == host_core::history::CAPACITY,
        "the plot is one column per sample: its width must equal the history capacity"
    );

    // Labels fit their field without reaching the creature.
    assert!(
        TEXT_X as u32 + cell_width * LABEL_WIDTH as u32 <= SPRITE_ORIGIN.x as u32,
        "a full-width label would run into the creature"
    );

    // Each label sits above its own graph, and the graphs do not overlap.
    assert!(
        CPU_LABEL_Y as u32 + cell_height <= CPU_GRAPH.top_left.y as u32,
        "the CPU label overlaps its graph"
    );
    assert!(
        CPU_GRAPH.top_left.y as u32 + GRAPH_HEIGHT <= MEM_LABEL_Y as u32,
        "the CPU graph overlaps the memory label"
    );
    assert!(
        MEM_LABEL_Y as u32 + cell_height <= MEM_GRAPH.top_left.y as u32,
        "the memory label overlaps its graph"
    );
    assert!(
        MEM_GRAPH.top_left.y as u32 + GRAPH_HEIGHT <= SCREEN_SIZE.height,
        "the memory graph runs off the bottom edge"
    );

    // The graphs live to the left of the creature.
    assert!(
        TEXT_X as u32 + GRAPH_WIDTH <= SPRITE_ORIGIN.x as u32,
        "the graphs overlap the creature"
    );

    // The creature lives inside the panel's right-hand region.
    assert!(
        SPRITE_ORIGIN.x as u32 + sprite_extent <= SCREEN_SIZE.width,
        "the creature runs off the right edge"
    );
    assert!(
        SPRITE_ORIGIN.y as u32 + sprite_extent <= SCREEN_SIZE.height,
        "the creature runs off the bottom edge"
    );
};
