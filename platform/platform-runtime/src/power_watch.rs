//! The power-watch thread — poll VBUS, debounce it, decide the edge, sound the chime.
//!
//! The board's other reusable background loop, alongside `spawn_display`: generic over any
//! [`PowerSource`] and [`Tone`] adapter, `std` but board-agnostic, so the whole watch cycle
//! is proven off the metal against fakes and cross-compiles unchanged. On its very first
//! sample it only takes a baseline — no chime, a device booted on USB does not greet you —
//! and from then on every settled VBUS transition sounds exactly one
//! [`PowerChime`](platform_core::PowerChime), never more, however much the raw level
//! chatters.

use std::fmt::Display;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use log::warn;
use platform_core::{edge, Clock, PowerDebounce, PowerSource, Tick, Tone};

/// How often VBUS is polled.
///
/// Gentle and modest, per the spec's constraint that the watch thread must not starve the
/// render or input threads — comfortably slower than a busy-loop, yet fast enough that a
/// plug/unplug a human just performed by hand is heard within a beat.
pub const POWER_WATCH_PERIOD: Duration = Duration::from_millis(50);

/// The power-watch thread's stack, in bytes (it becomes a FreeRTOS task stack on device).
/// Each cycle runs an I2C transaction to read VBUS and, on a read or chime failure, formats
/// an error through `warn!` — `core::fmt`'s formatting call tree is deep and stack-hungry,
/// and under FreeRTOS preemption an interrupt frame can land on top of it. 4 KiB leaves no
/// margin for that worst case; 8 KiB does, the same headroom the other hardware-touching
/// platform threads are sized to.
pub const POWER_WATCH_STACK_SIZE: usize = 8 * 1024;

/// How the watch loop is tuned: its poll cadence and its stack.
#[derive(Clone, Copy)]
pub struct PowerWatchConfig {
    /// The interval between VBUS polls.
    pub period: Duration,
    /// The watch thread's stack size, in bytes.
    pub stack_size: usize,
}

impl Default for PowerWatchConfig {
    fn default() -> Self {
        Self {
            period: POWER_WATCH_PERIOD,
            stack_size: POWER_WATCH_STACK_SIZE,
        }
    }
}

