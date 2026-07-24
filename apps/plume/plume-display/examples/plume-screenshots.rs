//! Render the plume across one breath, to PNG.
//!
//! ```sh
//! just screens          # → target/screens/plume-*.png
//! ```
//!
//! The pixels come from `Plume::render` — the same function the ST7789 adapter calls on the
//! board — drawn into a host framebuffer instead of down an SPI bus. So a reviewer looks at the
//! real picture, not at a drawing of it. The still catalog and the rasteriser are shared with the
//! `goldens` test (see `examples/common/scenes.rs`), so what you eyeball here is exactly what the
//! goldens lock in.
//!
//! **What these images do not show.** Everything below the `DrawTarget`: the panel's colour
//! order, its CGRAM offset, its inversion, its backlight. A host framebuffer paints green as
//! green however the glass is wired.

use std::fs;
use std::path::{Path, PathBuf};

#[path = "common/scenes.rs"]
mod scenes;

use plume_display::canvas_size;
use scenes::{render_png, scenes, Screen, PORTRAIT, SCALE};

/// Where the PNGs land. Under `target/`, so they are build output and git-ignored.
const OUT_DIR: &str = "target/screens";

fn main() {
    let out_dir: &Path = Path::new(OUT_DIR);
    fs::create_dir_all(out_dir).expect("create the screenshot directory");

    let written: Vec<PathBuf> = scenes()
        .iter()
        .map(|screen: &Screen| {
            let path: PathBuf = out_dir.join(&screen.file);
            render_png(screen, &path);
            path
        })
        .collect();

    written
        .iter()
        .for_each(|path: &PathBuf| println!("{}", path.display()));
    let canvas = canvas_size(PORTRAIT);
    println!(
        "\n{} plume phases at {}×{} (scaled {SCALE}×) → {OUT_DIR}/",
        written.len(),
        canvas.width,
        canvas.height
    );
}
