//! Boundary adapters — the driven side of the hexagon.
//!
//! Each adapter implements one domain port against real esp-hal peripherals,
//! one responsibility per file. Nothing here leaks back into `led-core`.

mod clock;
mod ws2812;

pub use clock::EspClock;
pub use ws2812::{buffer_size, Ws2812Rmt, RMT_FREQ_MHZ};
