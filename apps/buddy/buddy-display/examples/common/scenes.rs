//! The buddy's screen gallery — every screen the glass can show, and how to raster one.
//!
//! One catalog, two consumers: the `buddy-screenshots` example renders it to `target/screens/`
//! for a human to eyeball, and the `goldens` integration test renders it and compares against
//! the committed reference PNGs so an *unintended* change to the picture fails the build. Both
//! `#[path]`-include this file, so the screens and the rasterisation are defined **once** — the
//! example and the goldens can never drift.
//!
//! It lives under `examples/common/` (not `src/`) on purpose: it is std, uses `Vec`, and pulls
//! the `embedded-graphics-simulator` dev-dependency, none of which belong in the `no_std`
//! library that ships to the board. Cargo does not auto-compile files in an example
//! subdirectory without a `main.rs`, so this is a shared module, not an example of its own.

use std::path::Path;

use buddy_core::{MenuEntry, PersonaState, SpeciesIndex};
use buddy_display::{
    canvas_size, BuddyView, ClockView, DeviceView, Field, Hint, InfoPage, Overlay, PetPage,
    PromptView, Screen as ScreenKind, StatsView, Tool, Transcript,
};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{OutputSettings, OutputSettingsBuilder, SimulatorDisplay};
use platform_core::ScreenRotation;

/// The 240×135 panel is too small to read on a monitor; scale it up so a human — and a golden
/// diff — sees the alignment and the wording, not a postage stamp. Shared so the example and the
/// goldens raster at the identical size (byte-for-byte comparable).
pub const SCALE: u32 = 4;

/// One captioned screen: the file it lands in, the view to paint, the creature's animation
/// clock, and the way up it is drawn.
pub struct Screen {
    /// The PNG basename, e.g. `buddy-01-home.png` — also the golden's name.
    pub file: &'static str,
    /// The view to render.
    pub view: BuddyView,
    /// How long the persona has been current — the creature's animation clock.
    pub elapsed_ms: u64,
    /// Which way up the picture is drawn.
    pub rotation: ScreenRotation,
}

/// A landscape screen — the panel's native way up, and what most of these are.
fn flat(file: &'static str, view: BuddyView, elapsed_ms: u64) -> Screen {
    Screen {
        file,
        view,
        elapsed_ms,
        rotation: ScreenRotation::Deg0,
    }
}

/// A portrait screen: the board stood on its USB-C port, drawn on the taller canvas.
fn turned(file: &'static str, view: BuddyView, elapsed_ms: u64) -> Screen {
    Screen {
        file,
        view,
        elapsed_ms,
        rotation: ScreenRotation::Deg90,
    }
}

/// A linked, busy buddy with a couple of things behind it — the everyday picture the other
/// scenes are variations on.
fn buddy() -> BuddyView {
    let mut view: BuddyView = BuddyView::resting(SpeciesIndex::new(0));
    view.persona = PersonaState::Busy;
    view.sessions_running = 3;
    view.transcript = Transcript::oldest_first(&[
        "Read apps/buddy/buddy-core/src/persona.rs",
        "Bash: cargo test --workspace",
        "Edit apps/buddy/buddy-display/src/hud.rs",
    ]);
    view.stats = StatsView {
        mood: 3,
        fed: 6,
        energy: 4,
        level: 7,
        approvals: 41,
        denials: 3,
        naps: 5,
        nap_minutes: 134,
        tokens_today: 128_400,
    };
    view.clock = ClockView {
        hour: 14,
        minute: 37,
        battery_pct: Some(82),
        charging: true,
    };
    view.device = DeviceView {
        name: Field::new("Claude-4F2A"),
        firmware: Field::new("0.1.0"),
        address: Field::new("A0:B7:65:4F:2A:11"),
        owner: Field::new("elendal"),
        bonded: true,
        linked: true,
    };
    view
}

/// The same buddy with a permission prompt pending, `waiting_s` seconds old.
fn asking(waiting_s: u32) -> BuddyView {
    let mut view: BuddyView = buddy();
    view.persona = PersonaState::Attention;
    view.sessions_waiting = 1;
    view.prompt = Some(PromptView {
        tool: Tool::new("Bash"),
        hint: Hint::new("rm -rf target/xtensa-esp32-espidf/release"),
        waiting_s,
    });
    view
}

/// A screen at one of the info pages.
fn info(page: InfoPage) -> BuddyView {
    let mut view: BuddyView = buddy();
    view.screen = ScreenKind::Info(page);
    view
}

