//! Clock adapter: the domain's [`Clock`] port backed by the esp-hal monotonic
//! timer, reported as elapsed time since boot.

use core::time::Duration;
use esp_hal::time::Instant;
use led_core::Clock;

/// A monotonic clock measuring time since it was started.
pub struct EspClock {
    start: Instant,
}

impl EspClock {
    /// Anchor the clock at the current instant (call once, after `esp_hal::init`).
    pub fn start() -> Self {
        Self { start: Instant::now() }
    }
}

impl Clock for EspClock {
    fn now(&self) -> Duration {
        let elapsed = Instant::now() - self.start;
        Duration::from_millis(elapsed.as_millis())
    }
}
