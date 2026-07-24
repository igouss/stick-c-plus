//! Golden screens — the committed picture of every plant monitor state, guarded against drift.
//!
//! Every screen in the shared catalog (`examples/common/scenes.rs`, the same one the
//! `plant-screenshots` example renders) is rasterised through `plant_display::render` and
//! compared byte-for-byte against a committed reference PNG under `plant-display/goldens/`.
//! A change to the layout, the wording, the colours, the clock or the creature's frame flips the
//! bytes and **fails this test** — so a change to the picture is never invisible; a human decides
//! whether it is an improvement (and re-blesses) or a regression (and reverts).
//!
//! Re-bless after an *intended* change:
//! ```sh
//! just screens-bless      # BLESS_GOLDENS=1 — overwrite the committed goldens, then commit
//! ```
//!
//! The check itself lives in `golden-screens`, shared by every display crate.

use std::path::{Path, PathBuf};

use golden_screens::{verify, Golden};

#[path = "../examples/common/scenes.rs"]
mod scenes;

/// Render every catalogued screen and either overwrite its golden (bless) or assert it is
/// byte-identical to the committed one.
#[test]
fn every_screen_matches_its_committed_golden() {
    let goldens_dir: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("goldens");
    let screens: Vec<scenes::Screen> = scenes::scenes();

    verify(
        &goldens_dir,
        Path::new(env!("CARGO_TARGET_TMPDIR")),
        screens
            .iter()
            .map(|screen: &scenes::Screen| {
                Golden::new(screen.file, |path: &Path| scenes::render_png(screen, path))
            })
            .collect::<Vec<Golden<'_>>>(),
    );
}
