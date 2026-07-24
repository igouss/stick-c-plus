//! The display thread — pull the freshest app state, render it when the picture changed.
//!
//! The board's one reusable render loop, generic over any [`Animated`] app state `S` and any
//! [`Screen`] adapter that paints it. A composition root hands it a `source` closure (which
//! computes the current `S` from whatever the app owns — a moisture cache, a timer) and a
//! `Screen`; the loop owns the *render cadence* and the *change suppression*, and nothing
//! else. The pixel-pushing stays in the adapter, the state policy in the app, so this loop's
//! body is a straight line.
//!
//! ## The picture, not the value
//!
//! The glass shows a creature beside the app's value, and for the states that must draw
//! attention that creature *animates* ([`Animated::is_animated`]). So the loop suppresses on
//! the pair `(state, frame)` rather than on the state alone, and takes its cadence from the
//! same policy:
//!
//! - a **still** state → a constant pair → painted once, then left alone at [`RENDER_PERIOD`];
//!   nothing keeps the CPU awake between changes.
//! - an **animated** state → the frame advances on the creature's own clock, so the panel
//!   repaints at [`ANIMATION_PERIOD`] — and only on the ticks where a new frame is due.
//!
//! ## The animation clock, and the anchor
//!
//! The clock for a state's creature is `now - since`, where `since` is when the state's
//! [`anchor`](Animated::anchor) last changed — *not* when the state last changed. A pomodoro
//! screen's `mm:ss` value changes every second, but its creature must keep animating on the
//! phase's clock rather than restarting each second; the anchor (the phase, not the value) is
//! what the loop resets on. For a plant `Observation`, the anchor is the whole observation, so
//! this reproduces the original behaviour exactly.
//!
//! ## A dark glass
//!
//! A screen nobody can see is not worth painting, so a loop whose [`LitFlag`] says dark skips
//! the whole cycle: no state read, no pixels, no SPI. It keeps a slow heartbeat
//! ([`DARK_PERIOD`]) so the light coming back is noticed promptly.
//!
//! Going dark also *drops the remembered picture*, which is the half that is easy to miss. The
//! backlight and the panel are separate rails here, so cutting the light leaves the ST7789
//! holding its framebuffer — the old picture is still physically on the glass. Without the
//! reset, suppression would compare against it and paint nothing, and the stale picture would
//! reappear when the light did, standing until the app's state next happened to change.
//!
//! ## Fail-visible
//!
//! A render error is logged and skipped, never fatal: a flaky panel must not take an app
//! down, and the next cycle repaints regardless. Nothing here is ESP-specific, so the whole
//! loop is exercised on the host against a fake screen; a composition root wires the real
//! ST7789 adapter in.

use std::fmt::Display;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use log::{info, warn};
use platform_core::{Animated, Clock, Screen, ScreenRotation, Tick};

use crate::backlight::LitFlag;

/// How often the panel is repainted while a state is **still**.
///
/// A shorter tick than the app's own value cadence lets a transition show promptly, and
/// repainting a couple of lines is cheap. A still state suppresses anyway, so this is only
/// the interval at which the loop *checks* whether the picture changed.
pub const RENDER_PERIOD: Duration = Duration::from_secs(1);

/// How often the panel is repainted while the creature is **animating**.
///
/// The shortest sprite frame hold is 60 ms, so waking every 50 ms lands on every frame. This
/// cadence applies only while a state animates; a still state falls back to [`RENDER_PERIOD`],
/// so a resting device is not kept awake. Waking is not repainting: a tick that finds the
/// same frame due touches no pixels and no SPI bus.
pub const ANIMATION_PERIOD: Duration = Duration::from_millis(50);

/// The shortest sleep the loop will ever take — **one FreeRTOS tick**.
///
/// A paint that outran its tick budget would otherwise sleep for zero and the thread would
/// never yield — starving lower-priority FreeRTOS tasks on a single core. That happened for
/// real: before the sprite fill was made contiguous, a paint took 85 ms against a 50 ms
/// budget. The loop survived on luck, and luck is not a scheduling policy.
///
/// One *tick*, not one millisecond. ESP-IDF here runs at `CONFIG_FREERTOS_HZ = 100`, so a
/// tick is 10 ms — and a sleep shorter than a tick cannot yield at all. It falls through to a
/// busy wait, which is worse than not sleeping, because it burns the core while looking in
/// the source like a pause. A 1 ms floor was therefore no floor at all.
///
/// Found on the metal, not on the host: the orientation readout's first flash asked for a
/// 20 ms cadence against a 31 ms paint, took this floor on every frame, and starved the idle
/// task on core 1 until the task watchdog fired continuously. A host `cargo test` cannot see
/// this — `std::thread::sleep` on Linux yields at any duration.
pub const MIN_YIELD: Duration = Duration::from_millis(10);

/// How often the loop checks back while the glass is **dark**.
///
/// A dark screen is painted for nobody, so the loop skips the render entirely — the whole point
/// of a backlight toggle is to stop doing work, and the expensive part is the SPI traffic, not
/// the wake. But it must still notice promptly when the glass comes back, so it keeps a slow
/// heartbeat rather than sleeping until the next second boundary: 100 ms costs one relaxed
/// atomic load ten times a second and bounds the wake latency below what a thumb can feel.
pub const DARK_PERIOD: Duration = Duration::from_millis(100);

/// How often the loop reports the frame rate it is actually holding, in milliseconds.
///
/// Only *painted* frames are counted, so a still screen — which paints once and then suppresses
/// — reports nothing: a resting device stays silent. While a creature animates, one line every
/// five seconds carries the achieved fps and the real paint cost without a log entry per frame.
pub const FPS_REPORT_PERIOD: Tick = 5_000;

