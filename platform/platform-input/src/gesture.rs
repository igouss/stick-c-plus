//! What a button did, and the timing policy that decides it.

use platform_core::Tick;

/// What a button did.
///
/// The vocabulary every app maps onto its own commands. Which of these a given button can
/// actually report is fixed by its [`Gestures`] set — a button that does not ask for
/// [`DoubleClick`](Gesture::DoubleClick) never emits one, and pays nothing for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gesture {
    /// A short press: pressed and released inside [`GestureConfig::hold_ms`]. Reported on
    /// release — or, for a button with [`Gestures::WithDoubleClick`], once its
    /// [`double_click_ms`](GestureConfig::double_click_ms) window has passed with no second click.
    Click,
    /// Two clicks inside [`GestureConfig::double_click_ms`]. Only a button with
    /// [`Gestures::WithDoubleClick`] can report this.
    DoubleClick,
    /// A long press: held for at least [`GestureConfig::hold_ms`]. Reported once, the moment the
    /// threshold passes — while the button is still down — so a hold feels immediate.
    LongHold,
}

/// Which gestures a button reports, and therefore what it costs.
///
/// This is the one place the latency trade is spelled out, because it is a real cost and it
/// should be visible where a button is declared rather than buried in the wiring. A
/// double-click can only be told from a single click by *waiting*: a lone click is confirmed
/// only once the window has passed with no second one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Gestures {
    /// [`Click`](Gesture::Click) and [`LongHold`](Gesture::LongHold), each reported as soon as
    /// it is certain. A click lands on release.
    ///
    /// The default: the cheapest, most responsive button, holding nothing back.
    #[default]
    Prompt,
    /// Adds [`DoubleClick`](Gesture::DoubleClick), at the cost of delaying every lone click by
    /// [`GestureConfig::double_click_ms`].
    WithDoubleClick,
}

/// How long a raw level must hold steady before it is accepted, in milliseconds.
///
/// A mechanical button bounces for a few milliseconds on each edge; 15 ms is comfortably past
/// the bounce yet far below human reaction time, so a real press is never missed and a bounce
/// train never counts twice.
pub const DEBOUNCE_MS: Tick = 15;

/// How long an accepted press must be held to count as a [`LongHold`](Gesture::LongHold) rather
/// than a [`Click`](Gesture::Click), in milliseconds.
pub const HOLD_MS: Tick = 600;

/// How long a [`Click`](Gesture::Click) waits for a second one before it counts as a lone
/// click, in milliseconds.
///
/// 300 ms is a comfortable double-click gap yet short enough that a start/pause still feels
/// prompt. Paid only by a button declared [`Gestures::WithDoubleClick`].
pub const DOUBLE_CLICK_MS: Tick = 300;

/// The three durations that decide every gesture.
///
/// [`Default`] gives the module constants, which is what every app so far wants; the fields are
/// public so a composition root can tune one without forking the recognizer. [`Copy`], so it
/// can be built once and handed to each button.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GestureConfig {
    /// How long a level must hold steady to be accepted.
    pub debounce_ms: Tick,
    /// How long a press must be held to be a [`LongHold`](Gesture::LongHold).
    pub hold_ms: Tick,
    /// How long a lone click waits for a second one.
    pub double_click_ms: Tick,
}

impl Default for GestureConfig {
    fn default() -> Self {
        GestureConfig {
            debounce_ms: DEBOUNCE_MS,
            hold_ms: HOLD_MS,
            double_click_ms: DOUBLE_CLICK_MS,
        }
    }
}
