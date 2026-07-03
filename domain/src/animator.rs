//! The animation use case (Control).
//!
//! [`Animator`] owns an [`Effect`] and drives one render tick: paint the frame
//! for a given instant, then hand it to a [`LedOutput`]. It depends only on
//! ports, so it is fully exercisable on the host with fakes — no hardware, no
//! real clock.

use crate::color::Rgb;
use crate::effect::Effect;
use crate::ports::LedOutput;
use core::time::Duration;

/// Drives an effect: render for time `t`, then push the frame downstream.
pub struct Animator<E> {
    effect: E,
}

impl<E: Effect> Animator<E> {
    pub const fn new(effect: E) -> Self {
        Self { effect }
    }

    /// Render one frame for instant `t` into `buf`, then write it to `out`.
    ///
    /// The caller owns `buf` (the domain never allocates) and supplies `t` —
    /// firmware passes `clock.now()`, tests pass a fixed instant.
    pub fn tick<O: LedOutput>(
        &mut self,
        t: Duration,
        buf: &mut [Rgb],
        out: &mut O,
    ) -> Result<(), O::Error> {
        self.effect.render(t, buf);
        out.write(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgb;
    use crate::effect::{Rainbow, SolidColor};
    use crate::ports::Clock;

    /// A pixel sink that remembers the last frame it was handed.
    struct Recorder {
        last: Vec<Rgb>,
    }

    impl LedOutput for Recorder {
        type Error = core::convert::Infallible;

        fn write(&mut self, frame: &[Rgb]) -> Result<(), Self::Error> {
            self.last = frame.to_vec();
            Ok(())
        }
    }

    /// A clock frozen at one instant.
    struct FixedClock(Duration);

    impl Clock for FixedClock {
        fn now(&self) -> Duration {
            self.0
        }
    }

    #[test]
    fn tick_pushes_the_rendered_frame() {
        let mut buf = [Rgb::BLACK; 2];
        let mut out = Recorder { last: Vec::new() };
        let mut anim = Animator::new(SolidColor::new(Rgb::new(4, 5, 6)));

        anim.tick(Duration::ZERO, &mut buf, &mut out).unwrap();

        assert_eq!(out.last, vec![Rgb::new(4, 5, 6), Rgb::new(4, 5, 6)]);
    }

    #[test]
    fn clock_reading_feeds_the_tick() {
        let clock = FixedClock(Duration::from_millis(250));
        let mut buf = [Rgb::BLACK; 1];
        let mut out = Recorder { last: Vec::new() };
        let mut anim = Animator::new(Rainbow {
            spatial: 10,
            speed: 100,
            sat: 255,
            val: 255,
        });

        anim.tick(clock.now(), &mut buf, &mut out).unwrap();

        assert_eq!(out.last.len(), 1);
    }
}