/// A running power-watch thread — a handle to stop and join it.
pub struct PowerWatchTask {
    handle: JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

impl PowerWatchTask {
    /// Ask the watch loop to finish after its current cycle.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Block until the watch thread has exited, propagating a panic it carried.
    pub fn join(self) -> thread::Result<()> {
        self.handle.join()
    }
}

/// Take the watch's very first sample: the baseline [`PowerDebounce`] and the starting
/// settled level, seeded from `source`'s own first reading so debounce state never invents a
/// spurious transition at boot — whichever side the board starts on becomes the baseline,
/// not an edge.
fn seed<P>(source: &mut P) -> Result<(PowerDebounce, bool), P::Error>
where
    P: PowerSource,
{
    let initial: bool = source.on_usb()?;
    Ok((PowerDebounce::new(initial), initial))
}

/// One steady-state watch cycle: poll `source`, fold it through `debounce`, and — only on a
/// settled transition away from `prev` — play the [`PowerChime`](platform_core::PowerChime)
/// [`edge`](platform_core::edge) decides. Returns the settled level to carry into the next
/// cycle (unchanged if nothing settled this poll).
///
/// A `source` or `tone` failure is logged and never fatal, matching `spawn_display`'s
/// fail-visible render errors: a flaky PMIC read or buzzer must not take the watch thread
/// down, and the next poll retries.
fn watch_once<P, T>(
    source: &mut P,
    tone: &mut T,
    debounce: &mut PowerDebounce,
    prev: bool,
    now: Tick,
) -> bool
where
    P: PowerSource,
    P::Error: Display,
    T: Tone,
    T::Error: Display,
{
    // Poll VBUS. A flaky read is logged and skipped, never fatal — the level carried forward
    // is unchanged, and the next poll retries (fail-visible, like `spawn_display`'s renders).
    let raw_on_usb: bool = match source.on_usb() {
        Ok(raw_on_usb) => raw_on_usb,
        Err(err) => {
            warn!("platform-power-watch: VBUS read failed, skipping this cycle: {err}");
            return prev;
        }
    };

    // Fold it through the debounce; a settled transition — and only a settled one — sounds the
    // chime `edge` decides. A failed play is logged too, and the settled level still carries.
    match debounce.update(now, raw_on_usb) {
        Some(settled) => {
            if let Some(chime) = edge(prev, settled) {
                if let Err(err) = tone.play(chime.notes()) {
                    warn!("platform-power-watch: chime failed: {err}");
                }
            }
            settled
        }
        None => prev,
    }
}

/// Spawn the power-watch thread: seed the baseline from `source`, then every `config.period`
/// poll it, debounce it, and sound the [`PowerChime`](platform_core::PowerChime) a settled
/// transition decides on `tone`.
///
/// `source` and `tone` move into the thread, so both must be [`Send`] + `'static`; both
/// adapters' errors must be [`Display`] so a failure can be logged. Returns the
/// [`PowerWatchTask`] handle, or the [`io::Error`] from failing to spawn the OS/RTOS thread.
pub fn spawn_power_watch<P, T, C>(
    mut source: P,
    tone: T,
    clock: C,
    config: PowerWatchConfig,
) -> io::Result<PowerWatchTask>
where
    P: PowerSource + Send + 'static,
    P::Error: Display,
    T: Tone + Send + 'static,
    T::Error: Display,
    C: Clock + Send + 'static,
{
    // Take the baseline synchronously, before the thread exists, so the first VBUS sample is
    // the one true at spawn — a boot chime never rides on which thread the scheduler runs
    // first. A failed baseline read is a genuine start-up failure: report it, do not guess.
    let (debounce, baseline): (PowerDebounce, bool) =
        seed(&mut source).map_err(|err: P::Error| io::Error::other(err.to_string()))?;

    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let stop_in_thread: Arc<AtomicBool> = Arc::clone(&stop);
    let period: Duration = config.period;
    let handle: JoinHandle<()> = thread::Builder::new()
        .name("platform-power-watch".to_string())
        .stack_size(config.stack_size)
        .spawn(move || {
            watch_loop(
                source,
                tone,
                clock,
                period,
                debounce,
                baseline,
                stop_in_thread,
            )
        })?;
    Ok(PowerWatchTask { handle, stop })
}

/// The thread body: seed already taken, poll → debounce → chime every `period` until stopped.
///
/// `prev` is the last settled level, threaded through each [`watch_once`]; a gentle
/// `period` sleep keeps the watch off the CPU between polls so it never starves render or
/// input, exactly as the spec's cadence constraint requires.
fn watch_loop<P, T, C>(
    mut source: P,
    mut tone: T,
    clock: C,
    period: Duration,
    mut debounce: PowerDebounce,
    mut prev: bool,
    stop: Arc<AtomicBool>,
) where
    P: PowerSource,
    P::Error: Display,
    T: Tone,
    T::Error: Display,
    C: Clock,
{
    while !stop.load(Ordering::Relaxed) {
        prev = watch_once(&mut source, &mut tone, &mut debounce, prev, clock.now());
        thread::sleep(period);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::Mutex;

    use platform_core::{Note, PowerChime, POWER_DEBOUNCE_MS};

    use crate::clock::Monotonic;

    /// A [`PowerSource`] a single-threaded test drives by hand through a shared cell.
    struct FakePowerSource<'a> {
        on_usb: &'a Cell<bool>,
    }

    impl PowerSource for FakePowerSource<'_> {
        type Error = std::convert::Infallible;
        fn on_usb(&mut self) -> Result<bool, Self::Error> {
            Ok(self.on_usb.get())
        }
    }

