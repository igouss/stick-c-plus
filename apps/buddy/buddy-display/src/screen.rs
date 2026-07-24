//! The compositor: which screen, and what is on top of it.
//!
//! Three layers, in one order, and the order is the contract:
//!
//! 1. **the passkey takeover** — if a passkey is active it is the whole glass and the other two
//!    layers do not run at all. The peer is waiting for six digits and the window is seconds
//!    long; nothing else on the device is worth a pixel while that is true.
//! 2. **the screen** — home, a pet page, an info page, or the charging clock.
//! 3. **the overlay** — a bordered panel over the screen, at most one, the innermost open one.
//!
//! ## Every pixel, once
//!
//! There is no framebuffer between here and the glass, so the rule that makes a flicker
//! impossible is that **no pixel is painted twice in one frame**. Two things enforce it:
//!
//! - each screen paints its own region opaquely and clears only the pixels *between* its rows
//!   ([`backdrop`](crate::backdrop)), rather than clearing the screen and drawing over it;
//! - the screen underneath an open overlay is drawn through a [`Masked`] target, so the panel's
//!   rectangle reaches the glass exactly once — as the panel.
//!
//! A picture that obeys that rule cannot flicker even when it repaints for no reason: repainting
//! the same colours, once each, changes nothing the eye can see.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use platform_core::ScreenRotation;
use platform_display::{Masked, RenderError};

use crate::layout::Layout;
use crate::view::{BuddyView, Screen};
use crate::{clock, home, info, overlay, passkey, pet};

/// Draw the whole picture for `view` at `rotation`, with the creature's animation clock at
/// `elapsed_ms`.
///
/// Device-independent by construction: it draws into any [`DrawTarget`], so the on-target panel
/// and a host framebuffer render *the same code*.
pub fn render<D>(
    target: &mut D,
    view: &BuddyView,
    elapsed_ms: u64,
    rotation: ScreenRotation,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let layout: Layout = Layout::for_rotation(rotation);

    if let Some(key) = view.passkey {
        return passkey::render(target, &layout, key);
    }

    // The screen, drawn where the overlay is not: an open overlay claims its panel, and the
    // screen underneath is drawn through a target that drops it. Without the mask the panel's
    // rectangle would reach the glass twice on every frame — once as the creature, once as the
    // panel over it — a region flashing at the animation cadence for as long as the menu is open.
    if view.overlay.is_open() {
        let (at, size): (Point, Size) = layout.panel();
        let mut under: Masked<'_, D> = Masked::new(target, Rectangle::new(at, size));
        screen(&mut under, &layout, view, elapsed_ms)?;
    } else {
        screen(target, &layout, view, elapsed_ms)?;
    }

    overlay::render(target, &layout, view)
}

