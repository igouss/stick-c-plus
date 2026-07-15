//! Which creature the glass shows for an observation, and whether it moves.
//!
//! The whole animation policy, as pure functions of an [`Observation`] and elapsed
//! milliseconds. No clock, no timer, no state — the shell supplies both arguments and
//! this module answers, so every rule below is decided on the host.
//!
//! ## The creature *is* the status
//!
//! | Observation | Creature | Motion |
//! |---|---|---|
//! | [`Fresh`](Observation::Fresh) | breathing | **still** |
//! | [`Faulted`](Observation::Faulted) | startled | animated |
//! | [`Stale`](Observation::Stale) | asleep | animated |
//! | [`NeverSampled`](Observation::NeverSampled) | thinking | animated |
//!
//! A healthy monitor shows a *motionless* creature. That is not an aesthetic choice: a
//! still scene means [`frame_index`] is constant, so the render loop finds nothing to
//! repaint and the device is free to sleep between samples. Motion costs power, so only
//! the states an operator needs to notice are allowed to move.
//!
//! The pairing carries meaning, too. A creature **asleep** is a sampler that stopped; a
//! creature **startled** is a probe that is lying. Those are the two failures a bare
//! `Option<Measurement>` could not tell apart — the distinction that
//! [`Observation`] exists to preserve, now visible from across a room.

use plant_core::Observation;

use crate::sprite::{Sprite, EXPRESSION_SLEEP, EXPRESSION_SURPRISE, IDLE_BREATHE, WORK_THINK};

/// Whether the creature for a scene moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    /// One frame, held forever. The render loop repaints only when the observation
    /// changes, and the CPU may sleep between samples.
    Still,
    /// The creature animates on its own clock, so the panel repaints as frames advance.
    Animated,
}

/// The creature shown for an observation, and whether it moves.
#[derive(Clone, Copy, Debug)]
pub struct Scene {
    /// The creature.
    pub sprite: &'static Sprite,
    /// Whether it animates.
    pub motion: Motion,
}

/// The scene for an observation.
pub const fn scene(observation: Observation) -> Scene {
    match observation {
        // Still: a healthy device must not keep the CPU awake to breathe.
        Observation::Fresh(_) => Scene {
            sprite: &IDLE_BREATHE,
            motion: Motion::Still,
        },
        // Startled: the sampler is alive and the probe is lying.
        Observation::Faulted(_) => Scene {
            sprite: &EXPRESSION_SURPRISE,
            motion: Motion::Animated,
        },
        // Asleep: nothing has been sampled recently — the writer stopped.
        Observation::Stale => Scene {
            sprite: &EXPRESSION_SLEEP,
            motion: Motion::Animated,
        },
        // Thinking: the first cycle has not finished yet.
        Observation::NeverSampled => Scene {
            sprite: &WORK_THINK,
            motion: Motion::Animated,
        },
    }
}

/// Whether the observation's creature moves.
///
/// The render loop reads this to choose its own cadence: there is no reason to wake 20
/// times a second to ask whether a motionless creature has changed.
pub fn is_animated(observation: Observation) -> bool {
    matches!(scene(observation).motion, Motion::Animated)
}

