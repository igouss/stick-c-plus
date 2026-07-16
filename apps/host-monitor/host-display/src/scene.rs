//! Which creature the glass shows for a host's load, and whether it moves.
//!
//! The whole animation policy, as pure functions of the host [`Status`] and elapsed
//! milliseconds. No clock, no history — the render loop supplies both and this module
//! answers, so every rule below is decided on the host.
//!
//! ## The creature *is* the load
//!
//! | Band | Meaning | Creature | Motion |
//! |---|---|---|---|
//! | [`Calm`](LoadBand::Calm) | load below [`BUSY_AT`] | breathing | **still** |
//! | [`Busy`](LoadBand::Busy) | working, not stressed | coding | **still** |
//! | [`Pegged`](LoadBand::Pegged) | load at [`PEGGED_AT`] or above | frantic dance | animated |
//! | [`Faulted`](LoadBand::Faulted) | the host did not answer | startled | animated |
//! | [`Stale`](LoadBand::Stale) | the poller stopped | asleep | animated |
//! | [`NeverSampled`](LoadBand::NeverSampled) | no first reading yet | thinking | animated |
//!
//! A calm or merely busy host shows a *motionless* creature. That is not an aesthetic
//! choice: a still scene means [`frame_index`] is constant, so the render loop finds
//! nothing to repaint and the device is free to rest between samples. Motion costs
//! power (and, with two sparklines to redraw, SPI bandwidth), so only the states an
//! operator needs to notice are allowed to move — a pegged host, or a fault.
//!
//! The pairing carries meaning, too. A creature **asleep** is a poller that stopped; a
//! creature **startled** is a host that did not answer. Those are the two failures a
//! bare `Option<Sample>` could not tell apart — the distinction
//! [`Status`](host_core::Status) exists to preserve, now visible from across a room.

use host_core::{Sample, Status};
use platform_display::sprite::{
    Sprite, DANCE_DJMIX, EXPRESSION_SLEEP, EXPRESSION_SURPRISE, IDLE_BREATHE, WORK_CODING,
    WORK_THINK,
};

/// The load (higher of CPU and memory percent) at or above which the host is **busy**
/// rather than calm — the creature starts working.
pub const BUSY_AT: u8 = 50;

/// The load at or above which the host is **pegged** — the creature goes frantic and
/// the percentage is drawn in red.
pub const PEGGED_AT: u8 = 85;

/// A coarse projection of the host's [`Status`] into the animation's anchor.
///
/// This is the [`Anchor`](platform_core::Animated::anchor) the render loop resets the
/// creature's clock on — deliberately *excluding* the sample-by-sample history, so the
/// creature keeps animating on the band's clock while the graph scrolls beneath it, and
/// restarts only when the host crosses a load threshold or changes status.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadBand {
    /// Load below [`BUSY_AT`] — the host is idling.
    Calm,
    /// Load in `BUSY_AT..PEGGED_AT` — working, but not stressed.
    Busy,
    /// Load at [`PEGGED_AT`] or above — the host is pegged.
    Pegged,
    /// The host did not answer the last scrape.
    Faulted,
    /// No fresh scrape within the staleness bound — the poller stopped.
    Stale,
    /// Nothing usable has been sampled yet.
    NeverSampled,
}

/// The load band for a host status.
///
/// A fresh status is banded by its load — the *higher* of CPU and memory percent, so a
/// pegged CPU or an exhausted memory both raise the alarm. Every non-fresh status maps
/// to its own band, so the creature tells apart an unreachable host, a dead poller, and
/// a warming device.
pub fn band(status: Status) -> LoadBand {
    match status {
        Status::Fresh(sample) => load_band(sample),
        Status::Faulted(_) => LoadBand::Faulted,
        Status::Stale => LoadBand::Stale,
        Status::NeverSampled => LoadBand::NeverSampled,
    }
}

/// The band of a fresh sample, by the higher of its two percentages.
fn load_band(sample: Sample) -> LoadBand {
    let load: u8 = if sample.cpu().value() >= sample.mem().value() {
        sample.cpu().value()
    } else {
        sample.mem().value()
    };
    if load >= PEGGED_AT {
        LoadBand::Pegged
    } else if load >= BUSY_AT {
        LoadBand::Busy
    } else {
        LoadBand::Calm
    }
}

/// Whether the creature for a scene moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    /// One frame, held forever. The render loop repaints only when the picture changes,
    /// and the CPU may rest between samples.
    Still,
    /// The creature animates on its own clock, so the panel repaints as frames advance.
    Animated,
}

/// The creature shown for a load band, and whether it moves.
#[derive(Clone, Copy, Debug)]
pub struct Scene {
    /// The creature.
    pub sprite: &'static Sprite,
    /// Whether it animates.
    pub motion: Motion,
}

/// The scene for a load band.
pub const fn scene(band: LoadBand) -> Scene {
    match band {
        // Still: a calm host must not keep the CPU awake to breathe.
        LoadBand::Calm => Scene {
            sprite: &IDLE_BREATHE,
            motion: Motion::Still,
        },
        // Still: working, but nothing an operator must watch — the host is fine.
        LoadBand::Busy => Scene {
            sprite: &WORK_CODING,
            motion: Motion::Still,
        },
        // Frantic: the host is pegged — the one healthy state worth glancing up for.
        LoadBand::Pegged => Scene {
            sprite: &DANCE_DJMIX,
            motion: Motion::Animated,
        },
        // Startled: the poller is alive and the host did not answer.
        LoadBand::Faulted => Scene {
            sprite: &EXPRESSION_SURPRISE,
            motion: Motion::Animated,
        },
        // Asleep: nothing has been scraped recently — the poller stopped.
        LoadBand::Stale => Scene {
            sprite: &EXPRESSION_SLEEP,
            motion: Motion::Animated,
        },
        // Thinking: the first CPU interval has not completed yet.
        LoadBand::NeverSampled => Scene {
            sprite: &WORK_THINK,
            motion: Motion::Animated,
        },
    }
}

