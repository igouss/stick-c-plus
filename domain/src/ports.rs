//! Ports — the interfaces the domain requires of the outside world.
//!
//! These are the driven side of the hexagon. The firmware supplies adapters
//! (an esp-hal timer for [`Clock`], an RMT WS2812 driver for [`LedOutput`]);
//! the domain depends only on these traits, so dependencies point inward.

use crate::color::Rgb;
use core::time::Duration;

/// A monotonic time source, measured from the animation's start.
pub trait Clock {
    fn now(&self) -> Duration;
}

/// A sink that pushes a rendered frame to physical LEDs.
///
/// `Error` is associated so an adapter can surface its own failure type without
/// the domain naming any concrete driver error.
pub trait LedOutput {
    type Error;

    fn write(&mut self, frame: &[Rgb]) -> Result<(), Self::Error>;
}