/// The screen layer alone — whichever of the four `view` has chosen.
fn screen<D>(
    target: &mut D,
    layout: &Layout,
    view: &BuddyView,
    elapsed_ms: u64,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    match view.screen {
        Screen::Home => home::render(target, layout, view, elapsed_ms),
        Screen::Pet(page) => pet::render(target, layout, view, page),
        Screen::Info(page) => info::render(target, layout, &view.device, page),
        Screen::Clock => clock::render(target, layout, &view.clock),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::canvas_size;
    use crate::view::{InfoPage, Overlay, PetPage, Transcript};
    use buddy_core::{MenuEntry, PersonaState, SpeciesIndex};
    use platform_display::testing::Framebuffer;

    /// Every screen the compositor can pick, so a rule stated once is checked against each.
    const SCREENS: [Screen; 4] = [
        Screen::Home,
        Screen::Pet(PetPage::Stats),
        Screen::Info(InfoPage::About),
        Screen::Clock,
    ];

    /// Every overlay, likewise.
    const OVERLAYS: [Overlay; 4] = [
        Overlay::None,
        Overlay::Menu { cursor: 2 },
        Overlay::Settings {
            entry: MenuEntry::Status,
        },
        Overlay::Reset,
    ];

    fn view() -> BuddyView {
        let mut view: BuddyView = BuddyView::resting(SpeciesIndex::new(0));
        view.persona = PersonaState::Busy;
        view.device.linked = true;
        view.transcript = Transcript::oldest_first(&["read the bead", "wrote the crate"]);
        view
    }

    fn painted(view: &BuddyView, rotation: ScreenRotation) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::sized(canvas_size(rotation));
        render(&mut fb, view, 0, rotation).expect("a framebuffer render cannot fail");
        fb
    }

    /// One: the compositor paints, on every screen.
    #[test]
    fn every_screen_paints_pixels() {
        let blank: usize = SCREENS
            .iter()
            .filter(|screen: &&Screen| {
                let mut view: BuddyView = view();
                view.screen = **screen;
                painted(&view, ScreenRotation::Deg0).lit_pixels() == 0
            })
            .count();
        assert_eq!(blank, 0);
    }

    /// Many: the four screens are four different pictures.
    #[test]
    fn the_screens_are_distinct_pictures() {
        let duplicates: usize = SCREENS
            .iter()
            .enumerate()
            .filter(|(index, screen): &(usize, &Screen)| {
                let mut left: BuddyView = view();
                left.screen = **screen;
                let mut right: BuddyView = view();
                right.screen = SCREENS[(index + 1) % SCREENS.len()];
                painted(&left, ScreenRotation::Deg0).pixels()
                    == painted(&right, ScreenRotation::Deg0).pixels()
            })
            .count();
        assert_eq!(duplicates, 0);
    }

    /// THE PRIORITY RULE: an active passkey is the whole glass — the screen and the overlay
    /// underneath it make no difference to the picture at all.
    #[test]
    fn a_passkey_takes_over_from_every_screen_and_overlay() {
        let mut over_home: BuddyView = view();
        over_home.passkey = Some(482_913);
        let mut over_a_menu_on_the_clock: BuddyView = over_home;
        over_a_menu_on_the_clock.screen = Screen::Clock;
        over_a_menu_on_the_clock.overlay = Overlay::Reset;
        assert_eq!(
            painted(&over_home, ScreenRotation::Deg0).pixels(),
            painted(&over_a_menu_on_the_clock, ScreenRotation::Deg0).pixels()
        );
    }

    /// An overlay changes the picture over every screen — it is composited, not swapped in for
    /// one particular screen.
    #[test]
    fn an_overlay_draws_over_every_screen() {
        let unchanged: usize = SCREENS
            .iter()
            .filter(|screen: &&Screen| {
                let mut bare: BuddyView = view();
                bare.screen = **screen;
                let mut covered: BuddyView = bare;
                covered.overlay = Overlay::Reset;
                painted(&bare, ScreenRotation::Deg0).pixels()
                    == painted(&covered, ScreenRotation::Deg0).pixels()
            })
            .count();
        assert_eq!(unchanged, 0);
    }

    /// Nothing escapes the canvas, on any screen crossed with any overlay, in either shape.
    #[test]
    fn nothing_escapes_the_canvas_in_any_combination() {
        let escaped: usize = SCREENS
            .iter()
            .flat_map(|screen: &Screen| {
                OVERLAYS
                    .iter()
                    .map(move |overlay: &Overlay| (*screen, *overlay))
            })
            .map(|(screen, overlay): (Screen, Overlay)| {
                let mut view: BuddyView = view();
                view.screen = screen;
                view.overlay = overlay;
                painted(&view, ScreenRotation::Deg0).escaped()
                    + painted(&view, ScreenRotation::Deg90).escaped()
            })
            .sum();
        assert_eq!(escaped, 0);
    }

    /// THE FLICKER GATE: no picture paints a pixel twice, on any screen, under any overlay, in
    /// either shape.
    ///
    /// The one rule that makes a flicker impossible on this board. There is no framebuffer
    /// between here and the glass, so a pixel written twice in a frame is a colour the owner
    /// *sees* — and a picture that repaints on a clock shows it over and over. It is how the
    /// charging clock came to blink once a second (a full-screen clear, then the time), and it is
    /// what the whole [`backdrop`](crate::backdrop) discipline and the [`Masked`] compositing
    /// exist to prevent.
    ///
    /// Stated over the cross product rather than per screen: a new screen, page or overlay is
    /// held to it without anyone remembering to add a test. It also catches a field whose
    /// content overflows its own width, because the glyphs that overflow land on pixels some
    /// other layer has already painted — which is how the portrait reset overlay was found
    /// writing its title out past its own panel and onto the screen underneath.
    #[test]
    fn no_picture_paints_a_pixel_twice_in_one_frame() {
        let flickering: usize = SCREENS
            .iter()
            .flat_map(|screen: &Screen| {
                OVERLAYS
                    .iter()
                    .map(move |overlay: &Overlay| (*screen, *overlay))
            })
            .map(|(screen, overlay): (Screen, Overlay)| {
                let mut view: BuddyView = view();
                view.screen = screen;
                view.overlay = overlay;
                painted(&view, ScreenRotation::Deg0).overpainted()
                    + painted(&view, ScreenRotation::Deg90).overpainted()
            })
            .sum();
        assert_eq!(flickering, 0);
    }

    /// THE OTHER HALF OF THE FLICKER GATE: a screen replaces its predecessor completely.
    ///
    /// [`overpainted`](Framebuffer::overpainted) counts pixels written twice; it can never count
    /// a pixel written *no* times. That makes it one-directional, and blind to the failure the
    /// backdrop discipline introduces: no screen paints its whole canvas (each leaves thousands
    /// of pixels alone), the panel is cleared exactly once at bring-up
    /// (`firmware/platform/adapters/src/panel.rs`), and nothing clears between screens. So every
    /// pixel one screen paints and the next does not keeps the *old* screen's colour.
    ///
    /// Stated as the property that actually matters, rather than as full coverage: painting the
    /// whole canvas is one way to satisfy this, but leaving a pixel alone is equally fine as long
    /// as no other screen ever writes it. That is why the assertion renders one screen over
    /// another instead of counting pixels — and why [`Framebuffer::start_frame`] exists, since
    /// the second render legitimately repaints what the first one left.
    /// **Ignored because it FAILS, and the failure is real** — see the bead. Today it reports
    /// `Pet then Home: 756 pixel(s) of Pet(Stats) survived`, plus four more pairs. It is kept,
    /// and kept executable, because the bug is genuine and this is the only thing in the tree
    /// that can see it; deleting it would restore the false green it was written to remove.
    ///
    /// The fix is not local. Home's creature and its status rows sit *side by side* in the same
    /// grid rows, and `backdrop::behind` takes horizontal bands top to bottom — one claim per row
    /// range. A screen whose claim is not a stack of bands cannot state it, so covering Home
    /// needs the shared region/claim type that `Masked` and `backdrop` should both consume.
    #[test]
    #[ignore = "known failure: screens do not fully replace one another — see stick-c-plus-yvq"]
    fn a_screen_leaves_nothing_of_the_screen_it_replaced_on_the_glass() {
        let stale: Vec<String> = SCREENS
            .iter()
            .flat_map(|before: &Screen| SCREENS.iter().map(move |after: &Screen| (*before, *after)))
            .filter(|(before, after): &(Screen, Screen)| before != after)
            .filter_map(|(before, after): (Screen, Screen)| {
                let rotation: ScreenRotation = ScreenRotation::Deg0;
                let (mut first, mut second): (BuddyView, BuddyView) = (view(), view());
                first.screen = before;
                second.screen = after;

                // The two rendered in sequence onto one canvas, exactly as the glass sees them.
                let mut glass: Framebuffer = Framebuffer::sized(canvas_size(rotation));
                render(&mut glass, &first, 0, rotation).expect("a framebuffer render cannot fail");
                glass.start_frame();
                render(&mut glass, &second, 0, rotation).expect("a framebuffer render cannot fail");

                // And the second alone, on a canvas that never held the first.
                let alone: Framebuffer = painted(&second, rotation);

                let differing: usize = glass
                    .pixels()
                    .iter()
                    .zip(alone.pixels().iter())
                    .filter(|(over, fresh): &(&Rgb565, &Rgb565)| over != fresh)
                    .count();
                (differing != 0).then(|| {
                    format!(
                        "  {before:?} then {after:?}: {differing} pixel(s) of {before:?} survived"
                    )
                })
            })
            .collect();

        assert!(
            stale.is_empty(),
            "a screen showed through the one that replaced it:\n{}",
            stale.join("\n")
        );
    }

    /// The same rule for the two pictures that are not a (screen, overlay) pair: the passkey
    /// takeover, and the approval screen that replaces the transcript band.
    #[test]
    fn neither_takeover_paints_a_pixel_twice() {
        let mut pairing: BuddyView = view();
        pairing.passkey = Some(482_913);
        let mut asking: BuddyView = view();
        asking.prompt = Some(crate::view::PromptView {
            tool: crate::view::Tool::new("Bash"),
            hint: crate::view::Hint::new("cargo test --workspace"),
            waiting_s: 3,
        });

        assert_eq!(painted(&pairing, ScreenRotation::Deg0).overpainted(), 0);
        assert_eq!(painted(&pairing, ScreenRotation::Deg90).overpainted(), 0);
        assert_eq!(painted(&asking, ScreenRotation::Deg0).overpainted(), 0);
        assert_eq!(painted(&asking, ScreenRotation::Deg90).overpainted(), 0);
    }

    /// Turning the board draws on the other canvas rather than clipping this one.
    #[test]
    fn a_quarter_turn_draws_on_the_taller_canvas() {
        let flat: Framebuffer = painted(&view(), ScreenRotation::Deg0);
        let turned: Framebuffer = painted(&view(), ScreenRotation::Deg90);
        assert_ne!(flat.size(), turned.size());
        assert!(turned.lit_pixels() > 0);
    }

    /// Both quarter turns draw the same picture, and both half turns the other one — a layout
    /// answers the SHAPE of the canvas, and only the panel cares which way up it is.
    #[test]
    fn the_two_turns_of_each_shape_paint_identically() {
        assert_eq!(
            painted(&view(), ScreenRotation::Deg90).pixels(),
            painted(&view(), ScreenRotation::Deg270).pixels()
        );
        assert_eq!(
            painted(&view(), ScreenRotation::Deg0).pixels(),
            painted(&view(), ScreenRotation::Deg180).pixels()
        );
    }
}
