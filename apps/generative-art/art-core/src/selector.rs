//! [`Selector`] — the gallery's running state: which sketch is on the glass.
//!
//! A `Selector` is one [`Sketch`] and the rule for advancing it. A button press calls
//! [`advance`](Selector::advance); the display reads [`current`](Selector::current) to know what
//! to paint. The wrap lives here, in the domain, so the composition root wires a button to
//! `advance` and nothing else, and so the cycle is proven on the host rather than discovered on
//! the glass.

use crate::sketch::Sketch;

/// The gallery's state: the sketch currently on the glass.
///
/// Tiny and `Copy` — it holds a single [`Sketch`] and nothing more. All the running-order logic
/// lives in [`Sketch::next`]; the selector only remembers where the gallery is and steps it
/// forward.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selector {
    current: Sketch,
}

impl Selector {
    /// A fresh gallery, showing the first piece — the plume the standalone app used to be.
    pub const fn new() -> Self {
        Self {
            current: Sketch::Plume,
        }
    }

    /// The sketch on the glass right now.
    pub const fn current(self) -> Sketch {
        self.current
    }

    /// Advance to the next sketch, wrapping after the last back to the first — one button press,
    /// one step through the running order.
    pub fn advance(&mut self) {
        self.current = self.current.next();
    }
}

impl Default for Selector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zero presses: a fresh gallery shows the plume.
    #[test]
    fn a_fresh_gallery_shows_the_plume() {
        assert_eq!(Selector::new().current(), Sketch::Plume);
    }

    /// One press: the gallery steps to the second piece.
    #[test]
    fn one_press_advances_to_the_next_sketch() {
        let mut gallery: Selector = Selector::new();
        gallery.advance();
        assert_eq!(gallery.current(), Sketch::Squares);
    }

    /// Many presses: stepping through the whole order returns to the plume, so the gallery loops
    /// forever rather than running off the end.
    #[test]
    fn stepping_through_the_whole_order_returns_to_the_plume() {
        let mut gallery: Selector = Selector::new();
        for _ in 0..Sketch::ALL.len() {
            gallery.advance();
        }
        assert_eq!(gallery.current(), Sketch::Plume);
    }

    /// A press always changes the picture — there is no piece whose `next` is itself, so every
    /// press is a visible step.
    #[test]
    fn every_press_changes_the_current_sketch() {
        for &start in &Sketch::ALL {
            let mut gallery: Selector = Selector { current: start };
            gallery.advance();
            assert_ne!(gallery.current(), start, "{start:?} advanced to itself");
        }
    }
}
