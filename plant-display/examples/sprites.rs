//! Render every vendored sprite to a PNG contact sheet — six frames sampled across each
//! animation's loop.
//!
//! ```sh
//! just sprite-screens        # → target/screens/sprites.png
//! ```
//!
//! This exists because the sprite tests cannot see. "Ink is painted", "scaling squares the
//! area", "nothing escapes the canvas" all pass just as happily if the nibble packing
//! transposed rows and columns, or if the palette were reversed. The creature either looks
//! like a creature here, or the decoder is wrong.

use std::fs;
use std::path::{Path, PathBuf};

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use plant_display::sprite::{self, Frame, Sprite, ALL, SPRITE_SIZE};

const OUT_DIR: &str = "target/screens";
/// Frames sampled per animation — first and last always included.
const COLS: usize = 6;
/// Panel pixels per sprite cell.
const SCALE: u32 = 3;
/// Gap between sprites, in panel pixels.
const GAP: u32 = 6;

/// One sprite cell block: the sprite plus its trailing gap.
const BLOCK: u32 = SPRITE_SIZE as u32 * SCALE + GAP;

/// The `i`th of [`COLS`] frames sampled evenly across `frames`, endpoints included.
fn sampled(frames: &'static [Frame], i: usize) -> &'static Frame {
    let last: usize = frames.len() - 1;
    let index: usize = (i * last).div_ceil(COLS - 1).min(last);
    &frames[index]
}

fn main() {
    let out_dir: &Path = Path::new(OUT_DIR);
    fs::create_dir_all(out_dir).expect("create the screenshot directory");

    let width: u32 = COLS as u32 * BLOCK + GAP;
    let height: u32 = ALL.len() as u32 * BLOCK + GAP;
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(Size::new(width, height));

    ALL.iter()
        .enumerate()
        .for_each(|(row, s): (usize, &&Sprite)| {
            (0..COLS).for_each(|col: usize| {
                let origin: Point = Point::new(
                    (GAP + col as u32 * BLOCK) as i32,
                    (GAP + row as u32 * BLOCK) as i32,
                );
                sprite::draw(&mut display, s, sampled(s.frames(), col), origin, SCALE)
                    .expect("a framebuffer render cannot fail");
            });
        });

    let settings: OutputSettings = OutputSettingsBuilder::new().scale(1).build();
    let path: PathBuf = out_dir.join("sprites.png");
    display
        .to_rgb_output_image(&settings)
        .save_png(&path)
        .expect("save the sprite sheet");

    println!("{}", path.display());
    ALL.iter().for_each(|s: &&Sprite| {
        println!(
            "  {:<20} {:>3} frames  {:>6.2}s  {}",
            s.slug(),
            s.frames().len(),
            f64::from(s.loop_ms()) / 1000.0,
            s.category()
        );
    });
}
