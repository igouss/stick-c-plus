//! `SharedOrientation` — the pose shared between the sampler thread and the render loop.

use std::sync::{Arc, Mutex};

use orientation_core::Orientation;

/// The latest [`Orientation`], shared between the sampler thread (the one writer) and the
/// render loop (the reader).
///
/// Poison-tolerant: a panic in any holder recovers the inner value rather than propagating,
/// so one wedged thread cannot take the others down. The lock is held for exactly one copy in
/// and one copy out — an `Orientation` is a handful of words — so the render loop never waits
/// on the sampler even at the sampler's full rate.
#[derive(Clone)]
pub struct SharedOrientation {
    inner: Arc<Mutex<Orientation>>,
}

impl SharedOrientation {
    /// A cache holding the default orientation: nothing read yet, so no pose is named.
    pub fn new() -> Self {
        SharedOrientation {
            inner: Arc::new(Mutex::new(Orientation::default())),
        }
    }

    /// The current orientation, copied out — what the render loop's source reads each tick.
    pub fn snapshot(&self) -> Orientation {
        *self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Publish `orientation` as the current pose.
    pub fn publish(&self, orientation: Orientation) {
        let mut held = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *held = orientation;
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
    use orientation_core::Facing;
    use platform_core::{Acceleration, ONE_G_MG};

    /// The orientation of a board lying on its back.
    fn screen_up() -> Orientation {
        Orientation::of(Acceleration::new(0, 0, ONE_G_MG))
    }

    /// Zero: a fresh cache names no pose, rather than claiming the board is flat before
    /// anything has been read.
    #[test]
    fn a_fresh_cache_names_no_pose() {
        assert_eq!(SharedOrientation::new().snapshot().facing, Facing::Moving);
    }

    /// One: what was published is what is read back.
    #[test]
    fn a_published_orientation_is_what_the_reader_sees() {
        let shared: SharedOrientation = SharedOrientation::new();
        shared.publish(screen_up());
        assert_eq!(shared.snapshot(), screen_up());
    }

    /// Many: the newest publication wins — the readout shows now, not a moment ago.
    #[test]
    fn the_newest_publication_wins() {
        let shared: SharedOrientation = SharedOrientation::new();
        shared.publish(Orientation::of(Acceleration::new(0, ONE_G_MG, 0)));
        shared.publish(Orientation::of(Acceleration::new(-ONE_G_MG, 0, 0)));
        shared.publish(screen_up());
        assert_eq!(shared.snapshot().facing, Facing::ScreenUp);
    }

    /// The writer and the reader hold clones of one cache — the whole point of the type.
    #[test]
    fn a_clone_sees_the_same_orientation() {
        let writer: SharedOrientation = SharedOrientation::new();
        let reader: SharedOrientation = writer.clone();
        writer.publish(screen_up());
        assert_eq!(reader.snapshot().facing, Facing::ScreenUp);
    }

    /// A panic while the lock was held must not wedge the readout: the next reader recovers
    /// the value rather than propagating the poison.
    #[test]
    fn a_poisoned_lock_still_reads() {
        let shared: SharedOrientation = SharedOrientation::new();
        shared.publish(screen_up());

        let poisoner: SharedOrientation = shared.clone();
        let panicked = std::thread::spawn(move || {
            let _held = poisoner.inner.lock().expect("lock");
            panic!("poison the lock while holding it");
        })
        .join();
        assert!(panicked.is_err(), "the helper thread must have panicked");

        assert_eq!(
            shared.snapshot(),
            screen_up(),
            "a poisoned lock wedged the readout"
        );
    }
}
