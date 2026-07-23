//! Golden screens — the committed picture of every screen, guarded against silent drift.
//!
//! Each screen in the shared catalog (`examples/common/scenes.rs`, the same one the
//! `buddy-screenshots` example renders) is rasterised through `buddy_display::render` and
//! compared byte-for-byte against a committed reference PNG under `buddy-display/goldens/`. A
//! change to the layout, the wording, the colours, the wrapping or the scroll indicator flips
//! the bytes and **fails this test** — so a change to the picture is never invisible; a human
//! decides whether it is an improvement (and re-blesses) or a regression (and reverts).
//!
//! Re-bless after an *intended* change:
//! ```sh
//! just screens-bless      # BLESS_GOLDENS=1 — overwrite the committed goldens, then commit
//! ```
//! Both paths raster through the shared `render_png`, so a blessed golden and a checked render
//! are produced identically — the comparison can be exact bytes.
//!
//! **A matching golden is not a verdict on the glass.** It proves the layout did not move; it
//! cannot prove the panel's colour order, its CGRAM offset, or that the backlight is powered. A
//! wrong `ColorOrder` shipped unseen in this repo once already, because white-on-black cannot
//! test colour. Only the panel answers that.

use std::fs;
use std::path::{Path, PathBuf};

#[path = "../examples/common/scenes.rs"]
mod scenes;

use scenes::{render_png, scenes};

/// The committed reference PNGs live beside the crate, in `goldens/`.
fn goldens_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens")
}

/// Whether this run overwrites the goldens (`BLESS_GOLDENS` set) instead of checking them.
fn blessing() -> bool {
    std::env::var_os("BLESS_GOLDENS").is_some()
}

/// Render every catalogued screen and either overwrite its golden (bless) or assert it is
/// byte-identical to the committed one. One test over the catalog: it reports **every** drifted
/// screen at once, so a sweeping change is seen whole, not one failure at a time.
#[test]
fn every_screen_matches_its_committed_golden() {
    let goldens: PathBuf = goldens_dir();
    let fresh_dir: &Path = Path::new(env!("CARGO_TARGET_TMPDIR"));

    if blessing() {
        fs::create_dir_all(&goldens).expect("create the goldens directory");
    }

    let mut drifted: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    for screen in scenes() {
        let golden_path: PathBuf = goldens.join(screen.file);

        if blessing() {
            render_png(&screen, &golden_path);
            continue;
        }

        // Render fresh into the per-test temp dir (under target/), then compare.
        let fresh_path: PathBuf = fresh_dir.join(screen.file);
        render_png(&screen, &fresh_path);
        let fresh: Vec<u8> = fs::read(&fresh_path).expect("read the fresh render");

        match fs::read(&golden_path) {
            Ok(golden) if golden == fresh => {}
            Ok(_) => drifted.push(format!(
                "  {}: committed {} vs new {}",
                screen.file,
                golden_path.display(),
                fresh_path.display()
            )),
            Err(_) => missing.push(format!(
                "  {}: no golden at {}",
                screen.file,
                golden_path.display()
            )),
        }
    }

    if blessing() {
        return; // nothing to assert — we just (re)wrote the goldens.
    }

    assert!(
        missing.is_empty(),
        "golden screen(s) missing — bless them first with `just screens-bless`:\n{}",
        missing.join("\n")
    );
    assert!(
        drifted.is_empty(),
        "the rendered screen(s) changed from their goldens:\n{}\n\nInspect the new render against \
         the committed golden. If the change is intended, re-bless: `just screens-bless`.",
        drifted.join("\n")
    );
}
