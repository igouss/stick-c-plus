#![forbid(unsafe_code)]
//! # platform-runtime
//!
//! The board's reusable imperative runtime: the effectful driver a composition root wires
//! around the pure [`platform_core`] ports. Everything here is `std` (a real clock, a
//! background thread), but nothing is ESP-specific — `std::thread`, `Arc`, `Instant` all run
//! on the host under `cargo test` *and* cross-compile to `xtensa-esp32-espidf` unchanged (the
//! esp-idf std maps `std::thread` onto a FreeRTOS task) — so the whole render loop is proven
//! off the metal and a firmware bin only supplies the real [`Screen`](platform_core::Screen)
//! adapter and the app's state `source`.
//!
//! - [`Monotonic`] — the one monotonic millisecond clock, the concrete adapter for the
//!   [`Clock`](platform_core::Clock) port; a composition root makes one and copies it to every
//!   thread so their time bases agree.
//! - [`spawn_display`] — the change-suppressing render thread, generic over any
//!   [`Animated`](platform_core::Animated) app state and [`Screen`](platform_core::Screen)
//!   adapter: it pulls the freshest state from an injected `source`, repaints only when the
//!   `(state, frame)` picture changed, and takes its cadence from whether the creature moves.
//! - [`spawn_power_watch`] — the VBUS-watch thread: poll a
//!   [`PowerSource`](platform_core::PowerSource), debounce it, and sound the
//!   [`PowerChime`](platform_core::PowerChime) a settled transition decides, on an injected
//!   [`Tone`](platform_core::Tone).
//! - [`spawn_buzzer`] — the buzzer's one owner thread: serialises every melody submitted
//!   through its [`BuzzerHandle`] so a chime and a jingle never interleave or truncate one
//!   another.

mod buzzer;
mod clock;
mod display;
mod power_watch;

pub use buzzer::{spawn_buzzer, BuzzerHandle, BuzzerTask, OwnerGone, BUZZER_STACK_SIZE};
pub use clock::Monotonic;
pub use display::{
    spawn_display, DisplayConfig, DisplayTask, ANIMATION_PERIOD, DISPLAY_STACK_SIZE, MIN_YIELD,
    RENDER_PERIOD,
};
pub use power_watch::{
    spawn_power_watch, PowerWatchConfig, PowerWatchTask, POWER_WATCH_PERIOD, POWER_WATCH_STACK_SIZE,
};
