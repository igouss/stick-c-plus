//! The display port: a driven screen that renders a view of app state.

use crate::tick::Tick;

/// A driven display that renders a view of some app state `S`.
///
/// The generic form of a single-panel display port: where the plant monitor once had a
/// `MoistureDisplay` that spoke `Observation`, this speaks any `S`, so the ST7789 panel
/// adapter serves the plant screen and the pomodoro clock through one trait. The firmware
/// supplies the adapter; the app supplies the picture (how an `S` becomes pixels). The
/// domain depends only on this trait, so dependencies point inward.
///
/// `elapsed` is how long the current state has been shown, in [`Tick`] milliseconds — the
/// animation clock. A screen that only prints a value ignores it; one that animates a
/// creature has the clock it needs without inventing its own. `Error` is associated so an
/// adapter surfaces its own SPI or panel failure without the domain naming a driver error.
pub trait Screen<S> {
    /// The adapter's own render-failure type.
    type Error;

    /// Render `state`, `elapsed` milliseconds after it became the current state.
    fn show(&mut self, state: S, elapsed: Tick) -> Result<(), Self::Error>;
}
