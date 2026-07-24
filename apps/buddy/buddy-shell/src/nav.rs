//! Screen navigation: which page the two buttons walk to next.
//!
//! Pure, and separate from the domain on purpose. `buddy_core` owns the *menu* — a settings tree
//! with its own dispatch table — while this owns which **screen** is showing, which is a
//! presentation concern the domain has no opinion about.
//!
//! ## Two buttons, and one of them is spoken for
//!
//! A is the approve button. It has to be free the instant a prompt lands, so at rest it does the
//! smallest possible thing — turns the page *within* the current screen — and never navigates
//! away from where the owner left the glass. B is the one that walks the screens. A held is the
//! menu.
//!
//! That asymmetry is the whole design: a stray press on the answer button must never move the
//! screen out from under an approval that is about to arrive.

use buddy_display::{InfoPage, PetPage, Screen};
use platform_input::{ButtonEvent, ButtonId, Gesture};

/// The screens B walks through, in order.
///
/// The clock is in the ring because it is the owner's to walk to. Nothing puts it there on their
/// behalf except a battery about to die — a stick that answered a charger by hiding its creature
/// behind a clock spent the glass on a fact the owner already had.
const RING: [Screen; 4] = [
    Screen::Home,
    Screen::Pet(PetPage::Stats),
    Screen::Info(InfoPage::About),
    Screen::Clock,
];

/// Which screen is showing, and where within it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Nav {
    screen: Screen,
}

impl Nav {
    /// At home, where the buddy starts.
    pub const fn new() -> Self {
        Nav {
            screen: Screen::Home,
        }
    }

    /// The screen to render.
    pub const fn screen(&self) -> Screen {
        self.screen
    }

    /// Force a screen — how the clock takes the glass when the battery is about to die.
    pub fn show(&mut self, screen: Screen) {
        self.screen = screen;
    }

    /// Fold one button event into the navigation, answering whether it was consumed.
    ///
    /// A click turns the page within the screen; B click walks to the next screen. Everything
    /// else — a hold, the power button — is left for the caller, which is what lets the menu and
    /// the approval both keep first claim on the same two buttons.
    pub fn press(&mut self, event: ButtonEvent) -> bool {
        match (event.button, event.gesture) {
            (ButtonId::Front, Gesture::Click) => {
                self.screen = next_page(self.screen);
                true
            }
            (ButtonId::Side, Gesture::Click) => {
                self.screen = next_screen(self.screen);
                true
            }
            _ => false,
        }
    }
}

/// The next page within a screen, wrapping. Home and the clock have one page each, so A does
/// nothing there — which is the point: the answer button stays quiet at rest.
const fn next_page(screen: Screen) -> Screen {
    match screen {
        Screen::Pet(PetPage::Stats) => Screen::Pet(PetPage::HowTo),
        Screen::Pet(PetPage::HowTo) => Screen::Pet(PetPage::Stats),
        Screen::Info(page) => Screen::Info(next_info(page)),
        other => other,
    }
}

/// The next info page, wrapping through all six.
const fn next_info(page: InfoPage) -> InfoPage {
    match page {
        InfoPage::About => InfoPage::Buttons,
        InfoPage::Buttons => InfoPage::Claude,
        InfoPage::Claude => InfoPage::Device,
        InfoPage::Device => InfoPage::Bluetooth,
        InfoPage::Bluetooth => InfoPage::Credits,
        InfoPage::Credits => InfoPage::About,
    }
}

/// The next screen in the ring, entering each at its first page.
///
/// A screen not in the ring — which cannot happen today, and would if a screen were added
/// without adding it here — walks to the first, so B always leads somewhere.
fn next_screen(screen: Screen) -> Screen {
    let at: usize = RING
        .iter()
        .position(|candidate: &Screen| same_screen(*candidate, screen))
        .unwrap_or(RING.len() - 1);
    RING[(at + 1) % RING.len()]
}

