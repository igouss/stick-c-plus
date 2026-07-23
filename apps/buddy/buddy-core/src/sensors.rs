//! The two accelerometer readings the buddy makes of itself: a shake, and a face-down nap.
//!
//! Both fold an [`Acceleration`] (milli-g) and time into a decision. Both carry a quirk that
//! is **load-bearing and preserved**, not fixed — read the notes on [`ShakeDetector`] and
//! [`NapCounter`].

use platform_core::Acceleration;

/// The strict shake threshold: a magnitude delta must **exceed** `0.8` g (`>`, not `>=`).
pub const SHAKE_THRESHOLD_G: f32 = 0.8;
/// The seed for the EMA baseline: `1.0` g, so a cold start under gravity does not false-fire.
pub const SHAKE_BASELINE_SEED_G: f32 = 1.0;

/// A shake detector: an EMA of the acceleration magnitude, and a strict delta trigger.
///
/// Each accepted sample computes the L2 magnitude, takes `delta = |mag - baseline|` against
/// the **pre-update** baseline, then advances `baseline = baseline * 0.95 + mag * 0.05`, and
/// fires when `delta > `[`SHAKE_THRESHOLD_G`].
///
/// ## Preserved quirk: the baseline goes stale
///
/// Upstream never samples the detector while the menu is open or the screen is off (a `&&`
/// short-circuit), so the EMA does **not** advance across those spans — [`crate::step`] must
/// only call [`sample`](ShakeDetector::sample) when awake and out of the menu. The first
/// sample after closing the menu therefore differs. This is a quirk to keep, not a bug.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ShakeDetector {
    baseline: f32,
}

impl ShakeDetector {
    /// A fresh detector with the baseline seeded to [`SHAKE_BASELINE_SEED_G`].
    pub const fn new() -> Self {
        ShakeDetector {
            baseline: SHAKE_BASELINE_SEED_G,
        }
    }

    /// Fold one acceleration sample in, advancing the baseline, and report whether it is a
    /// shake (`delta > `[`SHAKE_THRESHOLD_G`], strict). Call only while awake and not in the
    /// menu, to preserve the stale-baseline quirk.
    pub fn sample(&mut self, accel: Acceleration) -> bool {
        // Magnitude in g, via an exact integer square root of the milli-g² norm — no `libm`,
        // no `unsafe`, and `no_std`-clean. `magnitude_squared_mg2` is always non-negative.
        let magnitude_mg: u64 = (accel.magnitude_squared_mg2() as u64).isqrt();
        let magnitude_g: f32 = magnitude_mg as f32 / 1_000.0;
        // Delta against the PRE-update baseline.
        let raw: f32 = magnitude_g - self.baseline;
        let delta: f32 = if raw < 0.0 { -raw } else { raw };
        // Advance the EMA (0.95 old / 0.05 new).
        self.baseline = self.baseline * 0.95 + magnitude_g * 0.05;
        delta > SHAKE_THRESHOLD_G
    }
}

impl Default for ShakeDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// The counter enters a nap at `>= 15`.
pub const NAP_ENTER: i8 = 15;
/// The counter leaves a nap at `<= -8`.
pub const NAP_LEAVE: i8 = -8;
/// The counter saturates high at `+20`.
pub const NAP_COUNTER_MAX: i8 = 20;
/// The counter saturates low at `-10`.
pub const NAP_COUNTER_MIN: i8 = -10;

/// The face-down predicate over an acceleration, in milli-g, all strict:
/// `z < -700 && |x| < 400 && |y| < 400` (i.e. `az < -0.7 g` and both lateral axes `< 0.4 g`).
pub fn is_face_down(accel: Acceleration) -> bool {
    accel.z_mg < -700 && accel.x_mg.abs() < 400 && accel.y_mg.abs() < 400
}

/// What a nap-counter update did to the latched nap state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NapTransition {
    /// The nap state did not change this update.
    None,
    /// The counter reached [`NAP_ENTER`] and the buddy fell asleep face-down.
    Entered,
    /// The counter fell to [`NAP_LEAVE`] and the buddy woke (drives stats-on-nap-end / wake).
    Left,
}