    /// A [`PowerSource`] a spawned-thread test flips by hand, from outside the thread.
    struct ThreadPowerSource {
        on_usb: Arc<Mutex<bool>>,
    }

    impl PowerSource for ThreadPowerSource {
        type Error = std::convert::Infallible;
        fn on_usb(&mut self) -> Result<bool, Self::Error> {
            Ok(*self.on_usb.lock().expect("on_usb lock"))
        }
    }

    /// A buzzer that records every chime's notes, so a test can assert what sounded — and,
    /// through `ping`, signal a spawned thread's play so a test can await it instead of
    /// sleep-polling.
    struct RecordingTone {
        log: Arc<Mutex<Vec<Note>>>,
        ping: Option<Sender<()>>,
    }

    impl RecordingTone {
        fn new() -> (Self, Arc<Mutex<Vec<Note>>>) {
            let log: Arc<Mutex<Vec<Note>>> = Arc::new(Mutex::new(Vec::new()));
            (
                RecordingTone {
                    log: Arc::clone(&log),
                    ping: None,
                },
                log,
            )
        }

        fn pinging(mut self, tx: Sender<()>) -> Self {
            self.ping = Some(tx);
            self
        }
    }

    impl Tone for RecordingTone {
        type Error = std::convert::Infallible;
        fn play(&mut self, notes: &[Note]) -> Result<(), Self::Error> {
            self.log
                .lock()
                .expect("chime log lock")
                .extend_from_slice(notes);
            if let Some(tx) = &self.ping {
                let _ = tx.send(());
            }
            Ok(())
        }
    }

    /// The notes a `RecordingTone`'s log holds so far.
    fn played(log: &Arc<Mutex<Vec<Note>>>) -> Vec<Note> {
        log.lock().expect("chime log lock").clone()
    }

    /// Drive `steps` (a `(now, raw_on_usb)` schedule) through [`watch_once`], starting from
    /// `prev`, and return the settled level after the last step — the same value a real
    /// watch thread would carry into its next cycle.
    fn feed_watch(
        on_usb: &Cell<bool>,
        source: &mut FakePowerSource,
        tone: &mut RecordingTone,
        debounce: &mut PowerDebounce,
        mut prev: bool,
        steps: &[(Tick, bool)],
    ) -> bool {
        steps.iter().for_each(|&(now, raw): &(Tick, bool)| {
            on_usb.set(raw);
            prev = watch_once(source, tone, debounce, prev, now);
        });
        prev
    }

    /// AC1 — Plugged in -> spool-up plays exactly once, and no other melody.
    #[test]
    fn plugging_in_plays_spool_up_once() {
        let on_usb: Cell<bool> = Cell::new(false);
        let mut source: FakePowerSource = FakePowerSource { on_usb: &on_usb };
        let (mut tone, log): (RecordingTone, _) = RecordingTone::new();
        let mut debounce: PowerDebounce = PowerDebounce::new(false);

        let next: bool = feed_watch(
            &on_usb,
            &mut source,
            &mut tone,
            &mut debounce,
            false,
            &[(0, true), (POWER_DEBOUNCE_MS, true)],
        );

        assert_eq!(played(&log), PowerChime::SpoolUp.notes());
        assert!(next, "the level carried forward must be on-USB");
    }

    /// AC2 — Unplugged -> spool-down plays exactly once.
    #[test]
    fn unplugging_plays_spool_down_once() {
        let on_usb: Cell<bool> = Cell::new(true);
        let mut source: FakePowerSource = FakePowerSource { on_usb: &on_usb };
        let (mut tone, log): (RecordingTone, _) = RecordingTone::new();
        let mut debounce: PowerDebounce = PowerDebounce::new(true);

        let next: bool = feed_watch(
            &on_usb,
            &mut source,
            &mut tone,
            &mut debounce,
            true,
            &[(0, false), (POWER_DEBOUNCE_MS, false)],
        );

        assert_eq!(played(&log), PowerChime::SpoolDown.notes());
        assert!(!next, "the level carried forward must be on-battery");
    }

