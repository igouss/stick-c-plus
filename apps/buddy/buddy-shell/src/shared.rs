//! The device state, shared between the threads that write it and the render loop that reads it.
//!
//! Three writers — the input thread (buttons), the BLE receive callback (inbound lines), and the
//! sensor tick — and one reader, the display loop's source closure. One mutex, and every access
//! is poison-tolerant: a panic in one holder must not take the glass down with it, because a
//! frozen picture is a worse failure on a desk pet than a stale one.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use buddy_display::BuddyView;
use platform_core::Tick;

use crate::state::DeviceState;

/// A cloneable handle on the one [`DeviceState`].
#[derive(Clone)]
pub struct SharedDevice {
    inner: Arc<Mutex<DeviceState>>,
}

impl SharedDevice {
    /// Wrap `state` for sharing.
    pub fn new(state: DeviceState) -> Self {
        SharedDevice {
            inner: Arc::new(Mutex::new(state)),
        }
    }

    /// Run `act` against the state.
    ///
    /// Poison-tolerant: a thread that panicked while holding the lock has left the state
    /// *consistent* — every mutation here is a whole method — so the guard is taken anyway
    /// rather than propagating the panic into every other thread.
    pub fn with<R>(&self, act: impl FnOnce(&mut DeviceState) -> R) -> R {
        let mut guard: MutexGuard<'_, DeviceState> =
            self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        act(&mut guard)
    }

    /// The picture at the state's last known tick — the display loop's source.
    pub fn view(&self) -> BuddyView {
        self.with(|state: &mut DeviceState| state.view())
    }

    /// Advance the state's clock and sensors, then read the picture. One lock, not two: a
    /// separate tick and read would let another writer land between them and paint a view of a
    /// state that never existed.
    pub fn tick_and_view(
        &self,
        now: Tick,
        accel: platform_core::Acceleration,
        screen_on: bool,
    ) -> BuddyView {
        self.with(|state: &mut DeviceState| {
            state.tick(now, accel, screen_on);
            state.view()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buddy_core::SpeciesIndex;
    use platform_core::Acceleration;

    use crate::identity::Identity;

    const REST: Acceleration = Acceleration::new(0, 0, 1_000);

    fn shared() -> SharedDevice {
        SharedDevice::new(DeviceState::new(
            Identity::new("Claude-4F2A", "0.1.0", "A0:B7:65:4F:2A:11"),
            SpeciesIndex::new(0),
        ))
    }

    /// One: a handle reads the state it wraps.
    #[test]
    fn a_handle_reads_the_state() {
        assert!(!shared().view().device.linked);
    }

    /// Many: two handles are one state — a write through either is seen through the other.
    #[test]
    fn two_handles_share_one_state() {
        let first: SharedDevice = shared();
        let second: SharedDevice = first.clone();
        first.with(|state: &mut DeviceState| state.show_passkey(Some(482_913)));
        assert_eq!(second.view().passkey, Some(482_913));
    }

    /// A poisoned lock still hands out the state: a panic in one thread must not freeze the
    /// glass, which is a worse failure on a desk pet than a stale picture.
    #[test]
    fn a_poisoned_lock_still_serves_the_glass() {
        let shared: SharedDevice = shared();
        let poisoner: SharedDevice = shared.clone();
        let panicked = std::thread::spawn(move || {
            poisoner.with(|_: &mut DeviceState| panic!("a writer fell over"));
        })
        .join();
        assert!(
            panicked.is_err(),
            "the test's own writer must have panicked"
        );
        assert!(!shared
            .tick_and_view(1_000, REST, true)
            .device
            .name
            .is_empty());
    }
}
