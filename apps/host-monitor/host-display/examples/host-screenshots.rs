//! Render every screen the host monitor's glass can show, to PNG.
//!
//! ```sh
//! just screens          # → target/screens/*.png
//! ```
//!
//! The pixels come from `host_display::render` — the same function the ST7789 adapter calls
//! on the board — drawn into a host framebuffer instead of down an SPI bus. So a reviewer
//! looks at the real layout, not at a drawing of it. The scene catalog and the rasteriser are
//! shared with the `goldens` test (see `examples/common/scenes.rs`), so what you eyeball here
//! is exactly what the goldens lock in.
//!
//! **What these images do not show.** Everything below the `DrawTarget`: the panel's colour
//! order, its CGRAM offset, its inversion, its backlight. A host framebuffer paints red as
//! red however the glass is wired. See the crate docs.

use std::fs;
use std::path::{Path, PathBuf};

#[path = "common/scenes.rs"]
mod scenes;

use host_display::SCREEN_SIZE;
use scenes::{render_png, scenes, Screen, SCALE};

/// Where the PNGs land. Under `target/`, so they are build output and git-ignored.
const OUT_DIR: &str = "target/screens";

fn main() {
    let out_dir: &Path = Path::new(OUT_DIR);
    fs::create_dir_all(out_dir).expect("create the screenshot directory");

    let written: Vec<PathBuf> = scenes()
        .iter()
        .map(|screen: &Screen| {
            let path: PathBuf = out_dir.join(screen.file);
            render_png(screen, &path);
            path
        })
        .collect();

    written
        .iter()
        .for_each(|path: &PathBuf| println!("{}", path.display()));
    println!(
        "\n{} host-monitor screens at {}×{} (scaled {SCALE}×) → {OUT_DIR}/",
        written.len(),
        SCREEN_SIZE.width,
        SCREEN_SIZE.height
    );
}
