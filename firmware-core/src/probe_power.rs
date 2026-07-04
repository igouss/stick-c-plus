//! Probe power-gating — energize a probe only across a read.
//!
//! Some resistive probes (the M5 Earth Unit among them) have electrodes that are
//! not corrosion-resistant: a constant DC bias electrolyzes them, so a 24/7
//! monitor must power the probe only for the instant it samples. [`ProbePower`]
//! is the port a board's power mechanism implements (an AXP192 rail, a GPIO
//! high-side switch); [`gated_read`] runs a read between energize and
//! de-energize and guarantees the probe is powered down afterwards — even if the
//! read fails.

/// Switches power to a probe's electrodes.
///
/// [`energize`](Self::energize) returns only once the rail is up *and settled*,
/// so a caller reads immediately after; [`deenergize`](Self::deenergize) cuts
/// it. The error type is associated so the mechanism surfaces its own failure
/// (an `EspError`, say) without this port naming any concrete driver.
pub trait ProbePower {
    /// The power mechanism's own failure type.
    type Error;

    /// Power the probe and block until it has settled enough to read.
    fn energize(&mut self) -> Result<(), Self::Error>;

    /// Cut power to the probe.
    fn deenergize(&mut self) -> Result<(), Self::Error>;
}

/// Run `read` with the probe energized, powering it down on every exit path.
///
/// `energize` → `read` → `deenergize`, in that order. If `energize` fails, `read`
/// never runs and there is nothing to power down. If `read` fails, the probe is
/// *still* de-energized before the error propagates — a failed sample must never
/// leave the electrodes biased.
///
/// The read error takes precedence over a de-energize error: it is the
/// meaningful failure, and by the time it is returned the probe is already off.
/// A de-energize error surfaces only when the read itself succeeded, because a
/// probe left stuck-on is its own hazard worth reporting.
///
/// `read`'s error and the probe's share one type, so a caller reads through the
/// probe's error channel without converting between two error kinds.
pub fn gated_read<P, T, F>(power: &mut P, read: F) -> Result<T, P::Error>
where
    P: ProbePower,
    F: FnOnce() -> Result<T, P::Error>,
{
    power.energize()?;
    let read_result: Result<T, P::Error> = read();
    let power_off: Result<(), P::Error> = power.deenergize();
    match (read_result, power_off) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(read_err), _) => Err(read_err),
        (Ok(_), Err(off_err)) => Err(off_err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A `ProbePower` that records its calls into a shared log (so ordering
    /// against the read can be asserted) and returns scripted results.
    struct MockProbe<'a> {
        log: &'a RefCell<Vec<&'static str>>,
        energize: Result<(), &'static str>,
        deenergize: Result<(), &'static str>,
    }

    impl ProbePower for MockProbe<'_> {
        type Error = &'static str;

        fn energize(&mut self) -> Result<(), &'static str> {
            self.log.borrow_mut().push("energize");
            self.energize
        }

        fn deenergize(&mut self) -> Result<(), &'static str> {
            self.log.borrow_mut().push("deenergize");
            self.deenergize
        }
    }

    #[test]
    fn a_successful_read_is_gated_energize_then_read_then_deenergize() {
        let log: RefCell<Vec<&str>> = RefCell::new(Vec::new());
        let mut probe: MockProbe = MockProbe {
            log: &log,
            energize: Ok(()),
            deenergize: Ok(()),
        };
        let out: Result<u16, &str> = gated_read(&mut probe, || {
            log.borrow_mut().push("read");
            Ok(42)
        });
        assert_eq!(out, Ok(42));
        assert_eq!(*log.borrow(), ["energize", "read", "deenergize"]);
    }

    #[test]
    fn a_failed_read_still_deenergizes() {
        // The corrosion-safety guarantee: a read error must not leave the probe
        // biased. deenergize runs anyway, and the read error is what surfaces.
        let log: RefCell<Vec<&str>> = RefCell::new(Vec::new());
        let mut probe: MockProbe = MockProbe {
            log: &log,
            energize: Ok(()),
            deenergize: Ok(()),
        };
        let out: Result<u16, &str> = gated_read(&mut probe, || {
            log.borrow_mut().push("read");
            Err("adc timeout")
        });
        assert_eq!(out, Err("adc timeout"));
        assert_eq!(*log.borrow(), ["energize", "read", "deenergize"]);
    }

    #[test]
    fn a_failed_energize_skips_the_read() {
        // Nothing came up, so nothing runs and nothing is powered down.
        let log: RefCell<Vec<&str>> = RefCell::new(Vec::new());
        let mut probe: MockProbe = MockProbe {
            log: &log,
            energize: Err("no power"),
            deenergize: Ok(()),
        };
        let out: Result<u16, &str> = gated_read(&mut probe, || {
            log.borrow_mut().push("read");
            Ok(1)
        });
        assert_eq!(out, Err("no power"));
        assert_eq!(*log.borrow(), ["energize"]);
    }

    #[test]
    fn a_deenergize_error_surfaces_when_the_read_succeeded() {
        // A probe stuck powered is a hazard: if the read was fine, the stuck-on
        // failure is the one worth reporting.
        let log: RefCell<Vec<&str>> = RefCell::new(Vec::new());
        let mut probe: MockProbe = MockProbe {
            log: &log,
            energize: Ok(()),
            deenergize: Err("stuck on"),
        };
        let out: Result<u16, &str> = gated_read(&mut probe, || Ok(7));
        assert_eq!(out, Err("stuck on"));
    }
}
