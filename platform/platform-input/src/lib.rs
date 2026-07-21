#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # platform-input
//!
//! The M5StickC Plus's buttons, as one pure event source. It names no framework and touches no
//! pin: an app hands it the time, it hands back [`ButtonEvent`]s.
//!
//! ## Three buttons, two ways of reading them
//!
//! The board has three buttons, and — this is the fact the whole design turns on — they are
//! **not read the same way**:
//!
//! | Button | Where | How it is read |
//! |---|---|---|
//! | [`Front`](ButtonId::Front) | G37 | a level, polled: [`ButtonLevel`] |
//! | [`Side`](ButtonId::Side) | G39 | a level, polled: [`ButtonLevel`] |
//! | [`Power`](ButtonId::Power) | AXP192 PEK | a latch, drained: [`LatchedGesture`] |
//!
//! The two GPIO buttons give a raw, bouncing level, and every gesture is decided here in pure
//! code. The power button gives no level at all: it is wired to the PMIC, which does its own
//! debouncing and its own press-duration timing in silicon, and offers the firmware a latched
//! bit saying "a short press happened since you last asked". Forcing that through the levelled
//! port would mean inventing a press history that never existed, so it gets its own port and
//! bypasses the recognizer entirely.
//!
//! Both paths converge on [`ButtonEvent`], so an app writes one match arm per control and never
//! learns which way a gesture arrived.
//!
//! ## The gestures, and what they cost
//!
//! [`Gesture`] is [`Click`](Gesture::Click), [`DoubleClick`](Gesture::DoubleClick) and
//! [`LongHold`](Gesture::LongHold). A double-click is not free: it can only be told from a
//! single click by *waiting*, so a button that reports one delays every lone click by
//! [`DOUBLE_CLICK_MS`]. That trade is declared per button in [`InputConfig`] rather than buried
//! in the wiring — see [`Gestures`] — and a button that did not ask for a double-click has no
//! [`Cadence`] to hold its clicks back and is structurally incapable of emitting one.
//!
//! [`GestureConfig`] carries the three durations, defaulted to the module constants, so a
//! composition root can tune a threshold without forking the recognizer.
//!
//! ## The pieces
//!
//! - [`Buttons`] — the aggregate an app actually uses: owns the three ports,
//!   [`poll`](Buttons::poll)s them, yields [`Events`].
//! - [`Recognizer`] — one levelled button's pipeline: a [`Debounce`], plus a [`Cadence`] exactly
//!   when the button asked for double-clicks.
//! - [`Debounce`] — a bouncing level to a settled click or hold, as a pure fold over time.
//! - [`Cadence`] — a click stream to clicks and double-clicks, likewise.
//!
//! Every stage is a pure function of `(input, now)` and reads no clock, so the entire gesture
//! policy is exercised on the host with a hand-cranked clock and never on the metal.

mod button;
mod buttons;
mod cadence;
mod debounce;
mod gesture;
mod recognizer;

pub use button::{ButtonEvent, ButtonId, ButtonLevel, LatchedGesture};
pub use buttons::{Buttons, Events, InputConfig};
pub use cadence::Cadence;
pub use debounce::Debounce;
pub use gesture::{Gesture, GestureConfig, Gestures, DEBOUNCE_MS, DOUBLE_CLICK_MS, HOLD_MS};
pub use recognizer::Recognizer;