    /// AC3 (zero case) — boot with USB already present takes a silent baseline: the first
    /// sample seeds `prev`, and the immediately following steady cycle plays nothing.
    #[test]
    fn boot_on_usb_takes_a_silent_baseline() {
        let on_usb: Cell<bool> = Cell::new(true);
        let mut source: FakePowerSource = FakePowerSource { on_usb: &on_usb };
        let (mut tone, log): (RecordingTone, _) = RecordingTone::new();

        let (mut debounce, prev): (PowerDebounce, bool) =
            seed(&mut source).expect("seed reads once");
        let next: bool = watch_once(
            &mut source,
            &mut tone,
            &mut debounce,
            prev,
            POWER_DEBOUNCE_MS + 1,
        );

        assert_eq!(
            played(&log),
            Vec::<Note>::new(),
            "a boot on USB must not chime"
        );
        assert!(prev, "the baseline must reflect the true first reading");
        assert!(next, "an unchanged level must carry forward unchanged");
    }

    /// AC4 (many -> one) — a plug/unplug bounce inside the debounce window collapses to
    /// exactly one spool-up.
    #[test]
    fn a_bounce_inside_the_window_still_plays_spool_up_once() {
        let on_usb: Cell<bool> = Cell::new(false);
        let mut source: FakePowerSource = FakePowerSource { on_usb: &on_usb };
        let (mut tone, log): (RecordingTone, _) = RecordingTone::new();
        let mut debounce: PowerDebounce = PowerDebounce::new(false);

        feed_watch(
            &on_usb,
            &mut source,
            &mut tone,
            &mut debounce,
            false,
            &[
                (0, true),
                (2, false),
                (4, true),                     // chatters, all inside the window
                (4 + POWER_DEBOUNCE_MS, true), // holds steady from here -> settles once
            ],
        );

        assert_eq!(played(&log), PowerChime::SpoolUp.notes());
    }

    /// P2 — idempotent across repeated equal samples: many steady polls, well past the
    /// baseline, decide no chime on any of them.
    #[test]
    fn many_repeated_equal_samples_after_the_baseline_play_nothing() {
        let on_usb: Cell<bool> = Cell::new(false);
        let mut source: FakePowerSource = FakePowerSource { on_usb: &on_usb };
        let (mut tone, log): (RecordingTone, _) = RecordingTone::new();
        let mut debounce: PowerDebounce = PowerDebounce::new(false);

        feed_watch(
            &on_usb,
            &mut source,
            &mut tone,
            &mut debounce,
            false,
            &[(0, false), (1_000, false), (2_000, false), (3_000, false)],
        );

        assert_eq!(played(&log), Vec::<Note>::new());
    }

    /// R8 — the one integration test: spawn the real thread against fakes, hear a plug-in,
    /// then stop and join cleanly. The plumbing, proven end to end, not just the pure cycle.
    #[test]
    fn the_spawned_thread_chimes_on_a_real_transition_and_stops_cleanly() {
        let clock: Monotonic = Monotonic::start();
        let on_usb: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let source: ThreadPowerSource = ThreadPowerSource {
            on_usb: Arc::clone(&on_usb),
        };
        let (tx, rx): (Sender<()>, _) = channel();
        let (base, log): (RecordingTone, _) = RecordingTone::new();
        let tone: RecordingTone = base.pinging(tx);
        let config: PowerWatchConfig = PowerWatchConfig {
            period: Duration::from_millis(1),
            stack_size: 64 * 1024,
        };

        let task: PowerWatchTask =
            spawn_power_watch(source, tone, clock, config).expect("spawn power-watch thread");

        *on_usb.lock().expect("on_usb lock") = true;
        rx.recv_timeout(Duration::from_secs(2))
            .expect("a real VBUS transition must chime");

        task.stop();
        task.join().expect("the power-watch thread must not panic");

        assert_eq!(played(&log), PowerChime::SpoolUp.notes());
    }
}
