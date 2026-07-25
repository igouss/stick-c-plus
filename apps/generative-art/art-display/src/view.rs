//! `GalleryView` — the app state the render loop shows: which sketch is on the glass.
//!
//! Where the standalone plume carried no state at all — it was one frond forever — the gallery
//! carries exactly one thing: the [`Sketch`] currently selected. That single field does real
//! work in the render loop's animation contract. It is the view's *anchor*, so when a button
//! press advances the selector to a new sketch, the anchor changes and the loop restarts the
//! animation clock: the new piece begins from the start of its own motion, not wherever the
//! previous one happened to be mid-breath.

use art_core::Sketch;
use platform_core::{Animated, Tick};

/// The gallery's repaint cadence target, in milliseconds — the *ceiling* on how often the loop
/// repaints, not the animation's speed.
///
/// Set at (not below) the render loop's `MIN_YIELD`, the one-tick yield every frame must give the
/// scheduler. So this target never itself throttles the gallery: the loop repaints as fast as a
/// frame's real work plus that mandatory yield allows, which for a full-screen sketch is the true
/// ceiling. A sketch's *own* motion is [`plume_core::phase`], a continuous function of the elapsed
/// clock, so repainting faster changes only smoothness, never speed — the faster the repaint, the
/// more distinct pictures of the same sweep reach the glass.
pub const REPAINT_MS: Tick = 10;

/// The gallery's view: the sketch currently on the glass.
///
/// It is [`Animated`] like every other app's view, so the gallery rides the *same* generic render
/// loop the pomodoro timer and the orientation readout do. Unlike the plume's empty marker, its
/// [`anchor`](Animated::anchor) is the selected sketch — the coarse identity whose change resets
/// the animation clock for a freshly selected piece.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GalleryView {
    /// The sketch this view paints. The whole of the gallery's drawable state.
    current: Sketch,
}

impl GalleryView {
    /// The view showing `current`.
    pub const fn new(current: Sketch) -> Self {
        Self { current }
    }

    /// The sketch on the glass right now — what the [`Gallery`](crate::Gallery) dispatches on.
    pub const fn current(self) -> Sketch {
        self.current
    }
}

impl Default for GalleryView {
    /// A fresh gallery opens on the plume, matching [`Selector::new`](art_core::Selector::new).
    fn default() -> Self {
        Self::new(Sketch::Plume)
    }
}

impl Animated for GalleryView {
    /// The selected sketch. When a press advances it, the anchor changes and the loop restarts the
    /// animation clock — so the new piece animates from the beginning of its own motion rather
    /// than resuming the clock the previous piece left running.
    type Anchor = Sketch;

    fn anchor(&self) -> Self::Anchor {
        self.current
    }

    /// Always: every piece in the gallery is a moving picture, so the loop always paints at its
    /// animation cadence.
    fn is_animated(&self) -> bool {
        true
    }

    /// Which frame is due after `elapsed_ms`: the elapsed millisecond itself. The gallery is a
    /// *continuous* animation — every distinct moment is a distinct picture (see
    /// [`plume_core::phase`]) — so the finest honest frame number is time itself. The render loop
    /// suppresses a repaint only when this `(state, frame)` pair is unchanged; keying it on the
    /// millisecond means a repaint is suppressed only when no time has passed, never merely because
    /// two repaints fell inside some coarser frame window. That is what lets the glass show every
    /// distinct picture the work-bound loop can produce, rather than throttling to a fixed cadence.
    fn frame_index(&self, elapsed_ms: Tick) -> usize {
        elapsed_ms as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every piece animates — the property that keeps the loop on its fast cadence for the whole
    /// gallery.
    #[test]
    fn every_sketch_is_animated() {
        for &sketch in &Sketch::ALL {
            assert!(GalleryView::new(sketch).is_animated(), "{sketch:?}");
        }
    }

    /// The anchor is the selected sketch, so two different sketches have two different anchors —
    /// this is what makes a switch reset the animation clock rather than resume it.
    #[test]
    fn each_sketch_has_its_own_anchor() {
        assert_ne!(
            GalleryView::new(Sketch::Plume).anchor(),
            GalleryView::new(Sketch::Squares).anchor(),
            "a switch must change the anchor, or the clock would not reset"
        );
    }

    /// The same sketch keeps the same anchor across time — a repaint of the current piece must
    /// *not* reset its clock, only a switch does.
    #[test]
    fn the_same_sketch_keeps_its_anchor() {
        let view: GalleryView = GalleryView::new(Sketch::Fan);
        assert_eq!(view.anchor(), GalleryView::new(Sketch::Fan).anchor());
    }

    /// Zero: at the start the first frame is due.
    #[test]
    fn the_first_frame_is_index_zero() {
        assert_eq!(GalleryView::default().frame_index(0), 0);
    }

    /// One, then many: the frame index is the elapsed millisecond, so it advances the instant the
    /// clock does and never collapses two distinct moments into one — every millisecond of the
    /// continuous animation is its own repaintable frame.
    #[test]
    fn the_frame_index_is_the_elapsed_millisecond() {
        let view: GalleryView = GalleryView::default();
        assert_eq!(view.frame_index(1), 1);
        assert_eq!(view.frame_index(2), 2);
        assert_eq!(view.frame_index(999), 999);
    }

    /// A fresh view opens on the plume, so the gallery's first glass matches its first selector
    /// state.
    #[test]
    fn the_default_view_is_the_plume() {
        assert_eq!(GalleryView::default().current(), Sketch::Plume);
    }
}