/// Every screen the glass can be in. Adding a screen without adding it here means it ships
/// un-looked-at — and un-golden'd.
///
/// The portrait set is deliberately shorter than the landscape one: what a turn changes is the
/// *shape*, which the layout answers the same way for every screen, so re-shooting all fourteen
/// at a quarter turn would add files without adding a question. What portrait has to answer is
/// whether each element fits and reads on thirteen columns — so it covers the home screen, the
/// approval band, the stats page with its stacking meter, and the clock.
pub fn scenes() -> Vec<Screen> {
    let mut sleepy: BuddyView = buddy();
    sleepy.persona = PersonaState::Sleep;
    sleepy.sessions_running = 0;

    let mut unlinked: BuddyView = buddy();
    unlinked.device.linked = false;
    unlinked.persona = PersonaState::Idle;
    unlinked.sessions_running = 0;
    unlinked.transcript = Transcript::oldest_first(&[]);

    let mut celebrating: BuddyView = buddy();
    celebrating.persona = PersonaState::Celebrate;

    let mut pet_stats: BuddyView = buddy();
    pet_stats.screen = ScreenKind::Pet(PetPage::Stats);
    let mut pet_howto: BuddyView = buddy();
    pet_howto.screen = ScreenKind::Pet(PetPage::HowTo);

    let mut charging: BuddyView = buddy();
    charging.screen = ScreenKind::Clock;

    let mut menu: BuddyView = buddy();
    menu.overlay = Overlay::Menu { cursor: 0 };
    let mut menu_end: BuddyView = buddy();
    menu_end.overlay = Overlay::Menu { cursor: 4 };
    let mut settings: BuddyView = buddy();
    settings.overlay = Overlay::Settings {
        entry: MenuEntry::Status,
    };
    let mut reset: BuddyView = buddy();
    reset.overlay = Overlay::Reset;

    let mut pairing: BuddyView = buddy();
    pairing.passkey = Some(482_913);

    vec![
        flat("buddy-01-home-busy.png", buddy(), 400),
        // The same busy buddy later in the creature's loop — proof it animates.
        flat("buddy-02-home-mid-frame.png", buddy(), 1_200),
        flat("buddy-03-home-sleeping.png", sleepy, 0),
        flat("buddy-04-home-unlinked-quiet.png", unlinked, 0),
        flat("buddy-05-home-celebrating.png", celebrating, 300),
        // The headline screen, warm and then hot.
        flat("buddy-06-approval-warm.png", asking(3), 200),
        flat("buddy-07-approval-hot.png", asking(14), 200),
        flat("buddy-08-pet-stats.png", pet_stats, 0),
        flat("buddy-09-pet-howto.png", pet_howto, 0),
        flat("buddy-10-info-about.png", info(InfoPage::About), 0),
        flat("buddy-11-info-buttons.png", info(InfoPage::Buttons), 0),
        flat("buddy-12-info-claude.png", info(InfoPage::Claude), 0),
        flat("buddy-13-info-device.png", info(InfoPage::Device), 0),
        flat("buddy-14-info-bluetooth.png", info(InfoPage::Bluetooth), 0),
        flat("buddy-15-info-credits.png", info(InfoPage::Credits), 0),
        flat("buddy-16-menu.png", menu, 0),
        // The cursor on the last entry, which the panel cannot show without scrolling.
        flat("buddy-17-menu-scrolled.png", menu_end, 0),
        flat("buddy-18-settings-status.png", settings, 0),
        flat("buddy-19-reset.png", reset, 0),
        flat("buddy-20-pairing-passkey.png", pairing, 0),
        flat("buddy-21-charging-clock.png", charging, 0),
        // Stood on the USB-C port.
        turned("buddy-22-portrait-home.png", buddy(), 400),
        turned("buddy-23-portrait-approval.png", asking(14), 200),
        turned("buddy-24-portrait-pet-stats.png", pet_stats, 0),
        turned("buddy-25-portrait-clock.png", charging, 0),
        turned("buddy-26-portrait-passkey.png", pairing, 0),
    ]
}

/// Rasterise one screen to a PNG at [`SCALE`], through `buddy_display::render` — the very
/// function the ST7789 adapter calls on the board — so the file is the real layout, not a
/// drawing of it. Shared by the example and the goldens, so both produce byte-identical PNGs.
pub fn render_png(screen: &Screen, path: &Path) {
    // Sized from the ROTATION, not from the panel: a portrait screen drawn into a landscape
    // target would be silently clipped and the PNG would look like a layout bug.
    let mut display: SimulatorDisplay<Rgb565> = SimulatorDisplay::new(canvas_size(screen.rotation));
    buddy_display::render(
        &mut display,
        &screen.view,
        screen.elapsed_ms,
        screen.rotation,
    )
    .expect("a framebuffer render cannot fail");
    let settings: OutputSettings = OutputSettingsBuilder::new().scale(SCALE).build();
    display
        .to_rgb_output_image(&settings)
        .save_png(path)
        .expect("save the PNG");
}