/// Whether two screens are the same screen, ignoring which page of it is showing.
const fn same_screen(left: Screen, right: Screen) -> bool {
    matches!(
        (left, right),
        (Screen::Home, Screen::Home)
            | (Screen::Pet(_), Screen::Pet(_))
            | (Screen::Info(_), Screen::Info(_))
            | (Screen::Clock, Screen::Clock)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const A_CLICK: ButtonEvent = ButtonEvent::new(ButtonId::Front, Gesture::Click);
    const B_CLICK: ButtonEvent = ButtonEvent::new(ButtonId::Side, Gesture::Click);
    const A_HOLD: ButtonEvent = ButtonEvent::new(ButtonId::Front, Gesture::LongHold);

    fn after(events: &[ButtonEvent]) -> Nav {
        let mut nav: Nav = Nav::new();
        events.iter().for_each(|event: &ButtonEvent| {
            assert!(nav.press(*event) || event.gesture != Gesture::Click)
        });
        nav
    }

    /// Zero: a fresh buddy is at home.
    #[test]
    fn a_fresh_buddy_is_at_home() {
        assert_eq!(Nav::new().screen(), Screen::Home);
    }

    /// THE ASYMMETRY: A does not navigate away from home. The answer button must not move the
    /// screen out from under an approval that is about to arrive.
    #[test]
    fn the_answer_button_does_not_leave_the_home_screen() {
        assert_eq!(after(&[A_CLICK, A_CLICK, A_CLICK]).screen(), Screen::Home);
    }

    /// One: B walks to the next screen.
    #[test]
    fn b_walks_to_the_next_screen() {
        assert_eq!(after(&[B_CLICK]).screen(), Screen::Pet(PetPage::Stats));
    }

    /// Many: B walks the whole ring and comes back home.
    #[test]
    fn b_walks_the_ring_and_returns_home() {
        assert_eq!(
            after(&[B_CLICK, B_CLICK]).screen(),
            Screen::Info(InfoPage::About)
        );
        assert_eq!(after(&[B_CLICK, B_CLICK, B_CLICK]).screen(), Screen::Clock);
        assert_eq!(
            after(&[B_CLICK, B_CLICK, B_CLICK, B_CLICK]).screen(),
            Screen::Home
        );
    }

    /// A turns the page within a screen that has pages, and B leaves at whichever page it is on.
    #[test]
    fn a_turns_the_page_within_a_screen() {
        assert_eq!(
            after(&[B_CLICK, A_CLICK]).screen(),
            Screen::Pet(PetPage::HowTo)
        );
        assert_eq!(
            after(&[B_CLICK, A_CLICK, A_CLICK]).screen(),
            Screen::Pet(PetPage::Stats)
        );
    }

    /// All six info pages are reachable, and the sixth wraps to the first — the check a
    /// hand-written cycle gets wrong at exactly one end.
    #[test]
    fn every_info_page_is_reachable_and_the_last_wraps() {
        assert_eq!(next_info(InfoPage::Credits), InfoPage::About);
        let walked: usize = InfoPage::ALL
            .iter()
            .filter(|page: &&InfoPage| InfoPage::ALL.contains(&next_info(**page)))
            .count();
        assert_eq!(walked, InfoPage::ALL.len());
    }

    /// A hold is not consumed here — it belongs to the menu, which is the domain's business.
    #[test]
    fn a_hold_is_left_for_the_menu() {
        let mut nav: Nav = Nav::new();
        assert!(!nav.press(A_HOLD));
        assert_eq!(nav.screen(), Screen::Home);
    }

    /// A forced screen is shown, and B walks on from it — how the clock takes the glass
    /// and gives it back.
    #[test]
    fn a_forced_screen_is_shown_and_walked_on_from() {
        let mut nav: Nav = Nav::new();
        nav.show(Screen::Clock);
        assert_eq!(nav.screen(), Screen::Clock);
        nav.press(B_CLICK);
        assert_eq!(nav.screen(), Screen::Home);
    }
}
