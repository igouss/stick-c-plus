//! Responsibility one: given the evidence, is anything broken, and what?

use core::cmp::Ordering;

use platform_core::Tick;

use crate::{boot::BootVerdict, fault::Fault, observed::Observed, resources::Resources};

/// The board's health, as of one `assess` call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Nothing is wrong.
    Healthy,
    /// The single highest-ranked fault present. The operator gets one banner and one sound,
    /// so a fold that found several problems still returns exactly one.
    Faulted(Fault),
}

/// One pure fold from everything known about the board at `now` to a [`Verdict`].
///
/// Total and effect-free: no clock, no I/O, no global state. `now` is a parameter because the
/// domain never reads a clock of its own — the caller injects the instant this judgement is
/// made for, which is what makes "same inputs, same verdict" a provable property rather than a
/// hope.
///
/// `resources: None` means the board has not read its heap yet, which is not itself a fault —
/// absence of evidence is not evidence of starvation. `heap_floor_bytes` is a parameter rather
/// than a constant because it has to be calibrated against real numbers read off a healthy
/// board; baking a guess into the domain would be worse than asking for it.
///
/// Ranking, ties, and the two board-scoped faults (`Crashed`, `Starved`) are all decided here;
/// see the crate's design notes for the total order and why ties break by (duration, then
/// position in `observed`).
pub fn assess(
    observed: &[Observed],
    boot: BootVerdict,
    resources: Option<Resources>,
    heap_floor_bytes: u32,
    now: Tick,
) -> Verdict {
    let crashed: Option<Fault> = match boot {
        BootVerdict::Clean => None,
        BootVerdict::Crashed(cause) => Some(Fault::Crashed {
            cause,
            since_ms: now,
        }),
    };
    let starved: Option<Fault> = resources
        .filter(|reading: &Resources| reading.free_heap_bytes < heap_floor_bytes)
        .map(|reading: Resources| Fault::Starved {
            free_heap_bytes: reading.free_heap_bytes,
            floor_bytes: heap_floor_bytes,
        });
    let worst: Option<Fault> = crashed
        .into_iter()
        .chain(starved)
        .chain(
            observed
                .iter()
                .filter_map(|entry: &Observed| component_fault(entry, now)),
        )
        .fold(None, |best: Option<Fault>, candidate: Fault| match best {
            Some(incumbent) if !outranks(&candidate, &incumbent) => Some(incumbent),
            _ => Some(candidate),
        });
    match worst {
        None => Verdict::Healthy,
        Some(fault) => Verdict::Faulted(fault),
    }
}

/// What one registered component contributes to the fold, if anything.
///
/// A component that concluded its own fault is [`Fault::Reported`] and is not also examined for
/// silence: it already said what is wrong, and inferring a stall on top would name the same
/// trouble twice. Otherwise silence past its own deadline is [`Fault::Stalled`], and a
/// component that has never once beaten is stalled from `now` — the "spawned but never
/// registered" false-green, inverted into a loud one.
fn component_fault(entry: &Observed, now: Tick) -> Option<Fault> {
    match (entry.gave_up_at, entry.last_beat) {
        (Some(gave_up_at), _) => Some(Fault::Reported {
            component: entry.component,
            for_ms: now.saturating_sub(gave_up_at),
        }),
        (None, None) => Some(Fault::Stalled {
            component: entry.component,
            silent_for_ms: now,
        }),
        (None, Some(last_beat)) => {
            // Saturating, so a clock that briefly runs backwards reads as age 0 and stays
            // healthy rather than inventing a stall — the same defence both `freshness.rs`
            // files already make.
            let silent_for_ms: Tick = now.saturating_sub(last_beat);
            match silent_for_ms > Tick::from(entry.deadline_ms) {
                true => Some(Fault::Stalled {
                    component: entry.component,
                    silent_for_ms,
                }),
                false => None,
            }
        }
    }
}

/// Whether `candidate` should displace `incumbent` as the one fault reported.
///
/// Rank first; inside one rank, whichever has stood longest, on the reasoning that the thing
/// that broke earliest is likeliest the cause. A strict `>` on the duration is what makes the
/// final tie-break positional: an equal-duration latecomer never displaces the entry that came
/// earlier in `observed`, so the verdict cannot depend on iteration order.
fn outranks(candidate: &Fault, incumbent: &Fault) -> bool {
    match candidate.severity().cmp(&incumbent.severity()) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => standing_ms(candidate) > standing_ms(incumbent),
    }
}