/// The display thread's stack, in bytes.
///
/// On-device this sizes a FreeRTOS task stack, so it is set explicitly. embedded-graphics
/// text + a screen clear stream through the adapter's SPI buffer at constant stack depth, so
/// 8 KiB holds it with headroom on the ESP32's scarce SRAM — but, like every stack here, it
/// is validated against the true high-water mark on the metal before it is trusted.
pub const DISPLAY_STACK_SIZE: usize = 8 * 1024;

/// How the display loop is tuned: its two cadences and its stack.
///
/// [`Default`] gives the module constants, which is what every app so far wants; the fields
/// are public so a composition root can override one. [`Copy`], so it can be built and still
/// moved into the thread.
#[derive(Clone, Copy)]
pub struct DisplayConfig {
    /// The interval between ticks while the picture is still.
    pub period: Duration,
    /// The interval between ticks while the creature animates.
    pub animation_period: Duration,
    /// The interval between checks while the glass is dark and nothing is painted.
    pub dark_period: Duration,
    /// How long a window of painted frames the loop aggregates before it logs the frame rate.
    pub fps_report_period: Tick,
    /// The display thread's stack size, in bytes.
    pub stack_size: usize,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            period: RENDER_PERIOD,
            animation_period: ANIMATION_PERIOD,
            dark_period: DARK_PERIOD,
            fps_report_period: FPS_REPORT_PERIOD,
            stack_size: DISPLAY_STACK_SIZE,
        }
    }
}

impl DisplayConfig {
    /// How long to sleep after painting `glass`: a fast tick while a creature moves, the slow
    /// one otherwise (including the very first tick, before anything is drawn).
    fn tick_period<S: Animated>(&self, glass: Glass<S>) -> Duration {
        match glass {
            Some(shown) if shown.state.is_animated() => self.animation_period,
            _ => self.period,
        }
    }
}

/// A rolling frame-rate meter over a window of painted frames.
///
/// Fed each painted frame's finish time and paint cost, it aggregates a window and hands back a
/// [`FrameStats`] the moment the window has elapsed, then starts a fresh one — so the loop can
/// report the frame rate it is really holding with one log line every few seconds rather than a
/// line per frame. Suppressed ticks are never offered to it, so a still screen reports nothing.
///
/// It carries no clock: the loop feeds it the tick it already read, which keeps the whole rule a
/// pure function of its inputs and testable without a thread or a real clock.
#[derive(Clone, Copy)]
struct FrameMeter {
    window_start: Tick,
    frames: u32,
    busy_ms: u64,
    peak_ms: u64,
}

/// What a meter reports when its window closes: how many frames it painted, over how long, and
/// how dear each paint was (the mean and the worst).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FrameStats {
    frames: u32,
    span_ms: Tick,
    busy_ms: u64,
    peak_ms: u64,
}

impl FrameMeter {
    /// A fresh window opening at `now` with nothing recorded.
    fn start(now: Tick) -> Self {
        FrameMeter {
            window_start: now,
            frames: 0,
            busy_ms: 0,
            peak_ms: 0,
        }
    }

    /// Record a painted frame that finished at `now` costing `took`; return a [`FrameStats`] and
    /// reopen the window once `window` milliseconds have elapsed since it opened, else `None`.
    fn painted(&mut self, now: Tick, took: Duration, window: Tick) -> Option<FrameStats> {
        let ms: u64 = took.as_millis() as u64;
        self.frames += 1;
        self.busy_ms += ms;
        self.peak_ms = self.peak_ms.max(ms);
        let span: Tick = now.saturating_sub(self.window_start);
        if span < window {
            return None;
        }
        let stats: FrameStats = FrameStats {
            frames: self.frames,
            span_ms: span,
            busy_ms: self.busy_ms,
            peak_ms: self.peak_ms,
        };
        *self = FrameMeter::start(now);
        Some(stats)
    }
}

impl FrameStats {
    /// Frames painted per second across the window — the frame rate the panel actually held.
    fn fps(&self) -> f32 {
        if self.span_ms == 0 {
            0.0
        } else {
            self.frames as f32 * 1_000.0 / self.span_ms as f32
        }
    }

    /// Mean paint cost over the window, in milliseconds — the headroom under the tick budget.
    fn mean_paint_ms(&self) -> f32 {
        if self.frames == 0 {
            0.0
        } else {
            self.busy_ms as f32 / self.frames as f32
        }
    }
}

/// A running display thread — a handle to stop and join it.
///
/// Dropping the handle detaches the thread (it keeps repainting), which is what a composition
/// root wants: the app renders for the life of the program. Tests and a future clean-shutdown
/// path use [`stop`](Self::stop) + [`join`](Self::join).
pub struct DisplayTask {
    handle: JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

impl DisplayTask {
    /// Ask the display loop to finish after its current cycle.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Block until the display thread has exited, propagating a panic it carried.
    pub fn join(self) -> thread::Result<()> {
        self.handle.join()
    }
}

/// Spawn the display thread: `source` → [`Screen::show`], every `config.period`
/// (or `animation_period` while the creature moves).
///
/// `source` computes the current app state from whatever the app owns; it moves into the
/// thread, so it must be [`Send`] + `'static`. `rotation` does the same for which way up the
/// picture is drawn — an app with no rotation source hands back a constant, and pays nothing.
/// `lit` says whether the glass is currently worth painting; an app with no backlight control
/// passes [`LitFlag::always(true)`](LitFlag::always). `clock` is the shared time base — the same
/// one the app's writer stamps against — so a state's age is measured on one clock. `display`
/// likewise moves in; its error must be [`Display`] so a render failure can be logged.
///
/// Returns the [`DisplayTask`] handle, or the [`io::Error`] from failing to spawn the OS/RTOS
/// thread.
pub fn spawn_display<S, D, F, R, C>(
    display: D,
    source: F,
    rotation: R,
    lit: LitFlag,
    clock: C,
    config: DisplayConfig,
) -> io::Result<DisplayTask>
where
    S: Animated + Send + 'static,
    D: Screen<S> + Send + 'static,
    D::Error: Display,
    F: FnMut(Tick) -> S + Send + 'static,
    R: FnMut(Tick) -> ScreenRotation + Send + 'static,
    C: Clock + Send + 'static,
{
    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let stop_in_thread: Arc<AtomicBool> = Arc::clone(&stop);
    let handle: JoinHandle<()> = thread::Builder::new()
        .name("platform-display".to_string())
        .stack_size(config.stack_size)
        .spawn(move || {
            render_loop(
                display,
                source,
                rotation,
                lit,
                clock,
                config,
                stop_in_thread,
            )
        })?;
    Ok(DisplayTask { handle, stop })
}

/// What is currently on the glass: the state painted, the *frame* of its creature that was
/// painted, which way up it was drawn, and the [`Tick`] at which this state's
/// [`anchor`](Animated::anchor) last changed.
///
/// `rotation` is a field rather than an argument passed straight through because this struct
/// *is* the repaint-suppression rule. A turn that did not enter the comparison would leave the
/// glass holding the previous quadrant's pixels, correctly rotated and wrong, until the app's
/// own state happened to change.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Shown<S: Animated> {
    state: S,
    frame: usize,
    rotation: ScreenRotation,
    since: Tick,
}

