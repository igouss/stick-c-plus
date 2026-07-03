//! Boundary adapters — the driven side of the hexagon.
//!
//! Each adapter implements one domain port against real esp-hal peripherals,
//! one responsibility per file. Nothing here leaks back into `led-core`.

mod clock;
mod strip;

pub use clock::EspClock;
pub use strip::Ws2812Strip;
