//! The backlight, decorated so that other threads can see it.
//!
//! Two threads care about whether the glass is lit, and they care about it differently. One
//! *decides* — the app's input loop, when the power button is clicked — and it owns the
//! [`Backlight`] port because it is the only writer. The display thread only needs to *know*, so
//! that it can skip a paint nobody can see.
//!
//! [`BacklightSwitch`] is a decorator: it *is* a [`Backlight`], wrapping the real adapter and
//! mirroring every change into an atomic that [`LitFlag`] clones read. So the app's shell keeps
//! depending on nothing but the port — it never learns that anybody is watching — while the
//! display loop gets a cheap, `Send` reader. Sharing the port itself behind a mutex would
//! instead put an I2C transaction inside a lock on the render path, once per tick.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use platform_core::Backlight;

/// A [`Backlight`] that publishes its state to every interested thread.
///
/// Wraps the real adapter and is itself the port, so a composition root injects one wherever a
/// plain backlight was expected and nothing downstream changes.
pub struct BacklightSwitch<B> {
    port: B,
    lit: Arc<AtomicBool>,
}

impl<B: Backlight> BacklightSwitch<B> {
    /// Wrap `port`, publishing the state it is already in.
    pub fn new(port: B) -> Self {
        let lit: bool = port.is_lit();
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
}

impl<B: Backlight> Backlight for BacklightSwitch<B> {
    type Error = B::Error;

    fn is_lit(&self) -> bool {
        self.port.is_lit()
    }

    /// Set the rail, then publish.
    ///
    /// In that order, and only on success: a failed write leaves the flag telling the truth —
    /// the rail never changed, so the glass is still lit and the display thread must keep
    /// painting it. Publishing the *request* instead would blank a live screen.
    fn set(&mut self, lit: bool) -> Result<(), B::Error> {
        self.port.set(lit)?;
        self.lit.store(lit, Ordering::Relaxed);
        Ok(())
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
        lit: bool,
        log: Rc<RefCell<Vec<bool>>>,
        fail: bool,
    }

    impl Backlight for FakeBacklight {
        type Error = &'static str;

        fn is_lit(&self) -> bool {
            self.lit
        }

        fn set(&mut self, lit: bool) -> Result<(), &'static str> {
            self.log.borrow_mut().push(lit);
            match self.fail {
                true => Err("i2c offline"),
                false => {
                    self.lit = lit;
                    Ok(())
                }
            }
        }
    }

    /// A switch over a recording backlight that starts lit, plus the log it writes to.
    fn switch(fail: bool) -> (BacklightSwitch<FakeBacklight>, Rc<RefCell<Vec<bool>>>) {
        let log: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
        let port: FakeBacklight = FakeBacklight {
            lit: true,
            log: Rc::clone(&log),
            fail,
        };
        (BacklightSwitch::new(port), log)
    }

    /// Zero: wrapping the port publishes what it already reports, without touching the rail.
    #[test]
    fn wrapping_writes_nothing_and_publishes_the_current_state() {
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