/// The glass, or nothing yet (before the first paint).
type Glass<S> = Option<Shown<S>>;

/// The thread body: on each tick, redraw only if the picture changed — until asked to stop.
///
/// A dark glass short-circuits the whole cycle: no state is read, no pixel is pushed, no SPI
/// transaction is issued. The remembered picture is dropped at the same time, so whatever the
/// app did while nobody was looking is painted in full the moment the light comes back — the
/// panel keeps its framebuffer across a backlight cut, so without that reset a stale picture
/// would sit on the glass until the app's state next happened to change.
fn render_loop<S, D, F, R, C>(
    mut display: D,
    mut source: F,
    mut rotation: R,
    lit: LitFlag,
    clock: C,
    config: DisplayConfig,
    stop: Arc<AtomicBool>,
) where
    S: Animated,
    D: Screen<S>,
    D::Error: Display,
    F: FnMut(Tick) -> S,
    R: FnMut(Tick) -> ScreenRotation,
    C: Clock,
{
    let mut glass: Glass<S> = None;
    let mut meter: FrameMeter = FrameMeter::start(clock.now());
    while !stop.load(Ordering::Relaxed) {
        if !lit.is_lit() {
            glass = None;
            thread::sleep(config.dark_period.max(MIN_YIELD));
            continue;
        }
        let now: Tick = clock.now();
        let before: Glass<S> = glass;
        let started: Instant = Instant::now();
        glass = render_once(&mut display, &mut source, &mut rotation, now, glass);
        let budget: Duration = config.tick_period(glass);

        // A paint that outruns its own tick means the creature cannot keep its timing: the
        // panel silently drops frames and the animation drags. Nothing on the glass says so,
        // so the loop says it here. Only an actual repaint is measured; a suppressed tick
        // costs nothing — and a paint that carried a turn is reported as the once-per-turn
        // cost it is rather than as a failure. Both logs sit outside the timed region.
        let painted: bool = glass != before;
        let took: Duration = started.elapsed();

        // The frame rate the panel is actually holding — one line every window, over painted
        // frames only, so an animating creature reports its cadence and a still screen stays
        // quiet. Read the tick once more here rather than reuse `now`: the paint's own cost has
        // passed, and the window is measured against wall time, not the pre-paint instant.
        if painted {
            if let Some(stats) = meter.painted(clock.now(), took, config.fps_report_period) {
                info!(
                    "platform-display: {:.1} fps over {} frames, paint {:.1} ms avg / {} ms peak \
                     (budget {budget:?})",
                    stats.fps(),
                    stats.frames,
                    stats.mean_paint_ms(),
                    stats.peak_ms,
                );
            }
        }

        match report_for(painted, turned(before, glass), took, budget) {
            PaintReport::Quiet => {}
            PaintReport::OverBudget => {
                warn!("platform-display: paint took {took:?}, over the {budget:?} tick budget")
            }
            PaintReport::TurnCost => info!(
                "platform-display: paint took {took:?} carrying a turn (budget {budget:?}) — \
                 the panel clears on a rotation change; expected, once per turn"
            ),
        }

        // The cadence follows what is on the glass: fast while a creature moves, slow once it
        // is still. The paint's own cost is subtracted so the frame rate does not drift, and
        // the sleep never reaches zero — see [`MIN_YIELD`].
        thread::sleep(budget.saturating_sub(took).max(MIN_YIELD));
    }
}

/// What the loop has to say about a paint that has just finished.
///
/// Three outcomes rather than a bool, because "over the budget" and "over the budget *because
/// it turned*" are different events that happened to trip the same threshold, and reporting
/// them the same way is what sent a whole profiling epic looking for a thread that did not
/// exist. See `kb/findings/turning-the-panel-costs-a-full-screen-clear.md`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PaintReport {
    /// Nothing worth saying: nothing was painted, or what was painted fit its budget.
    Quiet,
    /// The paint outran its tick budget. The creature cannot keep its timing and the
    /// animation will drag — a real fault, and the reason this alarm exists.
    OverBudget,
    /// The paint outran its budget *carrying a rotation change*.
    ///
    /// Not a fault. `Panel::set_rotation` early-returns when the rotation is unchanged, but on
    /// a real change it writes MADCTL **and clears the whole screen**, inside the same
    /// `Screen::show` this loop is timing — measured at 59.6 ms against 21.5 ms settled. Every
    /// pixel changes meaning when the glass turns, so that clear is work, not waste.
    ///
    /// It is exempt because it is a **once-per-turn** cost being judged by a **per-frame**
    /// budget. A turn is rare and user-initiated, and one dropped animation frame while the
    /// board is in someone's hand is invisible. Reported rather than silenced, so the cost
    /// stays visible in the log and a *regression* in it would still show up.
    TurnCost,
}

