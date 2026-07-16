//! Where the host monitor's three host rows and their sparklines sit on the canvas.
//!
//! Facts about *this app's* picture: the panel is 240×135 landscape, and it shows one row
//! per homelab host — `fedora`, `oracle-arm`, `oracle-amd`. Each row is a header line (the
//! name and the two current percentages) above two side-by-side sparklines (CPU on the
//! left, memory on the right). The canvas size and the font are the board's, not this
//! app's — they come from [`platform_display`]. The panel's own facts (offset, inversion,
//! SPI pins) stay in the driven adapter.
//!
//! Every measurement here is a constant, and a `const _: ()` block at the bottom asserts
//! the geometry fits — so a row that ran off an edge, a field that overlapped its
//! neighbour, or a graph wider than its column would fail the *build*, on the host and on
//! the Xtensa target alike.

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use platform_display::{FONT, SCREEN_SIZE};

/// How many host rows the panel shows — the homelab's three hosts.
///
/// The frame may carry up to [`host_core::MAX_HOSTS`]; the panel fits exactly this many
/// rows, and the compile-time block below proves the frame can supply them.
pub const ROWS: usize = 3;

/// The height of one host row, in pixels. `ROWS * ROW_HEIGHT` is the full canvas height.
pub const ROW_HEIGHT: u32 = 45;

/// Left inset of the name field and the left sparkline.
pub const X_MARGIN: i32 = 2;

/// Character width the host-name field is padded to — full `oracle-arm` (ten characters),
/// so the two `oracle-*` hosts stay distinguishable.
pub const NAME_CHARS: usize = 10;

/// Character width of a percentage field — `"100%"`, right-aligned.
pub const NUM_CHARS: usize = 4;

/// Baseline-top x of the CPU percentage on the header line.
pub const CPU_NUM_X: i32 = 108;
/// Baseline-top x of the memory percentage on the header line.
pub const MEM_NUM_X: i32 = 152;

/// Baseline-top x of the frame-status token (top row only) — `DOWN` / `OLD`.
pub const STATUS_X: i32 = 196;
/// Character width of the frame-status token.
pub const STATUS_CHARS: usize = 4;

/// Character width of the "no data" label a down host shows in place of its numbers.
pub const NODATA_CHARS: usize = 9;

/// Top offset of the sparklines within a row, below the header line.
pub const GRAPH_TOP: u32 = 23;
/// Height of each sparkline, in pixels.
pub const GRAPH_HEIGHT: u32 = 20;
/// Width of each sparkline, in pixels (and the column count of the plot scratch buffer).
pub const GRAPH_WIDTH: usize = 116;

/// Left edge of the CPU sparkline within a row.
pub const CPU_GRAPH_X: i32 = X_MARGIN;
/// Left edge of the memory sparkline within a row (to the right of the CPU one).
pub const MEM_GRAPH_X: i32 = 122;

/// The top-left y of row `i`.
pub const fn row_top(i: usize) -> i32 {
    (i as u32 * ROW_HEIGHT) as i32
}

/// Where row `i`'s header line is drawn (name, then percentages).
pub const fn header_origin(i: usize) -> Point {
    Point::new(X_MARGIN, row_top(i))
}

/// Row `i`'s CPU sparkline plot.
pub const fn cpu_graph(i: usize) -> Rectangle {
    Rectangle {
        top_left: Point::new(CPU_GRAPH_X, row_top(i) + GRAPH_TOP as i32),
        size: Size::new(GRAPH_WIDTH as u32, GRAPH_HEIGHT),
    }
}

/// Row `i`'s memory sparkline plot.
pub const fn mem_graph(i: usize) -> Rectangle {
    Rectangle {
        top_left: Point::new(MEM_GRAPH_X, row_top(i) + GRAPH_TOP as i32),
        size: Size::new(GRAPH_WIDTH as u32, GRAPH_HEIGHT),
    }
}

/// The geometry invariants, checked by the **compiler**.
///
/// Every term is a constant, so a portrait canvas, rows that overrun the screen, a header
/// field that laps its neighbour, or a plot wider than its column all fail the *build*.
/// That is strictly stronger than a test, which can be filtered or simply not run.
const _: () = {
    let cw: u32 = FONT.character_size.width;
    let ch: u32 = FONT.character_size.height;

    assert!(
        SCREEN_SIZE.width > SCREEN_SIZE.height,
        "the canvas is rotated 90° from the panel's native portrait: it must be landscape"
    );
    assert!(
        host_core::MAX_HOSTS >= ROWS,
        "the frame must be able to supply a host for every row the panel shows"
    );
    assert!(
        ROWS as u32 * ROW_HEIGHT <= SCREEN_SIZE.height,
        "the host rows run off the bottom edge"
    );

    // The header line sits above the sparklines within a row.
    assert!(ch <= GRAPH_TOP, "the header line overlaps its sparklines");
    assert!(
        GRAPH_TOP + GRAPH_HEIGHT <= ROW_HEIGHT,
        "a row's sparklines run into the next row"
    );

    // Header fields do not overlap, and fit before the right edge.
    assert!(
        X_MARGIN as u32 + NAME_CHARS as u32 * cw <= CPU_NUM_X as u32,
        "the name field runs into the CPU percentage"
    );
    assert!(
        CPU_NUM_X as u32 + NUM_CHARS as u32 * cw <= MEM_NUM_X as u32,
        "the CPU percentage runs into the memory percentage"
    );
    assert!(
        MEM_NUM_X as u32 + NUM_CHARS as u32 * cw <= STATUS_X as u32,
        "the memory percentage runs into the status token"
    );
    assert!(
        STATUS_X as u32 + STATUS_CHARS as u32 * cw <= SCREEN_SIZE.width,
        "the status token runs off the right edge"
    );
    // A down host's "no data" label replaces the numbers; it must also fit.
    assert!(
        CPU_NUM_X as u32 + NODATA_CHARS as u32 * cw <= SCREEN_SIZE.width,
        "the 'no data' label runs off the right edge"
    );

    // The two sparklines sit side by side without overlapping, inside the canvas.
    assert!(
        CPU_GRAPH_X as u32 + GRAPH_WIDTH as u32 <= MEM_GRAPH_X as u32,
        "the CPU sparkline overlaps the memory sparkline"
    );
    assert!(
        MEM_GRAPH_X as u32 + GRAPH_WIDTH as u32 <= SCREEN_SIZE.width,
        "the memory sparkline runs off the right edge"
    );
};
