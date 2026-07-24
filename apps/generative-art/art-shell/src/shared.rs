//! `SharedSelector` — the gallery's selector shared between the input thread and the render loop.

use std::sync::{Arc, Mutex};

use art_core::{Selector, Sketch};

/// The current [`Selector`], shared between the input thread (the one writer, on each button
/// click) and the render loop (the reader, once a frame). Poison-tolerant: a panic in any holder
/// recovers the inner value rather than propagating, so one wedged thread cannot take the other
/// down.
#[derive(Clone)]
pub struct SharedSelector {
    inner: Arc<Mutex<Selector>>,
}

impl SharedSelector {
    /// A fresh gallery, showing the first piece.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Selector::new())),
        }
    }

    /// The sketch on the glass right now — what the render loop's source reads each frame.
    pub fn current(&self) -> Sketch {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .current()
    }

    /// Advance to the next sketch, wrapping — one button click, one step through the running
    /// order.
    pub fn advance(&self) {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .advance();
    }
}

impl Default for SharedSelector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zero clicks: a fresh shared selector shows the plume.
    #[test]
    fn a_fresh_shared_selector_shows_the_plume() {
        assert_eq!(SharedSelector::new().current(), Sketch::Plume);
    }

    /// One click: advancing steps to the next sketch.
    #[test]
    fn advancing_steps_to_the_next_sketch() {
        let shared: SharedSelector = SharedSelector::new();
        shared.advance();
        assert_eq!(shared.current(), Sketch::Squares);
    }

    /// A clone sees the same selector — the writer (input thread) and the reader (render loop)
    /// hold clones of one cache.
    #[test]
    fn a_clone_sees_the_same_selector() {
        let writer: SharedSelector = SharedSelector::new();
        let reader: SharedSelector = writer.clone();
        writer.advance();
        assert_eq!(reader.current(), Sketch::Squares);
    }

    /// Many clicks: stepping through the whole order returns to the plume, so the shared selector
    /// loops forever rather than running off the end.
    #[test]
    fn stepping_through_the_whole_order_returns_to_the_plume() {
        let shared: SharedSelector = SharedSelector::new();
        for _ in 0..Sketch::ALL.len() {
            shared.advance();
        }
        assert_eq!(shared.current(), Sketch::Plume);
    }
}