/// Whether the glass turned between these two paints.
///
/// Exactly the condition under which the panel adapter pays for a turn: `set_rotation`
/// compares against the rotation it is already showing and early-returns when they match.
///
/// The very first paint is deliberately **not** treated as a turn. `before` is `None` so there
/// is no previous rotation to have changed from, and a first paint that genuinely overruns is
/// worth one honest warning at boot rather than a special case that hides it.
fn turned<S: Animated>(before: Glass<S>, after: Glass<S>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => before.rotation != after.rotation,
        _ => false,
    }
}

/// Which report a finished paint earns.
///
/// Split out from the loop so the rule is exercised without a thread, a clock or a panel.
fn report_for(painted: bool, turned: bool, took: Duration, budget: Duration) -> PaintReport {
    match (painted && took > budget, turned) {
        (false, _) => PaintReport::Quiet,
        (true, true) => PaintReport::TurnCost,
        (true, false) => PaintReport::OverBudget,
    }
}

/// One render cycle: read the freshest state and draw it *only if the picture changed*.
///
/// The whole shell↔adapter seam, in isolation and testable without a thread. The picture is
/// the pair `(state, frame)`, where the frame comes from [`Animated::frame_index`]. Suppression
/// compares the pair, so a still state (constant frame) paints once, while an animated state
/// repaints only on the ticks a new frame is due.
///
/// `since` — when the current state's anchor last changed — is the animation's zero. It is
/// kept while the anchor holds and reset when it changes, so a creature always starts at its
/// first frame on a real transition but keeps counting across the app's value updates.
///
/// A render error is logged, not propagated, and leaves the glass unchanged so the next tick
/// retries.
fn render_once<S, D, F, R>(
    display: &mut D,
    source: &mut F,
    rotation: &mut R,
    now: Tick,
    glass: Glass<S>,
) -> Glass<S>
where
    S: Animated,
    D: Screen<S>,
    D::Error: Display,
    F: FnMut(Tick) -> S,
    R: FnMut(Tick) -> ScreenRotation,
{
    let state: S = source(now);
    let rotation: ScreenRotation = rotation(now);

    // The animation's zero: the tick this state's anchor became current. A changed anchor
    // restarts the creature; an unchanged one keeps counting across value updates.
    let since: Tick = match glass {
        Some(shown) if shown.state.anchor() == state.anchor() => shown.since,
        _ => now,
    };
    let elapsed: Tick = now.saturating_sub(since);
    // Only a state that says it is animated gets a moving frame number. `frame` is part of the
    // repaint-suppression comparison, so on a still picture a counter that kept advancing would
    // fail that comparison every single tick and repaint a picture that had not changed — and a
    // renderer that clears its region before drawing turns that into a visible flicker. What
    // `is_animated` means is "this picture changes with time"; when it is false, it must not.
    let frame: usize = if state.is_animated() {
        state.frame_index(elapsed)
    } else {
        0
    };

    let next: Shown<S> = Shown {
        state,
        frame,
        rotation,
        since,
    };
    if glass == Some(next) {
        return glass; // same picture — leave the glass untouched.
    }
    match display.show(state, elapsed, rotation) {
        Ok(()) => Some(next),
        Err(err) => {
            warn!("platform-display: render failed, skipping this cycle: {err}");
            glass // keep the prior state; retry next tick.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::Mutex;

    use crate::backlight::BacklightSwitch;
    use crate::clock::Monotonic;
    use platform_core::Backlight;

    /// Frames advance every this many ms in the test's [`Moving`](TestState::Moving) state.
    const FRAME_HOLD: Tick = 100;
    /// Frames in the moving loop.
    const FRAMES: usize = 4;

    /// A test app state: a value that is drawn, with an [`anchor`](Animated::anchor) that
    /// deliberately excludes the value — so `Still(30)` and `Still(60)` share an anchor, the
    /// way a pomodoro's `mm:ss` value shares the phase's anchor. `Still` is motionless (frame
    /// 0 forever); `Moving` animates on its own clock.
    /// `Ticking` is the regression shape: a state that says it is **not** animated while its
    /// `frame_index` still moves with the clock — which is what every app state does whose
    /// creature is simply not on the current screen, because the frame is computed from the
    /// creature's own selector and never asked whether it is being drawn.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum TestState {
        Still(u8),
        Ticking(u8),
        Moving(u8),
    }

    impl Animated for TestState {
        /// 0 = still, 1 = moving — the drawn value is *not* part of the identity that resets
        /// the animation clock.
        type Anchor = u8;

        fn anchor(&self) -> u8 {
            match self {
                TestState::Still(_) => 0,
                TestState::Ticking(_) => 2,
                TestState::Moving(_) => 1,
            }
        }

        fn is_animated(&self) -> bool {
            matches!(self, TestState::Moving(_))
        }

        fn frame_index(&self, elapsed_ms: Tick) -> usize {
            match self {
                TestState::Still(_) => 0,
                TestState::Ticking(_) | TestState::Moving(_) => {
                    ((elapsed_ms / FRAME_HOLD) as usize) % FRAMES
                }
            }
        }
    }

    /// A render error whose message is [`Display`], as the loop's log bound requires.
    #[derive(Clone, Debug)]
    struct TestError(&'static str);

    impl Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    /// A [`Screen`] fake that records every `(state, elapsed)` it is shown, so a test can
    /// assert the exact sequence the loop painted. `fail` makes every `show` error (still
    /// recording first); `ping` signals each render so a thread test can await a real repaint.
    struct FakeScreen {
        shown: Arc<Mutex<Vec<TestState>>>,
        elapsed: Arc<Mutex<Vec<Tick>>>,
        drawn_at: Arc<Mutex<Vec<ScreenRotation>>>,
        fail: bool,
        ping: Option<Sender<()>>,
    }

    impl FakeScreen {
        fn new() -> (Self, Arc<Mutex<Vec<TestState>>>) {
            let shown: Arc<Mutex<Vec<TestState>>> = Arc::new(Mutex::new(Vec::new()));
            let screen: FakeScreen = FakeScreen {
                shown: Arc::clone(&shown),
                elapsed: Arc::new(Mutex::new(Vec::new())),
                drawn_at: Arc::new(Mutex::new(Vec::new())),
                fail: false,
                ping: None,
            };
            (screen, shown)
        }

        fn failing() -> Self {
            FakeScreen {
                shown: Arc::new(Mutex::new(Vec::new())),
                elapsed: Arc::new(Mutex::new(Vec::new())),
                drawn_at: Arc::new(Mutex::new(Vec::new())),
                fail: true,
                ping: None,
            }
        }

        fn pinging(mut self, tx: Sender<()>) -> Self {
            self.ping = Some(tx);
            self
        }

        fn elapsed_log(&self) -> Vec<Tick> {
            self.elapsed.lock().expect("elapsed log lock").clone()
        }

        /// Which way up each paint was drawn, in order.
        fn rotation_log(&self) -> Vec<ScreenRotation> {
            self.drawn_at.lock().expect("rotation log lock").clone()
        }
    }

    impl Screen<TestState> for FakeScreen {
        type Error = TestError;

        fn show(
            &mut self,
            state: TestState,
            elapsed: Tick,
            rotation: ScreenRotation,
        ) -> Result<(), TestError> {
            self.shown.lock().expect("shown log lock").push(state);
            self.elapsed.lock().expect("elapsed log lock").push(elapsed);
            self.drawn_at
                .lock()
                .expect("rotation log lock")
                .push(rotation);
            if let Some(tx) = &self.ping {
                let _ = tx.send(());
            }
            if self.fail {
                Err(TestError("panel offline"))
            } else {
                Ok(())
            }
        }
    }

    /// A source that always yields `state`.
    fn fixed(state: TestState) -> impl FnMut(Tick) -> TestState {
        move |_now: Tick| state
    }

    /// A rotation source that never turns — what an app with no IMU hands the loop.
    fn landscape() -> impl FnMut(Tick) -> ScreenRotation {
        |_now: Tick| ScreenRotation::Deg0
    }

    /// A rotation source that turns once, at `turn_at`.
    fn turning_at(turn_at: Tick) -> impl FnMut(Tick) -> ScreenRotation {
        move |now: Tick| {
            if now >= turn_at {
                ScreenRotation::Deg90
            } else {
                ScreenRotation::Deg0
            }
        }
    }

    /// The states a glass is showing, in order.
    fn shown_states(shown: &Arc<Mutex<Vec<TestState>>>) -> Vec<TestState> {
        shown.lock().expect("shown log lock").clone()
    }

    /// Zero — nothing was painted, so there is nothing to report however long the tick took.
    /// A suppressed tick costs nothing and must never raise an alarm.
    #[test]
    fn a_tick_that_painted_nothing_is_quiet() {
        assert_eq!(
            report_for(false, false, Duration::from_secs(9), ANIMATION_PERIOD),
            PaintReport::Quiet
        );
    }

    /// A paint inside its budget is quiet — including one that turned. The exemption is about
    /// what to do with an OVERRUN, not a licence to stop measuring turning paints.
    #[test]
    fn a_paint_inside_its_budget_is_quiet_whether_or_not_it_turned() {
        let quick: Duration = Duration::from_millis(21);
        assert_eq!(
            report_for(true, false, quick, ANIMATION_PERIOD),
            PaintReport::Quiet
        );
        assert_eq!(
            report_for(true, true, quick, ANIMATION_PERIOD),
            PaintReport::Quiet
        );
    }

    /// THE fault this alarm exists for: a paint that outran its budget without turning. The
    /// creature cannot keep its timing, and nothing on the glass says so.
    #[test]
    fn a_paint_that_outran_its_budget_without_turning_is_a_fault() {
        assert_eq!(
            report_for(true, false, Duration::from_millis(60), ANIMATION_PERIOD),
            PaintReport::OverBudget
        );
    }

    /// THE exemption: the same overrun, carrying a turn, is the once-per-turn full-screen clear
    /// and not a fault. Measured at 59.6 ms against a 50 ms budget, which is the case that made
    /// this distinction necessary.
    #[test]
    fn a_paint_that_outran_its_budget_carrying_a_turn_is_the_turn_cost() {
        assert_eq!(
            report_for(true, true, Duration::from_micros(59_560), ANIMATION_PERIOD),
            PaintReport::TurnCost
        );
    }

    /// A turn is a CHANGE of rotation, which is exactly when the adapter pays for one —
    /// `set_rotation` early-returns when the rotation it is handed is the one already showing.
    #[test]
    fn a_changed_rotation_is_a_turn_and_an_unchanged_one_is_not() {
        let at = |rotation: ScreenRotation| -> Glass<TestState> {
            Some(Shown {
                state: TestState::Moving(0),
                frame: 0,
                rotation,
                since: 0,
            })
        };

        assert!(turned(at(ScreenRotation::Deg0), at(ScreenRotation::Deg90)));
        assert!(!turned(
            at(ScreenRotation::Deg90),
            at(ScreenRotation::Deg90)
        ));
    }

    /// The first paint is not a turn: there is no previous rotation for it to have changed
    /// from, and a genuinely slow first paint is worth one honest warning at boot.
    #[test]
    fn the_first_paint_is_not_a_turn() {
        let after: Glass<TestState> = Some(Shown {
            state: TestState::Moving(0),
            frame: 0,
            rotation: ScreenRotation::Deg90,
            since: 0,
        });

        assert!(!turned(None, after));
        assert_eq!(
            report_for(
                true,
                turned(None, after),
                Duration::from_millis(60),
                ANIMATION_PERIOD
            ),
            PaintReport::OverBudget
        );
    }

    /// Zero closed windows: frames inside the window produce no report — the loop stays quiet
    /// until the window has elapsed, so it is one line per window and not one per frame.
    #[test]
    fn a_meter_is_quiet_until_its_window_elapses() {
        let mut meter: FrameMeter = FrameMeter::start(0);
        assert_eq!(meter.painted(10, Duration::from_millis(12), 5_000), None);
        assert_eq!(meter.painted(4_990, Duration::from_millis(12), 5_000), None);
    }

    /// One closed window reports the frames it saw. Thirty paints on a 33 ms cadence cross a
    /// ~1 s window, and the report carries all thirty.
    #[test]
    fn a_closed_window_reports_the_frames_it_saw() {
        let mut meter: FrameMeter = FrameMeter::start(0);
        let mut last: Option<FrameStats> = None;
        for frame in 1..=30u64 {
            last = meter.painted(frame * 33, Duration::from_millis(12), 990);
        }
        let stats: FrameStats = last.expect("the window closes on the frame that reaches it");
        assert_eq!(stats.frames, 30);
        assert_eq!(stats.peak_ms, 12);
    }

    /// Many paints aggregate; the peak is the worst single paint, and closing the window opens a
    /// fresh one that counts from zero rather than re-reporting immediately.
    #[test]
    fn the_peak_is_the_worst_paint_and_the_window_reopens() {
        let mut meter: FrameMeter = FrameMeter::start(0);
        assert_eq!(meter.painted(2_000, Duration::from_millis(10), 5_000), None);
        assert_eq!(meter.painted(4_000, Duration::from_millis(40), 5_000), None); // the peak
        let stats: FrameStats = meter
            .painted(6_000, Duration::from_millis(10), 5_000)
            .expect("the window closed");
        assert_eq!(stats.frames, 3);
        assert_eq!(stats.peak_ms, 40);
        assert_eq!(
            meter.painted(6_500, Duration::from_millis(10), 5_000),
            None,
            "a reopened window counts from zero"
        );
    }

    /// The reported rate is frames-over-span and the mean paint is busy-over-frames.
    #[test]
    fn fps_and_mean_paint_are_computed_from_the_window() {
        let stats: FrameStats = FrameStats {
            frames: 30,
            span_ms: 1_000,
            busy_ms: 300,
            peak_ms: 20,
        };
        assert!((stats.fps() - 30.0).abs() < 0.01);
        assert!((stats.mean_paint_ms() - 10.0).abs() < 0.01);
    }

    /// An empty or zero-span window divides by nothing and reports a zero rate, never a NaN.
    #[test]
    fn an_empty_window_has_a_zero_rate() {
        let empty: FrameStats = FrameStats {
            frames: 0,
            span_ms: 0,
            busy_ms: 0,
            peak_ms: 0,
        };
        assert_eq!(empty.fps(), 0.0);
        assert_eq!(empty.mean_paint_ms(), 0.0);
    }

    #[test]
    fn the_first_paint_shows_the_state() {
        let (mut screen, shown): (FakeScreen, _) = FakeScreen::new();
        let after: Glass<TestState> = render_once(
            &mut screen,
            &mut fixed(TestState::Still(30)),
            &mut landscape(),
            20,
            None,
        );

        assert_eq!(shown_states(&shown), vec![TestState::Still(30)]);
        assert!(after.is_some(), "the state is now on the glass");
    }

    #[test]
    fn a_steady_still_state_is_drawn_once_then_suppressed() {
        // Three ticks over an unchanging still state paint exactly once.
        let (mut screen, shown): (FakeScreen, _) = FakeScreen::new();
        let mut src = fixed(TestState::Still(30));

        let g0: Glass<TestState> = render_once(&mut screen, &mut src, &mut landscape(), 20, None);
        let g1: Glass<TestState> = render_once(&mut screen, &mut src, &mut landscape(), 21, g0);
        let _g2: Glass<TestState> = render_once(&mut screen, &mut src, &mut landscape(), 22, g1);

        assert_eq!(shown_states(&shown).len(), 1, "a steady state repainted");
    }

    /// A still state whose frame counter keeps ticking is *still still* — painted once.
    ///
    /// The regression. `frame` is part of the suppression comparison, and an app computes it
    /// from its creature's selector without asking whether the current screen draws a creature.
    /// So a state that is motionless by its own account had a frame number that advanced
    /// anyway, failed the comparison on every tick, and repainted forever. On the metal that is
    /// not a wasted cycle but a *visible flicker*, because a renderer clears its region before
    /// it draws: the buddy's charging clock blinked at the render period, which is how this was
    /// found. Three ticks, well past a frame boundary, must paint exactly once.
    #[test]
    fn a_still_state_with_a_ticking_frame_counter_is_still_painted_once() {
        let (mut screen, shown): (FakeScreen, _) = FakeScreen::new();
        let mut src = fixed(TestState::Ticking(30));

        let g0: Glass<TestState> = render_once(&mut screen, &mut src, &mut landscape(), 0, None);
        let g1: Glass<TestState> =
            render_once(&mut screen, &mut src, &mut landscape(), FRAME_HOLD, g0);
        let _g2: Glass<TestState> =
            render_once(&mut screen, &mut src, &mut landscape(), FRAME_HOLD * 2, g1);

        assert_eq!(
            shown_states(&shown).len(),
            1,
            "a state that reports itself still repainted as its frame counter advanced"
        );
    }

    /// Turning the picture repaints an otherwise unchanged state.
    ///
    /// The reason rotation is a field of `Shown` rather than an argument passed straight
    /// through to the panel. Without it, a board turned while its app sat still would rotate
    /// the glass and leave the previous quadrant's pixels on it — correctly oriented and
    /// wrong — until something else happened to change the state.
    #[test]
    fn turning_the_picture_repaints_an_unchanged_state() {
        let (mut screen, shown): (FakeScreen, _) = FakeScreen::new();
        let mut src = fixed(TestState::Still(30));
        let mut turns = turning_at(21);

        let g0: Glass<TestState> = render_once(&mut screen, &mut src, &mut turns, 20, None);
        let _g1: Glass<TestState> = render_once(&mut screen, &mut src, &mut turns, 21, g0);

        assert_eq!(
            shown_states(&shown).len(),
            2,
            "the state did not change, but the picture turned — that is a repaint"
        );
        assert_eq!(
            screen.rotation_log(),
            vec![ScreenRotation::Deg0, ScreenRotation::Deg90],
            "each paint must be drawn at the rotation current when it was painted"
        );
    }

    /// ...and a rotation that does not change suppresses exactly as before, so the new field
    /// costs a still readout no SPI traffic.
    #[test]
    fn an_unchanged_rotation_still_suppresses_the_repaint() {
        let (mut screen, shown): (FakeScreen, _) = FakeScreen::new();
        let mut src = fixed(TestState::Still(30));
        let mut turns = turning_at(21);

        // Both ticks land after the turn, so the rotation is Deg90 for each of them.
        let g0: Glass<TestState> = render_once(&mut screen, &mut src, &mut turns, 30, None);
        let _g1: Glass<TestState> = render_once(&mut screen, &mut src, &mut turns, 31, g0);

        assert_eq!(shown_states(&shown).len(), 1, "a steady picture repainted");
    }

    #[test]
    fn a_still_state_keeps_the_slow_cadence() {
        let config: DisplayConfig = DisplayConfig::default();
        let (mut screen, _shown): (FakeScreen, _) = FakeScreen::new();
        let g0: Glass<TestState> = render_once(
            &mut screen,
            &mut fixed(TestState::Still(30)),
            &mut landscape(),
            0,
            None,
        );
        assert_eq!(config.tick_period(g0), RENDER_PERIOD);
    }

    #[test]
    fn an_animated_state_repaints_as_its_frame_advances() {
        // The same moving state, one frame-hold later, is a different frame — a real repaint.
        let (mut screen, shown): (FakeScreen, _) = FakeScreen::new();
        let mut src = fixed(TestState::Moving(0));

        let g0: Glass<TestState> = render_once(&mut screen, &mut src, &mut landscape(), 100, None);
        let _g1: Glass<TestState> = render_once(
            &mut screen,
            &mut src,
            &mut landscape(),
            100 + FRAME_HOLD,
            g0,
        );

        assert_eq!(
            shown_states(&shown),
            vec![TestState::Moving(0), TestState::Moving(0)],
            "the moving state must repaint when its frame advances"
        );
        assert_eq!(
            screen.elapsed_log(),
            vec![0, FRAME_HOLD],
            "the animation clock"
        );
    }

    #[test]
    fn an_animated_state_is_not_repainted_between_frames() {
        // Ticks inside one frame's hold repaint nothing: waking is not repainting.
        let (mut screen, shown): (FakeScreen, _) = FakeScreen::new();
        let mut src = fixed(TestState::Moving(0));

        let g0: Glass<TestState> = render_once(&mut screen, &mut src, &mut landscape(), 100, None);
        let g1: Glass<TestState> = render_once(
            &mut screen,
            &mut src,
            &mut landscape(),
            100 + FRAME_HOLD / 2,
            g0,
        );
        let _g2: Glass<TestState> = render_once(
            &mut screen,
            &mut src,
            &mut landscape(),
            100 + FRAME_HOLD - 1,
            g1,
        );

        assert_eq!(shown_states(&shown).len(), 1, "a mid-frame tick repainted");
    }

    #[test]
    fn an_anchor_change_restarts_the_animation_clock() {
        // A still state giving way to a moving one restarts the creature at frame 0: the
        // elapsed handed to the second paint is 0, not the age of the first.
        let (mut screen, _shown): (FakeScreen, _) = FakeScreen::new();

        let g0: Glass<TestState> = render_once(
            &mut screen,
            &mut fixed(TestState::Still(9)),
            &mut landscape(),
            1_000,
            None,
        );
        let _g1: Glass<TestState> = render_once(
            &mut screen,
            &mut fixed(TestState::Moving(0)),
            &mut landscape(),
            1_500,
            g0,
        );

        assert_eq!(
            screen.elapsed_log(),
            vec![0, 0],
            "each new anchor starts its creature at elapsed 0"
        );
    }

    #[test]
    fn a_value_change_within_an_anchor_repaints_and_keeps_the_clock() {
        // THE pomodoro case: the drawn value changes but the anchor does not, so the panel
        // repaints the new value while the creature's clock keeps running — it is NOT reset.
        let (mut screen, shown): (FakeScreen, _) = FakeScreen::new();
        let cell: Cell<TestState> = Cell::new(TestState::Moving(0));
        let mut src = |_now: Tick| cell.get();

        let g0: Glass<TestState> = render_once(&mut screen, &mut src, &mut landscape(), 0, None);
        cell.set(TestState::Moving(1));
        let _g1: Glass<TestState> = render_once(&mut screen, &mut src, &mut landscape(), 250, g0);

        assert_eq!(
            shown_states(&shown),
            vec![TestState::Moving(0), TestState::Moving(1)],
            "a changed value must repaint"
        );
        assert_eq!(
            screen.elapsed_log(),
            vec![0, 250],
            "the animation clock kept running across the value change"
        );
    }

    #[test]
    fn a_changed_value_is_redrawn() {
        let (mut screen, shown): (FakeScreen, _) = FakeScreen::new();
        let cell: Cell<TestState> = Cell::new(TestState::Still(30));
        let mut src = |_now: Tick| cell.get();

        let g0: Glass<TestState> = render_once(&mut screen, &mut src, &mut landscape(), 10, None);
        cell.set(TestState::Still(60));
        let _g1: Glass<TestState> = render_once(&mut screen, &mut src, &mut landscape(), 20, g0);

        assert_eq!(
            shown_states(&shown),
            vec![TestState::Still(30), TestState::Still(60)]
        );
    }

    #[test]
    fn a_render_error_keeps_the_prior_state_and_is_not_fatal() {
        let mut screen: FakeScreen = FakeScreen::failing();
        let after: Glass<TestState> = render_once(
            &mut screen,
            &mut fixed(TestState::Still(55)),
            &mut landscape(),
            0,
            None,
        );
        assert!(
            after.is_none(),
            "a failed paint does not advance the shown state"
        );
    }

    #[test]
    fn the_cadence_is_fast_only_while_a_state_animates() {
        let config: DisplayConfig = DisplayConfig::default();
        let animating: Glass<TestState> = Some(Shown {
            state: TestState::Moving(0),
            frame: 0,
            rotation: ScreenRotation::Deg0,
            since: 0,
        });
        let still: Glass<TestState> = Some(Shown {
            state: TestState::Still(30),
            frame: 0,
            rotation: ScreenRotation::Deg0,
            since: 0,
        });

        assert_eq!(config.tick_period(animating), ANIMATION_PERIOD);
        assert_eq!(config.tick_period(still), RENDER_PERIOD);
        assert_eq!(config.tick_period::<TestState>(None), RENDER_PERIOD);
    }

    #[test]
    fn the_spawned_thread_redraws_on_change_and_stops_cleanly() {
        // The one integration test: spawn the real thread, paint the first value, suppress
        // the steady ticks, repaint on a change, then stop and join without a panic. Blocking
        // on pings (not polling) keeps it robust.
        let clock: Monotonic = Monotonic::start();
        let state: Arc<Mutex<TestState>> = Arc::new(Mutex::new(TestState::Still(45)));
        let (tx, rx): (Sender<()>, _) = channel();
        let (base, shown): (FakeScreen, _) = FakeScreen::new();
        let screen: FakeScreen = base.pinging(tx);
        let config: DisplayConfig = DisplayConfig {
            period: Duration::from_millis(1),
            animation_period: Duration::from_millis(1),
            dark_period: Duration::from_millis(1),
            fps_report_period: FPS_REPORT_PERIOD,
            stack_size: 256 * 1024,
        };
        let source = {
            let state: Arc<Mutex<TestState>> = Arc::clone(&state);
            move |_now: Tick| *state.lock().expect("state lock")
        };

        let task: DisplayTask = spawn_display(
            screen,
            source,
            landscape(),
            LitFlag::always(true),
            clock,
            config,
        )
        .expect("spawn display thread");

        rx.recv_timeout(Duration::from_secs(2))
            .expect("the display must paint the first value");
        *state.lock().expect("state lock") = TestState::Still(50);
        rx.recv_timeout(Duration::from_secs(2))
            .expect("a changed value must re-render");

        task.stop();
        task.join().expect("the display thread must not panic");

        assert_eq!(
            shown_states(&shown),
            vec![TestState::Still(45), TestState::Still(50)]
        );
    }

    /// A backlight that always succeeds — this test cares about the flag, not the rail.
    struct OkBacklight {
        lit: bool,
    }

    impl platform_core::Backlight for OkBacklight {
        type Error = std::convert::Infallible;

        fn is_lit(&self) -> bool {
            self.lit
        }

        fn set(&mut self, lit: bool) -> Result<(), Self::Error> {
            self.lit = lit;
            Ok(())
        }
    }

    /// A dark glass is not painted, and lighting it again repaints in full even though the app
    /// state never changed.
    ///
    /// Both halves matter and they are one behaviour. Skipping the paint is the point of the
    /// toggle — a screen nobody can see must not cost SPI traffic. Repainting on the way back is
    /// what makes that safe: the panel keeps its framebuffer across a backlight cut, so the
    /// stale picture would otherwise sit there until the app's state happened to move.
    #[test]
    fn a_dark_glass_is_not_painted_and_lighting_it_repaints() {
        let clock: Monotonic = Monotonic::start();
        let (tx, rx): (Sender<()>, _) = channel();
        let (base, shown): (FakeScreen, _) = FakeScreen::new();
        let screen: FakeScreen = base.pinging(tx);
        let config: DisplayConfig = DisplayConfig {
            period: Duration::from_millis(1),
            animation_period: Duration::from_millis(1),
            dark_period: Duration::from_millis(1),
            fps_report_period: FPS_REPORT_PERIOD,
            stack_size: 256 * 1024,
        };
        let mut switch: BacklightSwitch<OkBacklight> =
            BacklightSwitch::new(OkBacklight { lit: true });

        let task: DisplayTask = spawn_display(
            screen,
            fixed(TestState::Still(45)),
            landscape(),
            switch.flag(),
            clock,
            config,
        )
        .expect("spawn display thread");

        rx.recv_timeout(Duration::from_secs(2))
            .expect("the display must paint the first value while lit");

        // Go dark, let any paint already in flight land, then take the silence as the assertion.
        switch.toggle().expect("darken the glass");
        thread::sleep(Duration::from_millis(50));
        while rx.try_recv().is_ok() {}
        assert!(
            rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "a dark glass must not be painted"
        );

        // Light it again: the state never changed, so only the dropped picture can explain a
        // repaint — which is exactly the guarantee.
        switch.toggle().expect("light the glass");
        rx.recv_timeout(Duration::from_secs(2))
            .expect("lighting the glass must repaint it");

        task.stop();
        task.join().expect("the display thread must not panic");

        let painted: Vec<TestState> = shown_states(&shown);
        assert_eq!(
            painted,
            vec![TestState::Still(45), TestState::Still(45)],
            "exactly two paints: the first, and the one the light coming back forced"
        );
    }
}
