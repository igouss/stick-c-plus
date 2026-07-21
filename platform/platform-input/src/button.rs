//! The board's three buttons, and the two ports they are read through.

use crate::gesture::Gesture;

/// Which button a gesture came from.
///
/// Deliberately **physical**, not semantic: `Front` / `Side` / `Power` name where a thumb
/// lands, not what pressing it does. Which button means "up" depends on how the board is being
/// held and which way the picture is rotated, so that naming belongs to an app's own mapping,
/// not to the platform.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonId {
    /// Button A — the big face button (G37).
    Front,
    /// Button B — the small button on the right-hand edge (G39).
    Side,
    /// The power button, on the AXP192 PMIC rather than a GPIO. See [`LatchedGesture`].
    Power,
}

/// A momentary push-button read as a raw pressed/released level.
///
/// The driven port for the two GPIO buttons: the firmware adapter reads the pin (active-low on
/// this board) and reports `true` while pressed. All timing and bounce rejection live inward in
/// the pure recognizer, so the adapter stays a one-line level read and every gesture rule is
/// host-tested.
pub trait ButtonLevel {
    /// The current raw level: `true` while the button is physically pressed.
    fn pressed(&mut self) -> bool;
}

/// A button whose gestures are classified *before* the firmware ever sees them.
///
/// The power button is not on a GPIO and cannot be levelled. It is wired to the AXP192's PEK
/// pin, and the PMIC does its own debouncing and its own press-duration timing against
/// thresholds set in silicon; all the firmware can do is drain a latch that says "a short press
/// happened since you last asked". So it cannot be fed through [`ButtonLevel`] and the pure
/// recognizer — synthesizing a fake level from a latch would invent timing that never existed.
///
/// It gets its own port instead, and joins the same event stream downstream.
///
/// ## Why only a click ever arrives here
///
/// A long press on this button is **not the firmware's to observe**: at four seconds the PMIC
/// cuts the rails and the ESP32 is gone. The board's own power-off is not a gesture an app can
/// handle, so [`take`](LatchedGesture::take) yields only [`Gesture::Click`], and that is
/// enforced by the return type rather than left to a comment.
pub trait LatchedGesture {
    /// Drain the latch: the gesture recorded since the last call, if any.
    ///
    /// Draining is destructive — a gesture is reported exactly once — so this must be called on
    /// a steady cadence or presses will queue up behind one another and arrive late.
    fn take(&mut self) -> Option<Gesture>;
}

/// A button did something: which button, and what.
///
/// The one currency the whole crate deals in. Both acquisition paths — the levelled GPIO
/// buttons and the latched power button — converge on this, so an app writes one match arm per
/// control and never learns which path a gesture arrived by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ButtonEvent {
    /// Which button.
    pub button: ButtonId,
    /// What it did.
    pub gesture: Gesture,
}

impl ButtonEvent {
    /// A gesture on a button.
    pub const fn new(button: ButtonId, gesture: Gesture) -> Self {
        ButtonEvent { button, gesture }
    }
}
