//! WS2812 output adapter: the domain's [`LedOutput`] port bridged to a
//! `smart-leds` sink.
//!
//! Generic over the sink `A` so the composition root owns the concrete RMT
//! types while this file stays a pure translation — domain [`Rgb`] to wire
//! `RGB8`. That also makes it unit-testable against a fake `SmartLedsWrite`.

use led_core::{LedOutput, Rgb};
use smart_leds::{SmartLedsWrite, RGB8};

/// Adapts a `smart-leds` sink to the domain's [`LedOutput`] port.
pub struct Ws2812Strip<A> {
    sink: A,
}

impl<A> Ws2812Strip<A> {
    pub const fn new(sink: A) -> Self {
        Self { sink }
    }
}

impl<A> LedOutput for Ws2812Strip<A>
where
    A: SmartLedsWrite,
    // We emit RGB; the sink reorders to its wire format (WS2812 is GRB).
    RGB8: Into<A::Color>,
{
    type Error = A::Error;

    fn write(&mut self, frame: &[Rgb]) -> Result<(), Self::Error> {
        self.sink
            .write(frame.iter().map(|c| RGB8::new(c.r, c.g, c.b)))
    }
}
