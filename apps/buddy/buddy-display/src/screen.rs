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
//! Every layer paints its own region opaquely, so the whole picture is repainted in place with
//! no full-screen clear between frames and therefore no flash.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_core::ScreenRotation;
use platform_display::RenderError;

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

    match view.screen {
        Screen::Home => home::render(target, &layout, view, elapsed_ms)?,
        Screen::Pet(page) => pet::render(target, &layout, view, page)?,
        Screen::Info(page) => info::render(target, &layout, &view.device, page)?,
        Screen::Clock => clock::render(target, &layout, &view.clock)?,
    }

    overlay::render(target, &layout, view)
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
