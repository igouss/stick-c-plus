//! The backlight's one owner, and the flag the display loop reads.
//!
//! Two threads care about whether the glass is lit, and they care about it differently. The
//! input thread *decides* — a click on the power button toggles it — and it owns the
//! [`Backlight`] port because it is the only writer. The display thread only needs to *know*,
//! so that it can skip a paint nobody can see.
//!
//! Rather than share the port behind a mutex (which would put an I2C transaction on the render
//! path, in the lock, on every tick), the switch keeps a plain atomic mirror of the state and
//! hands out [`LitFlag`] clones. Reading it is one relaxed load; the I2C write happens once,
//! on the thread that asked for the change.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use platform_core::Backlight;

/// The backlight's one owner: the port, plus the shared mirror of its state.
///
/// Whoever holds this is the only writer. [`toggle`](Self::toggle) and [`set`](Self::set) drive
/// the real rail and update the mirror together, so a [`LitFlag`] never disagrees with the
/// glass for longer than one I2C transaction.
pub struct BacklightSwitch<B> {
    port: B,
    lit: Arc<AtomicBool>,
}

impl<B: Backlight> BacklightSwitch<B> {
    /// Take ownership of `port`, declaring the state it is already in.
    ///
    /// `lit` is the truth at construction, not a command: the composition root has just brought
    /// the rails up, so the glass is already lit and this must say so rather than issue a
    /// redundant write.
    pub fn new(port: B, lit: bool) -> Self {
        BacklightSwitch {
            port,
            lit: Arc::new(AtomicBool::new(lit)),
        }
    }

    /// A cheap, shareable reader of the current state — clone one per interested thread.
    pub fn flag(&self) -> LitFlag {
        LitFlag {
            lit: Arc::clone(&self.lit),
        }
    }

    /// Light the glass, or darken it.
    ///
    /// The mirror is updated only once the rail actually changed, so a failed I2C write leaves
    /// the flag telling the truth: the glass is still lit, and the display thread must keep
    /// painting it.
    pub fn set(&mut self, lit: bool) -> Result<(), B::Error> {
        self.port.set(lit)?;
        self.lit.store(lit, Ordering::Relaxed);
        Ok(())
    }

    /// Flip the glass between lit and dark, returning the state it ended in.
    pub fn toggle(&mut self) -> Result<bool, B::Error> {
        let next: bool = !self.lit.load(Ordering::Relaxed);
        self.set(next)?;
        Ok(next)
    }
}

/// A cheap, `Clone` + `Send` reader of whether the glass is lit.
///
/// Held by the display loop, which asks once per tick and skips the whole render when the
/// answer is no.
#[derive(Clone)]
pub struct LitFlag {
    lit: Arc<AtomicBool>,
}

impl LitFlag {
    /// A flag permanently reporting `lit` — for an app with no backlight control, which pays
    /// nothing beyond one atomic load per tick.
    pub fn always(lit: bool) -> Self {
        LitFlag {
            lit: Arc::new(AtomicBool::new(lit)),
        }
    }

    /// Whether the glass is currently lit.
    pub fn is_lit(&self) -> bool {
        self.lit.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A backlight that records every state it was set to, and can be made to fail.
    struct FakeBacklight {
        log: Rc<RefCell<Vec<bool>>>,
        fail: bool,
    }

    impl Backlight for FakeBacklight {
        type Error = &'static str;

        fn set(&mut self, lit: bool) -> Result<(), &'static str> {
            self.log.borrow_mut().push(lit);
            match self.fail {
                true => Err("i2c offline"),
                false => Ok(()),
            }
        }
    }

    /// A switch over a recording backlight, plus the log it writes to.
    fn switch(fail: bool) -> (BacklightSwitch<FakeBacklight>, Rc<RefCell<Vec<bool>>>) {
        let log: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
        let port: FakeBacklight = FakeBacklight {
            log: Rc::clone(&log),
            fail,
        };
        (BacklightSwitch::new(port, true), log)
    }

    /// Zero: constructing the switch declares the current state without touching the rail.
    #[test]
    fn construction_writes_nothing() {
        let (sw, log): (BacklightSwitch<FakeBacklight>, _) = switch(false);

        assert!(sw.flag().is_lit());
        assert_eq!(log.borrow().clone(), Vec::<bool>::new());
    }

    /// One: a toggle darkens the glass, and the flag every other thread reads follows it.
    #[test]
    fn a_toggle_darkens_the_glass_and_the_flag_follows() {
        let (mut sw, log): (BacklightSwitch<FakeBacklight>, _) = switch(false);
        let flag: LitFlag = sw.flag();

        let now_lit: bool = sw.toggle().expect("toggle must reach the rail");

        assert!(!now_lit);
        assert!(!flag.is_lit());
        assert_eq!(log.borrow().clone(), vec![false]);
    }

    /// Many: toggling twice returns to lit, and every step is written through.
    #[test]
    fn two_toggles_return_to_lit() {
        let (mut sw, log): (BacklightSwitch<FakeBacklight>, _) = switch(false);
        let flag: LitFlag = sw.flag();

        sw.toggle().expect("first toggle");
        sw.toggle().expect("second toggle");

        assert!(flag.is_lit());
        assert_eq!(log.borrow().clone(), vec![false, true]);
    }

    /// A failed write leaves the flag telling the truth: the rail never changed, so the glass is
    /// still lit and the display thread must keep painting it. Believing the request instead
    /// would blank a lit screen.
    #[test]
    fn a_failed_write_leaves_the_flag_honest() {
        let (mut sw, _log): (BacklightSwitch<FakeBacklight>, _) = switch(true);
        let flag: LitFlag = sw.flag();

        let outcome: Result<bool, &'static str> = sw.toggle();

        assert!(outcome.is_err());
        assert!(
            flag.is_lit(),
            "a failed toggle must not claim the glass went dark"
        );
    }

    /// An app with no backlight control gets a flag that always says lit.
    #[test]
    fn an_always_flag_never_changes() {
        assert!(LitFlag::always(true).is_lit());
        assert!(!LitFlag::always(false).is_lit());
    }
}
