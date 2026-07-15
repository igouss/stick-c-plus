//! Effects — pure animations.
//!
//! An [`Effect`] paints a frame given only the elapsed time. It knows nothing
//! of hardware, timers, or strip length: rendering into a `&mut [Rgb]` slice
//! makes every effect length-agnostic (the empty strip included) and each pixel
//! a pure function of its index and `t`. That locality is what the property
//! tests pin down.

use crate::color::{Hsv, Rgb};
use core::time::Duration;

/// A renderable animation: paint `frame` for the moment `t` (time since the
/// animation began).
pub trait Effect {
    fn render(&mut self, t: Duration, frame: &mut [Rgb]);
}

/// Fills the whole strip with one color — the simplest effect, and the natural
/// zero/one/many test subject.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SolidColor {
    pub color: Rgb,
}

impl SolidColor {
    pub const fn new(color: Rgb) -> Self {
        Self { color }
    }
}

impl Effect for SolidColor {
    fn render(&mut self, _t: Duration, frame: &mut [Rgb]) {
        frame.fill(self.color);
    }
}

/// A hue sweep along the strip that rotates over time — the canonical
/// FastLED / NightDriver rainbow.
///
/// - `spatial`: hue step between adjacent pixels (spreads the rainbow),
/// - `speed`: hue rotation per second (animates it),
/// - `sat` / `val`: saturation and brightness of every pixel.
///
/// Hue is a `u8`, so both the spatial and temporal terms wrap the color wheel
/// naturally.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rainbow {
    pub spatial: u8,
    pub speed: u8,
    pub sat: u8,
    pub val: u8,
}

impl Effect for Rainbow {
    fn render(&mut self, t: Duration, frame: &mut [Rgb]) {
        // Temporal phase: (elapsed_ms * speed / 1000) folded onto the wheel.
        let base: u8 = (t.as_millis().wrapping_mul(self.speed as u128) / 1000) as u8;
        for (i, px) in frame.iter_mut().enumerate() {
            let hue: u8 = base.wrapping_add((i as u8).wrapping_mul(self.spatial));
            *px = Hsv::new(hue, self.sat, self.val).into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // --- SolidColor: zero / one / many, no loops ---

    #[test]
    fn solid_leaves_the_empty_strip_empty() {
        let mut frame: [Rgb; 0] = [];
        SolidColor::new(Rgb::new(1, 2, 3)).render(Duration::ZERO, &mut frame);
        assert_eq!(frame.len(), 0);
    }

    #[test]
    fn solid_fills_a_single_pixel() {
        let mut frame = [Rgb::BLACK; 1];
        SolidColor::new(Rgb::new(1, 2, 3)).render(Duration::ZERO, &mut frame);
        assert_eq!(frame[0], Rgb::new(1, 2, 3));
    }

    #[test]
    fn solid_fills_many_pixels() {
        let mut frame = [Rgb::BLACK; 3];
        SolidColor::new(Rgb::new(9, 8, 7)).render(Duration::ZERO, &mut frame);
        assert_eq!(frame[0], Rgb::new(9, 8, 7));
        assert_eq!(frame[1], Rgb::new(9, 8, 7));
        assert_eq!(frame[2], Rgb::new(9, 8, 7));
    }

    // --- Rainbow ---

    #[test]
    fn rainbow_spreads_distinct_hues() {
        let mut frame = [Rgb::BLACK; 2];
        Rainbow {
            spatial: 85,
            speed: 0,
            sat: 255,
            val: 255,
        }
        .render(Duration::ZERO, &mut frame);
        assert_ne!(frame[0], frame[1]);
    }

    proptest! {
        /// The rule: a pixel depends only on its index and `t`, never on the
        /// strip's length — so a shorter render is a prefix of a longer one.
        #[test]
        fn rainbow_prefix_is_length_independent(len in 1usize..64) {
            let mut short = vec![Rgb::BLACK; len];
            let mut long = vec![Rgb::BLACK; len + 8];
            let t = Duration::from_millis(500);
            Rainbow { spatial: 7, speed: 3, sat: 255, val: 200 }.render(t, &mut short);
            Rainbow { spatial: 7, speed: 3, sat: 255, val: 200 }.render(t, &mut long);
            prop_assert_eq!(&short[..], &long[..len]);
        }
    }
}