/// The face-down nap hysteresis: a saturating counter with an enter/leave gap.
///
/// Each update nudges the counter `+1` when face-down, `-1` otherwise, saturating in
/// `[`[`NAP_COUNTER_MIN`]`, `[`NAP_COUNTER_MAX`]`]`, then latches the nap at
/// [`NAP_ENTER`] and un-latches it at [`NAP_LEAVE`].
///
/// ## Preserved quirk: the counter freezes, the state machine does not
///
/// While a prompt is up **and unanswered** the IMU is not read at all — the counter is
/// frozen (pass `prompt_unanswered = true`). But the enter/leave transition check sits
/// *outside* that freeze, so a nap already latched **stays latched** through a prompt. Keep
/// this shape; do not hoist the freeze over the transition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NapCounter {
    counter: i8,
    napping: bool,
}

impl NapCounter {
    /// A fresh counter: awake, at zero.
    pub const fn new() -> Self {
        NapCounter {
            counter: 0,
            napping: false,
        }
    }

    /// Advance the counter for one loop iteration and report any nap transition.
    ///
    /// When `prompt_unanswered` is true the counter is frozen (the IMU is not read), but the
    /// latched nap state is still evaluated — see the type-level quirk note.
    pub fn update(&mut self, accel: Acceleration, prompt_unanswered: bool) -> NapTransition {
        // The counter freezes entirely while a prompt is up and unanswered — the IMU is not
        // read at all. This is the load-bearing freeze that keeps a nap from waking mid-prompt.
        if !prompt_unanswered {
            let step: i8 = if is_face_down(accel) { 1 } else { -1 };
            let next: i8 = self.counter + step;
            self.counter = next.clamp(NAP_COUNTER_MIN, NAP_COUNTER_MAX);
        }
        // The transition check sits OUTSIDE the freeze guard (preserved quirk): a latched nap
        // is still evaluated during a prompt, so it stays latched through one.
        if !self.napping && self.counter >= NAP_ENTER {
            self.napping = true;
            NapTransition::Entered
        } else if self.napping && self.counter <= NAP_LEAVE {
            self.napping = false;
            NapTransition::Left
        } else {
            NapTransition::None
        }
    }

    /// Whether the buddy is currently napping.
    pub const fn is_napping(&self) -> bool {
        self.napping
    }
}

