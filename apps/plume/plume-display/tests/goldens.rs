//! Golden screens — the committed picture of the plume's breath, guarded against drift.
//!
//! Every still in the shared catalog (`examples/common/scenes.rs`, the same one the
//! `plume-screenshots` example renders) is rasterised through `Plume::render` and compared
//! byte-for-byte against a committed reference PNG under `plume-display/goldens/`. A change to
//! the frond geometry, the colours, or the breathing maths flips the bytes and **fails this
//! test** — so a change to the picture is never invisible; a human decides whether it is an
//! improvement (and re-blesses) or a regression (and reverts).
//!
//! This is the only check the plume can have that looks at the picture: it has no state machine
//! to unit-test, just a clock and some trigonometry, so pinning the shape at fixed points of the
//! period is what stops the frond silently changing.
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

/// Render every catalogued still and either overwrite its golden (bless) or assert it is
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
                Golden::new(screen.file.as_str(), |path: &Path| {
                    scenes::render_png(screen, path)
                })
            })
            .collect::<Vec<Golden<'_>>>(),
    );
}
