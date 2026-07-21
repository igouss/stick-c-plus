//! `SharedOrientation` — the pose shared between the sampler thread and the render loop,
//! stamped with when it was last confirmed.

use std::sync::{Arc, Mutex};

use orientation_core::{Orientation, Reading};
use platform_core::Tick;

/// The latest [`Orientation`] and the time it was published, shared between the sampler thread
/// (the one writer) and the render loop (the reader).
///
/// The stamp is what makes a dead sensor visible. Without it the cache cannot tell a board
/// that has been still for a minute from one whose IMU stopped answering a minute ago — both
/// hand back the same well-formed pose — so the reader is given an *age* rather than a bare
/// value, and [`Reading`] turns that age into a verdict.
///
/// Poison-tolerant: a panic in any holder recovers the inner value rather than propagating,
/// so one wedged thread cannot take the others down. The lock is held for exactly one copy in
/// and one copy out — a pose and a `u64` are a handful of words — so the render loop never
/// waits on the sampler even at the sampler's full rate.
#[derive(Clone)]
pub struct SharedOrientation {
    inner: Arc<Mutex<Stamped>>,
}

/// The cache's contents: a pose and when it was last confirmed.
#[derive(Clone, Copy, Default)]
struct Stamped {
    orientation: Orientation,
    published_at: Tick,
}

impl SharedOrientation {
    /// A cache holding the default orientation, stamped at time zero: nothing read yet, so no
    /// pose is named and its age grows from boot until the first publication lands.
    pub fn new() -> Self {
        SharedOrientation {
            inner: Arc::new(Mutex::new(Stamped::default())),
        }
    }

    /// The current reading as of `now`: the last published pose, and whether it is still being
    /// confirmed. What the render loop's source calls each tick.
    ///
    /// `now` comes from the injected [`Clock`](platform_core::Clock), so the staleness rule is
    /// exercised by handing tests explicit ticks rather than by sleeping.
    pub fn reading(&self, now: Tick) -> Reading {
        let held: Stamped = *self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // `Clock` is monotonic and non-decreasing, so this cannot underflow — but a saturating
        // subtraction costs nothing and keeps a clock that ever misbehaved from wrapping an
        // age of a few milliseconds into half an eternity of apparent freshness.
        Reading::aged(held.orientation, now.saturating_sub(held.published_at))
    }

    /// The current pose alone, ignoring how long ago it was confirmed.
    ///
    /// For a caller that genuinely wants the last known value with no claim about its
    /// freshness. Anything that puts a pose in front of a person should use [`reading`] —
    /// that is the whole point of the stamp.
    ///
    /// [`reading`]: SharedOrientation::reading
    pub fn last_known(&self) -> Orientation {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .orientation
    }

    /// Publish `orientation` as the current pose, confirmed at `now`.
    pub fn publish(&self, orientation: Orientation, now: Tick) {
        let mut held = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *held = Stamped {
            orientation,
            published_at: now,
        };
    }
}