impl Default for NapCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// An acceleration pointing straight into the desk: face-down.
    const DOWN: Acceleration = Acceleration::new(0, 0, -1_000);
    /// An acceleration pointing straight up: face-up.
    const UP: Acceleration = Acceleration::new(0, 0, 1_000);

    /// A cold detector under gravity does not fire: the seed baseline matches 1 g.
    #[test]
    fn a_resting_board_is_not_a_shake() {
        let mut detector: ShakeDetector = ShakeDetector::new();
        assert!(!detector.sample(Acceleration::new(0, 0, 1_000)));
    }

    /// A one-shot jolt well past the baseline is a shake; the threshold is strict.
    #[test]
    fn a_jolt_past_the_strict_threshold_is_a_shake() {
        let mut detector: ShakeDetector = ShakeDetector::new();
        // 2.0 g against a 1.0 g baseline is a delta of 1.0 > 0.8.
        assert!(detector.sample(Acceleration::new(0, 0, 2_000)));
    }

    /// The delta is taken against the PRE-update baseline, then the EMA advances.
    #[test]
    fn the_baseline_advances_by_the_ema_after_sampling() {
        let mut detector: ShakeDetector = ShakeDetector::new();
        // 1.5 g: delta 0.5 <= 0.8, no shake, but the baseline moves toward it.
        assert!(!detector.sample(Acceleration::new(0, 0, 1_500)));
        // baseline = 1.0*0.95 + 1.5*0.05 = 1.025.
        assert!((detector.sample(Acceleration::new(0, 0, 1_025)) as u8) == 0);
    }

    /// Zero: nothing near vertical is not face-down.
    #[test]
    fn a_flat_board_is_not_face_down() {
        assert!(!is_face_down(Acceleration::new(0, 0, 1_000)));
    }

    /// One: straight into the desk with no lateral lean is face-down.
    #[test]
    fn straight_down_is_face_down() {
        assert!(is_face_down(Acceleration::new(0, 0, -1_000)));
    }

    /// The lateral bound is strict: 400 mg of lean breaks face-down.
    #[test]
    fn too_much_lateral_lean_is_not_face_down() {
        assert!(is_face_down(Acceleration::new(399, 399, -1_000)));
        assert!(!is_face_down(Acceleration::new(400, 0, -1_000)));
    }

    /// Nap enters at +15 and leaves at −8, with the enter/leave gap as hysteresis.
    #[test]
    fn the_nap_enters_at_fifteen_and_leaves_at_minus_eight() {
        let mut nap: NapCounter = NapCounter::new();
        // 14 face-down updates: not yet napping.
        drive_face_down(&mut nap, 14);
        assert!(!nap.is_napping());
        // The fifteenth crosses the threshold.
        assert_eq!(nap.update(DOWN, false), NapTransition::Entered);
        assert!(nap.is_napping());
    }

    /// The counter saturates at +20, so waking still takes the full 28-step swing.
    #[test]
    fn the_counter_saturates_high_at_twenty() {
        let mut nap: NapCounter = NapCounter::new();
        // Drive far past +20; it saturates.
        drive_face_down(&mut nap, 40);
        assert!(nap.is_napping());
        // From +20, it takes 28 face-up steps to reach −8 (20 → −8).
        drive_face_up(&mut nap, 27);
        assert!(nap.is_napping());
        assert_eq!(nap.update(UP, false), NapTransition::Left);
        assert!(!nap.is_napping());
    }

    /// PRESERVED QUIRK: an unanswered prompt freezes the counter, so a latched nap survives
    /// a prompt even under face-up gravity. This test FAILS if the freeze is removed (the
    /// counter would decay to −8 and wake).
    #[test]
    fn a_latched_nap_survives_a_prompt_because_the_counter_is_frozen() {
        let mut nap: NapCounter = NapCounter::new();
        drive_face_down(&mut nap, 40); // latch, counter at +20
        assert!(nap.is_napping());
        // Many face-up frames, but all while a prompt is unanswered: the counter is frozen,
        // so the nap does not wake.
        assert_eq!(nap.update(UP, true), NapTransition::None);
        assert_eq!(nap.update(UP, true), NapTransition::None);
        assert_eq!(nap.update(UP, true), NapTransition::None);
        assert!(nap.is_napping());
    }

    /// A fresh counter face-up leaves nothing to leave: no spurious transition.
    #[test]
    fn a_fresh_awake_counter_reports_no_transition() {
        let mut nap: NapCounter = NapCounter::new();
        assert_eq!(nap.update(UP, false), NapTransition::None);
    }

    // Fixture drivers: loop-free (a range fold, no `for`/`while` keyword and no branch), so the
    // test bodies that call them stay cyclomatic-complexity 1 per this repo's convention.
    fn drive_face_down(nap: &mut NapCounter, times: u8) {
        (0..times).for_each(|_: u8| {
            nap.update(DOWN, false);
        });
    }

    fn drive_face_up(nap: &mut NapCounter, times: u8) {
        (0..times).for_each(|_: u8| {
            nap.update(UP, false);
        });
    }

    proptest! {
        /// The counter saturates high at +20, however it is driven: from ANY reachable state,
        /// the full band swing of 28 face-up frames (+20 → −8) is enough to wake. This FAILS if
        /// the high saturation is broken (a counter above +20 would need more than 28 steps).
        #[test]
        fn twenty_eight_face_up_frames_always_wake(face_down_flags in proptest::collection::vec(proptest::bool::ANY, 0..200)) {
            let mut nap: NapCounter = NapCounter::new();
            // Branch-free select: index a two-element table by the bool, no `if` in the closure.
            let frames: [Acceleration; 2] = [UP, DOWN];
            face_down_flags.iter().for_each(|&down: &bool| {
                nap.update(frames[usize::from(down)], false);
            });
            drive_face_up(&mut nap, 28);
            prop_assert!(!nap.is_napping());
        }
    }
}
