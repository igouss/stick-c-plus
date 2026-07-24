//! Golden screens — the committed picture of every state, guarded against silent drift.
//!
//! Every display crate renders a catalog of screens (`examples/common/scenes.rs`, shared by its
//! `screenshots` example and its `goldens` test). This crate holds the part that does not vary
//! between them: render each screen, and either overwrite the committed PNG (bless) or assert
//! the fresh bytes match it exactly.
//!
//! Byte comparison is sound because both paths raster through the SAME `render_png` the example
//! uses, at the same scale — a blessed golden and a checked render are produced identically.
//!
//! **A matching golden is not a verdict on the glass.** It proves the layout did not move. It
//! cannot prove the panel's colour order, its CGRAM offset, or that the backlight is powered — a
//! wrong `ColorOrder` shipped unseen in this repo once already, because white-on-black cannot
//! test colour. Only the panel answers that.

use std::fs;
use std::path::{Path, PathBuf};

/// One screen to check: the PNG basename, and how to raster it to a given path.
///
/// The renderer is a closure rather than a trait so each crate keeps its own `Screen` type —
/// a pomodoro screen carries a phase and a rotation, an orientation screen carries a gravity
/// vector — and this crate never learns what any of them mean.
pub struct Golden<'a> {
    /// The PNG basename, e.g. `pomodoro-01-ready.png`. Also the golden's filename.
    pub file: &'a str,
    /// Raster this screen to the given path.
    pub render: Box<dyn Fn(&Path) + 'a>,
}

impl<'a> Golden<'a> {
    /// A screen named `file`, rastered by `render`.
    pub fn new(file: &'a str, render: impl Fn(&Path) + 'a) -> Self {
        Self {
            file,
            render: Box::new(render),
        }
    }
}

/// Whether this run overwrites the goldens (`BLESS_GOLDENS` set) instead of checking them.
pub fn blessing() -> bool {
    std::env::var_os("BLESS_GOLDENS").is_some()
}

/// Render every screen and either overwrite its golden (bless) or assert it is byte-identical
/// to the committed one.
///
/// Reports **every** drifted screen at once rather than failing on the first, so a sweeping
/// change is seen whole. Panics — it is meant to be the body of a `#[test]`.
///
/// - `goldens_dir`: where the committed PNGs live, conventionally `<crate>/goldens/`.
/// - `fresh_dir`: scratch space for this run's renders, conventionally `CARGO_TARGET_TMPDIR`.
pub fn verify<'a>(
    goldens_dir: &Path,
    fresh_dir: &Path,
    screens: impl IntoIterator<Item = Golden<'a>>,
) {
    if blessing() {
        fs::create_dir_all(goldens_dir).expect("create the goldens directory");
    }

    let mut drifted: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    let mut checked: usize = 0;

    for screen in screens {
        checked += 1;
        let golden_path: PathBuf = goldens_dir.join(screen.file);

        if blessing() {
            (screen.render)(&golden_path);
            continue;
        }

        let fresh_path: PathBuf = fresh_dir.join(screen.file);
        (screen.render)(&fresh_path);
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

    // An empty catalog would otherwise pass in silence, which is the one result that looks
    // like success and proves nothing at all.
    assert!(
        checked > 0,
        "no screens to check — the catalog in examples/common/scenes.rs is empty, so this test \
         asserts nothing"
    );
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