impl Default for SharedOrientation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orientation_core::{Facing, Signal, SIGNAL_TIMEOUT_MS};
    use platform_core::{Acceleration, ONE_G_MG};

    /// The orientation of a board lying on its back.
    fn screen_up() -> Orientation {
        Orientation::of(Acceleration::new(0, 0, ONE_G_MG))
    }

    /// Zero: a fresh cache names no pose, rather than claiming the board is flat before
    /// anything has been read.
    #[test]
    fn a_fresh_cache_names_no_pose() {
        assert_eq!(
            SharedOrientation::new().reading(0).orientation.facing,
            Facing::Moving
        );
    }

    /// One: what was published is what is read back.
    #[test]
    fn a_published_orientation_is_what_the_reader_sees() {
        let shared: SharedOrientation = SharedOrientation::new();
        shared.publish(screen_up(), 0);
        assert_eq!(shared.reading(0).orientation, screen_up());
    }

    /// Many: the newest publication wins — the readout shows now, not a moment ago.
    #[test]
    fn the_newest_publication_wins() {
        let shared: SharedOrientation = SharedOrientation::new();
        shared.publish(Orientation::of(Acceleration::new(0, ONE_G_MG, 0)), 10);
        shared.publish(Orientation::of(Acceleration::new(-ONE_G_MG, 0, 0)), 20);
        shared.publish(screen_up(), 30);
        assert_eq!(shared.reading(30).orientation.facing, Facing::ScreenUp);
    }

    /// The writer and the reader hold clones of one cache — the whole point of the type.
    #[test]
    fn a_clone_sees_the_same_orientation() {
        let writer: SharedOrientation = SharedOrientation::new();
        let reader: SharedOrientation = writer.clone();
        writer.publish(screen_up(), 0);
        assert_eq!(reader.reading(0).orientation.facing, Facing::ScreenUp);
    }

    /// A recently published pose is live — the ordinary case, and the one that must not
    /// accidentally read as a fault.
    #[test]
    fn a_recently_published_pose_is_live() {
        let shared: SharedOrientation = SharedOrientation::new();
        shared.publish(screen_up(), 1_000);
        assert_eq!(shared.reading(1_000).signal, Signal::Live);
        assert_eq!(
            shared.reading(1_000 + SIGNAL_TIMEOUT_MS - 1).signal,
            Signal::Live
        );
    }

    /// A pose nothing has refreshed past the timeout reports itself lost, while still
    /// carrying the pose it last knew.
    #[test]
    fn a_pose_nothing_refreshed_goes_lost_but_keeps_its_value() {
        let shared: SharedOrientation = SharedOrientation::new();
        shared.publish(screen_up(), 1_000);

        let stale: Reading = shared.reading(1_000 + SIGNAL_TIMEOUT_MS);
        assert_eq!(stale.signal, Signal::Lost);
        assert_eq!(stale.orientation.facing, Facing::ScreenUp);
    }

    /// A publication revives a lost signal: the sensor coming back is as visible as it going
    /// away, so a recovered readout does not stay marked dead.
    #[test]
    fn a_publication_revives_a_lost_signal() {
        let shared: SharedOrientation = SharedOrientation::new();
        shared.publish(screen_up(), 0);
        assert_eq!(shared.reading(SIGNAL_TIMEOUT_MS).signal, Signal::Lost);

        shared.publish(screen_up(), SIGNAL_TIMEOUT_MS);
        assert_eq!(shared.reading(SIGNAL_TIMEOUT_MS).signal, Signal::Live);
    }

    /// A cache nothing ever published goes lost on the same clock as a sensor that died —
    /// a board whose IMU never came up must not sit on a plausible blank readout forever.
    #[test]
    fn a_cache_that_was_never_published_into_goes_lost() {
        let shared: SharedOrientation = SharedOrientation::new();
        assert_eq!(shared.reading(0).signal, Signal::Live);
        assert_eq!(shared.reading(SIGNAL_TIMEOUT_MS).signal, Signal::Lost);
    }

    /// `last_known` hands back the pose with no freshness claim attached, whatever the age.
    #[test]
    fn last_known_ignores_the_age() {
        let shared: SharedOrientation = SharedOrientation::new();
        shared.publish(screen_up(), 0);
        assert_eq!(shared.last_known(), screen_up());
        assert_eq!(shared.reading(60_000).orientation, shared.last_known());
    }

    /// A panic while the lock was held must not wedge the readout: the next reader recovers
    /// the value rather than propagating the poison.
    #[test]
    fn a_poisoned_lock_still_reads() {
        let shared: SharedOrientation = SharedOrientation::new();
        shared.publish(screen_up(), 0);

        let poisoner: SharedOrientation = shared.clone();
        let panicked = std::thread::spawn(move || {
            let _held = poisoner.inner.lock().expect("lock");
            panic!("poison the lock while holding it");
        })
        .join();
        assert!(panicked.is_err(), "the helper thread must have panicked");

        assert_eq!(
            shared.reading(0).orientation,
            screen_up(),
            "a poisoned lock wedged the readout"
        );
    }
}