/// Which frame of the observation's creature shows after `elapsed_ms` in this state.
///
/// A [`Still`](Motion::Still) scene is always frame 0, whatever the clock says. That one
/// line is the whole power policy: the render loop repaints iff the pair
/// `(observation, frame_index)` changed, so a healthy monitor produces an unchanging pair
/// and never repaints at all.
pub fn frame_index(observation: Observation, elapsed_ms: u64) -> usize {
    let scene: Scene = scene(observation);
    match scene.motion {
        Motion::Still => 0,
        Motion::Animated => scene.sprite.frame_index_at(elapsed_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plant_core::{Measurement, Moisture, ProbeFault};

    const FRESH: Observation = Observation::Fresh(Measurement::new(
        2048,
        match Moisture::new(50) {
            Some(m) => m,
            None => unreachable!(),
        },
    ));
    const FAULTED: Observation = Observation::Faulted(ProbeFault::OverRange);

    /// The load-bearing rule: a healthy device shows a frame that never advances, so the
    /// render loop finds nothing to do and the CPU may sleep. If this regresses, the
    /// deep-sleep duty cycle dies quietly and the battery goes with it.
    #[test]
    fn a_fresh_observation_never_advances_its_frame() {
        assert_eq!(frame_index(FRESH, 0), 0);
        assert_eq!(frame_index(FRESH, 5_000), 0);
        assert_eq!(frame_index(FRESH, u64::MAX), 0);
    }

    #[test]
    fn a_fresh_observation_is_still() {
        assert_eq!(scene(FRESH).motion, Motion::Still);
    }

    /// Zero: at the start of any unhealthy state, frame 0.
    #[test]
    fn an_animated_scene_starts_at_its_first_frame() {
        assert_eq!(frame_index(Observation::Stale, 0), 0);
        assert_eq!(frame_index(FAULTED, 0), 0);
        assert_eq!(frame_index(Observation::NeverSampled, 0), 0);
    }

    /// One, and many: an unhealthy state does advance, and eventually wraps. Asserted by
    /// finding a later frame, not by trusting the `Motion` tag.
    #[test]
    fn a_stale_observation_advances_its_frame_over_time() {
        let sprite: &Sprite = scene(Observation::Stale).sprite;
        let first_hold: u64 = u64::from(sprite.frames()[0].hold_ms());
        assert_eq!(frame_index(Observation::Stale, first_hold), 1);
    }

    #[test]
    fn a_faulted_observation_advances_its_frame_over_time() {
        let sprite: &Sprite = scene(FAULTED).sprite;
        let first_hold: u64 = u64::from(sprite.frames()[0].hold_ms());
        assert_eq!(frame_index(FAULTED, first_hold), 1);
    }

    #[test]
    fn an_animated_scene_wraps_at_the_end_of_its_loop() {
        let sprite: &Sprite = scene(Observation::NeverSampled).sprite;
        let loop_ms: u64 = u64::from(sprite.loop_ms());
        assert_eq!(frame_index(Observation::NeverSampled, loop_ms), 0);
    }

    /// Every unhealthy state gets a *different* creature. If two collided, an operator
    /// could not tell a dead sampler from a lying probe by looking — which is exactly the
    /// information `Observation` exists to carry.
    #[test]
    fn each_observation_shows_a_distinct_creature() {
        let fresh: &str = scene(FRESH).sprite.slug();
        let faulted: &str = scene(FAULTED).sprite.slug();
        let stale: &str = scene(Observation::Stale).sprite.slug();
        let never: &str = scene(Observation::NeverSampled).sprite.slug();

        assert_ne!(fresh, faulted);
        assert_ne!(fresh, stale);
        assert_ne!(fresh, never);
        assert_ne!(faulted, stale);
        assert_ne!(faulted, never);
        assert_ne!(stale, never);
    }

    /// Only the healthy state is still. Every state an operator must notice moves.
    #[test]
    fn every_unhealthy_state_animates() {
        assert_eq!(scene(FAULTED).motion, Motion::Animated);
        assert_eq!(scene(Observation::Stale).motion, Motion::Animated);
        assert_eq!(scene(Observation::NeverSampled).motion, Motion::Animated);
    }

    /// The fault *reason* does not change the creature — a startled creature says "the
    /// probe is lying", and the text row beside it says which way. Pinned so a future
    /// per-fault creature is a deliberate change, not an accident.
    #[test]
    fn every_fault_shows_the_same_startled_creature() {
        let over: &str = scene(Observation::Faulted(ProbeFault::OverRange))
            .sprite
            .slug();
        let under: &str = scene(Observation::Faulted(ProbeFault::UnderRange))
            .sprite
            .slug();
        let unreadable: &str = scene(Observation::Faulted(ProbeFault::Unreadable))
            .sprite
            .slug();
        assert_eq!(over, under);
        assert_eq!(under, unreadable);
    }
}