/// Whether the band's creature moves.
///
/// The render loop reads this to choose its cadence: there is no reason to wake 20
/// times a second to ask whether a motionless creature has changed.
pub fn is_animated(band: LoadBand) -> bool {
    matches!(scene(band).motion, Motion::Animated)
}

/// Which frame of the band's creature shows after `elapsed_ms` in this band.
///
/// A [`Still`](Motion::Still) scene is always frame 0, whatever the clock says. That one
/// line is the whole power policy: the render loop repaints iff the pair
/// `(state, frame_index)` changed, so a calm host's creature produces an unchanging
/// frame even as the graph beneath it scrolls on the state's own account.
pub fn frame_index(band: LoadBand, elapsed_ms: u64) -> usize {
    let scene: Scene = scene(band);
    match scene.motion {
        Motion::Still => 0,
        Motion::Animated => scene.sprite.frame_index_at(elapsed_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_core::{HostFault, Percent};

    /// A fresh status whose load (higher of cpu/mem) is `load`.
    fn fresh(load: u8) -> Status {
        Status::Fresh(Sample::new(
            Percent::new(load).expect("0..=100"),
            Percent::ZERO,
        ))
    }

    #[test]
    fn load_below_busy_is_calm_and_still() {
        assert_eq!(band(fresh(BUSY_AT - 1)), LoadBand::Calm);
        assert_eq!(scene(LoadBand::Calm).motion, Motion::Still);
    }

    #[test]
    fn load_at_busy_is_busy_and_still() {
        assert_eq!(band(fresh(BUSY_AT)), LoadBand::Busy);
        assert_eq!(scene(LoadBand::Busy).motion, Motion::Still);
    }

    #[test]
    fn load_at_pegged_is_pegged_and_animated() {
        assert_eq!(band(fresh(PEGGED_AT)), LoadBand::Pegged);
        assert_eq!(scene(LoadBand::Pegged).motion, Motion::Animated);
    }

    #[test]
    fn the_band_takes_the_higher_of_cpu_and_memory() {
        // A calm CPU with a pegged memory is still pegged — either axis raises the alarm.
        let status: Status = Status::Fresh(Sample::new(Percent::ZERO, Percent::new(90).unwrap()));
        assert_eq!(band(status), LoadBand::Pegged);
    }

    #[test]
    fn each_non_fresh_status_maps_to_its_own_band() {
        assert_eq!(
            band(Status::Faulted(HostFault::Unreachable)),
            LoadBand::Faulted
        );
        assert_eq!(band(Status::Stale), LoadBand::Stale);
        assert_eq!(band(Status::NeverSampled), LoadBand::NeverSampled);
    }

    /// The fault *kind* does not change the band — an unreachable and a malformed host
    /// are both `Faulted` (a startled creature); the graph and label carry the detail.
    #[test]
    fn both_faults_share_the_faulted_band() {
        assert_eq!(
            band(Status::Faulted(HostFault::Unreachable)),
            LoadBand::Faulted
        );
        assert_eq!(
            band(Status::Faulted(HostFault::Malformed)),
            LoadBand::Faulted
        );
    }

    /// The load-bearing power rule: the two healthy bands never advance their frame, so
    /// the render loop rests. If this regresses, the device animates a busy host forever.
    #[test]
    fn the_calm_and_busy_bands_never_advance_their_frame() {
        for band in [LoadBand::Calm, LoadBand::Busy] {
            assert_eq!(frame_index(band, 0), 0);
            assert_eq!(frame_index(band, u64::MAX), 0);
            assert!(!is_animated(band));
        }
    }

    /// Zero: every animated band starts at frame 0.
    #[test]
    fn an_animated_band_starts_at_its_first_frame() {
        for band in [
            LoadBand::Pegged,
            LoadBand::Faulted,
            LoadBand::Stale,
            LoadBand::NeverSampled,
        ] {
            assert_eq!(frame_index(band, 0), 0);
            assert!(is_animated(band));
        }
    }

    /// One, and many: an animated band advances, and eventually wraps.
    #[test]
    fn an_animated_band_advances_and_wraps() {
        let sprite: &Sprite = scene(LoadBand::Pegged).sprite;
        let first_hold: u64 = u64::from(sprite.frames()[0].hold_ms());
        assert_eq!(frame_index(LoadBand::Pegged, first_hold), 1);
        let loop_ms: u64 = u64::from(sprite.loop_ms());
        assert_eq!(frame_index(LoadBand::Pegged, loop_ms), 0);
    }

    /// Every band shows a *distinct* creature, so a glance tells calm from busy from
    /// pegged, and an unreachable host from a dead poller from a warming device.
    #[test]
    fn each_band_shows_a_distinct_creature() {
        let bands: [LoadBand; 6] = [
            LoadBand::Calm,
            LoadBand::Busy,
            LoadBand::Pegged,
            LoadBand::Faulted,
            LoadBand::Stale,
            LoadBand::NeverSampled,
        ];
        for (i, a) in bands.iter().enumerate() {
            for b in &bands[i + 1..] {
                assert_ne!(
                    scene(*a).sprite.slug(),
                    scene(*b).sprite.slug(),
                    "{a:?} and {b:?} share a creature"
                );
            }
        }
    }
}