/// How long a fault has stood, for breaking ties inside one rank. [`Fault::Starved`] carries a
/// level rather than a duration — a starving board has been starving since some unobserved
/// moment — and it is the only fault of its kind in any verdict, so it never reaches a tie.
fn standing_ms(fault: &Fault) -> Tick {
    match fault {
        Fault::Crashed { since_ms, .. } => *since_ms,
        Fault::Starved { .. } => 0,
        Fault::Reported { for_ms, .. } => *for_ms,
        Fault::Stalled { silent_for_ms, .. } => *silent_for_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{boot::CrashCause, fault::Fault, Component};
    use proptest::prelude::*;

    fn on_time(component: &'static str, deadline_ms: u32, last_beat: Tick) -> Observed {
        Observed {
            component: Component(component),
            deadline_ms,
            last_beat: Some(last_beat),
            gave_up_at: None,
        }
    }

    // --- R23 — zero / one / many components, everything else healthy. ---

    /// Zero — no registered components, a clean boot, no heap reading yet: healthy.
    #[test]
    fn no_components_and_a_clean_boot_is_healthy() {
        let verdict: Verdict = assess(&[], BootVerdict::Clean, None, 0, 1_000);
        assert_eq!(verdict, Verdict::Healthy);
    }

    /// One — a single component beating well inside its deadline: healthy.
    #[test]
    fn one_component_on_time_is_healthy() {
        let observed: [Observed; 1] = [on_time("display", 2_000, 900)];
        let verdict: Verdict = assess(&observed, BootVerdict::Clean, None, 0, 1_000);
        assert_eq!(verdict, Verdict::Healthy);
    }

    /// Many (three) — every component on time: healthy.
    #[test]
    fn three_components_all_on_time_is_healthy() {
        let observed: [Observed; 3] = [
            on_time("display", 2_000, 900),
            on_time("sensors", 5_000, 500),
            on_time("network", 10_000, 1_000),
        ];
        let verdict: Verdict = assess(&observed, BootVerdict::Clean, None, 0, 1_000);
        assert_eq!(verdict, Verdict::Healthy);
    }

    // --- R6/R7/R8 — the fault vocabulary, one row each. ---

    /// A component silent past its deadline is named and dated.
    ///
    /// The deadline decides *whether* it is stalled; the duration reported is the
    /// total age of the last beat, not the excess past the deadline.
    #[test]
    fn a_component_silent_past_its_deadline_is_stalled() {
        let observed: [Observed; 1] = [on_time("display", 2_000, 0)];
        let verdict: Verdict = assess(&observed, BootVerdict::Clean, None, 0, 5_000);
        assert_eq!(
            verdict,
            Verdict::Faulted(Fault::Stalled {
                component: Component("display"),
                silent_for_ms: 5_000,
            })
        );
    }

    /// A registered component that has never once beaten is stalled with its silence dated
    /// from `now` itself — the "spawned but never registered" false-green, inverted loud.
    #[test]
    fn a_component_that_never_beat_is_stalled_since_now() {
        let observed: [Observed; 1] = [Observed {
            component: Component("display"),
            deadline_ms: 2_000,
            last_beat: None,
            gave_up_at: None,
        }];
        let verdict: Verdict = assess(&observed, BootVerdict::Clean, None, 0, 5_000);
        assert_eq!(
            verdict,
            Verdict::Faulted(Fault::Stalled {
                component: Component("display"),
                silent_for_ms: 5_000,
            })
        );
    }

    /// A component that concluded its own fault is reported, dated from when it gave up.
    #[test]
    fn a_component_that_gave_up_is_reported() {
        let observed: [Observed; 1] = [Observed {
            component: Component("sensors"),
            deadline_ms: 2_000,
            last_beat: Some(4_000),
            gave_up_at: Some(1_000),
        }];
        let verdict: Verdict = assess(&observed, BootVerdict::Clean, None, 0, 5_000);
        assert_eq!(
            verdict,
            Verdict::Faulted(Fault::Reported {
                component: Component("sensors"),
                for_ms: 4_000,
            })
        );
    }

    /// A crashed boot is reported, naming the cause, before any component is even considered.
    #[test]
    fn a_crashed_boot_is_the_verdict() {
        let verdict: Verdict = assess(
            &[],
            BootVerdict::Crashed(CrashCause::TaskWatchdog),
            None,
            0,
            7_000,
        );
        assert_eq!(
            verdict,
            Verdict::Faulted(Fault::Crashed {
                cause: CrashCause::TaskWatchdog,
                since_ms: 7_000,
            })
        );
    }

    /// A heap reading under the configured floor is starvation, naming the level, not a
    /// fabricated duration.
    #[test]
    fn free_heap_under_the_floor_is_starved() {
        let verdict: Verdict = assess(
            &[],
            BootVerdict::Clean,
            Some(Resources {
                free_heap_bytes: 4_000,
            }),
            12_000,
            1_000,
        );
        assert_eq!(
            verdict,
            Verdict::Faulted(Fault::Starved {
                free_heap_bytes: 4_000,
                floor_bytes: 12_000,
            })
        );
    }

    /// A heap reading at exactly the floor is not yet starved — the boundary belongs to the
    /// healthy side, not the faulted one.
    #[test]
    fn free_heap_exactly_at_the_floor_is_healthy() {
        let verdict: Verdict = assess(
            &[],
            BootVerdict::Clean,
            Some(Resources {
                free_heap_bytes: 12_000,
            }),
            12_000,
            1_000,
        );
        assert_eq!(verdict, Verdict::Healthy);
    }

    /// R10 — absence of a heap reading is not itself a fault: a board that has not read its
    /// heap yet is not a starving board.
    #[test]
    fn no_heap_reading_yet_is_never_starved() {
        let verdict: Verdict = assess(&[], BootVerdict::Clean, None, 12_000, 1_000);
        assert_eq!(verdict, Verdict::Healthy);
    }

    // --- R12 — ranking picks exactly one winner when several things are broken. ---

    /// A crash outranks a stall it probably caused: with both present, the verdict names the
    /// crash.
    #[test]
    fn a_crash_outranks_a_simultaneous_stall() {
        let observed: [Observed; 1] = [on_time("display", 2_000, 0)];
        let verdict: Verdict = assess(
            &observed,
            BootVerdict::Crashed(CrashCause::Panic),
            None,
            0,
            5_000,
        );
        assert_eq!(
            verdict,
            Verdict::Faulted(Fault::Crashed {
                cause: CrashCause::Panic,
                since_ms: 5_000,
            })
        );
    }

    /// Between two simultaneous stalls, the one that has stood longest is reported first.
    #[test]
    fn the_longer_standing_stall_wins_the_tie() {
        let observed: [Observed; 2] = [
            on_time("sensors", 2_000, 4_000),
            on_time("display", 2_000, 0),
        ];
        let verdict: Verdict = assess(&observed, BootVerdict::Clean, None, 0, 5_000);
        assert_eq!(
            verdict,
            Verdict::Faulted(Fault::Stalled {
                component: Component("display"),
                silent_for_ms: 5_000,
            })
        );
    }

    /// Two stalls of exactly equal duration break their tie by position in `observed`, not by
    /// iteration order — otherwise "same inputs, same verdict" would be flaky rather than
    /// false, the worst kind of red.
    #[test]
    fn equal_duration_stalls_break_the_tie_by_position() {
        let observed: [Observed; 2] = [on_time("first", 2_000, 0), on_time("second", 2_000, 0)];
        let verdict: Verdict = assess(&observed, BootVerdict::Clean, None, 0, 5_000);
        assert_eq!(
            verdict,
            Verdict::Faulted(Fault::Stalled {
                component: Component("first"),
                silent_for_ms: 5_000,
            })
        );
    }

    // --- Property tests: the general laws. ---

    proptest! {
        /// R10 — determinism: the same evidence, assessed twice, is the same verdict.
        #[test]
        fn same_inputs_same_verdict(deadline in 100u32..10_000, gap in 0u64..100, now in 0u64..1_000_000) {
            let observed: [Observed; 1] = [on_time("display", deadline, now.saturating_sub(gap))];
            let first: Verdict = assess(&observed, BootVerdict::Clean, None, 0, now);
            let second: Verdict = assess(&observed, BootVerdict::Clean, None, 0, now);
            prop_assert_eq!(first, second);
        }

        /// R26 — a component beating strictly inside its deadline is never stalled, for any
        /// deadline and any gap within it.
        #[test]
        fn beating_inside_the_deadline_is_never_stalled(deadline in 1u32..10_000, gap in 0u64..10_000, now in 0u64..1_000_000) {
            let gap: u64 = gap % u64::from(deadline);
            let observed: [Observed; 1] = [on_time("display", deadline, now.saturating_sub(gap))];
            let verdict: Verdict = assess(&observed, BootVerdict::Clean, None, 0, now);
            prop_assert_eq!(verdict, Verdict::Healthy);
        }

        /// A backwards clock (the `last_beat` reads later than `now`) must never read as a
        /// stall — the saturating-age guard against a clock that briefly runs backwards.
        #[test]
        fn a_backwards_clock_never_reads_as_stalled(now in 0u64..1_000, future in 1_000u64..2_000) {
            let observed: [Observed; 1] = [on_time("display", 1, future)];
            let verdict: Verdict = assess(&observed, BootVerdict::Clean, None, 0, now);
            prop_assert_eq!(verdict, Verdict::Healthy);
        }
    }
}
