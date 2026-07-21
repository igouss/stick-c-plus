//! The identities silenced since the board was last fully healthy.

use crate::fault::FaultKey;

/// How many distinct acknowledged fault identities can be remembered at once.
///
/// `no_std`, no allocation: the set is a fixed-capacity FIFO. Filling it needs eight distinct
/// acknowledged faults with no healthy moment in between — eight separate button presses on a
/// board that never recovers — and overflow costs one extra chirp for the evicted identity,
/// never a silence. That asymmetry is deliberate (R17b): a false alarm is the lesser harm.
pub const ACK_CAPACITY: usize = 8;

/// A bounded, order-preserving set of acknowledged [`FaultKey`]s.
///
/// This is the memory that makes an acknowledgement survive being outranked: the alarm's three
/// spec-visible states describe the relationship to the *announced* fault only, so a single
/// stored key would forget an earlier acknowledgement the moment a higher-ranked fault masked
/// it. Carrying every acknowledged identity, not just the most recent one, is what makes R17
/// hold unconditionally.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AckSet {
    slots: [Option<FaultKey>; ACK_CAPACITY],
}

impl AckSet {
    /// A set with nothing acknowledged — the state the board starts in and returns to on
    /// recovery.
    pub fn empty() -> Self {
        AckSet {
            slots: [None; ACK_CAPACITY],
        }
    }

    /// Whether `key` has been acknowledged since the set was last cleared.
    pub fn contains(&self, key: FaultKey) -> bool {
        self.slots.contains(&Some(key))
    }

    /// `key`, acknowledged. Idempotent: acknowledging an already-present key changes nothing.
    /// When full, evicts the oldest entry to make room — the FIFO discipline that bounds
    /// memory without ever silently dropping the *newest* acknowledgement.
    pub fn insert(self, key: FaultKey) -> AckSet {
        if self.contains(key) {
            return self;
        }
        let mut slots: [Option<FaultKey>; ACK_CAPACITY] = self.slots;
        let free: Option<usize> = slots
            .iter()
            .position(|slot: &Option<FaultKey>| slot.is_none());
        match free {
            // Entries fill left to right, so the oldest acknowledgement is always at index 0.
            Some(index) => slots[index] = Some(key),
            // Full: shift everything down one, dropping the oldest, and land the new key last.
            None => {
                slots.rotate_left(1);
                slots[ACK_CAPACITY - 1] = Some(key);
            }
        }
        AckSet { slots }
    }

    /// Every acknowledgement forgotten — what recovery to `Healthy` does, so a fault that
    /// clears and later returns sounds again instead of staying muted for the life of the
    /// boot.
    pub fn clear(self) -> AckSet {
        AckSet::empty()
    }
}

impl Default for AckSet {
    fn default() -> Self {
        AckSet::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fault::FaultKind;

    fn key(component: &'static str) -> FaultKey {
        FaultKey {
            kind: FaultKind::Stalled,
            component: Some(Component(component)),
        }
    }

    use crate::Component;

    // --- Zero / one / many over the set itself. ---

    /// Zero — a fresh set has acknowledged nothing.
    #[test]
    fn an_empty_set_contains_nothing() {
        assert!(!AckSet::empty().contains(key("display")));
    }

    /// One — after acknowledging one key, that key is present and an unrelated one is not.
    #[test]
    fn a_single_insert_is_found_and_nothing_else_is() {
        let acked: AckSet = AckSet::empty().insert(key("display"));
        assert!(acked.contains(key("display")));
        assert!(!acked.contains(key("sensors")));
    }

    /// Many (two) — acknowledging a second key does not evict the first.
    #[test]
    fn two_inserts_both_remain() {
        let acked: AckSet = AckSet::empty()
            .insert(key("display"))
            .insert(key("sensors"));
        assert!(acked.contains(key("display")));
        assert!(acked.contains(key("sensors")));
    }

    /// Inserting the same key twice does not grow the set, and the key stays acknowledged —
    /// `insert` is idempotent.
    #[test]
    fn inserting_the_same_key_twice_is_idempotent() {
        let acked: AckSet = AckSet::empty()
            .insert(key("display"))
            .insert(key("display"));
        assert!(acked.contains(key("display")));
    }

    /// R17b — filling the set past `ACK_CAPACITY` with distinct keys evicts the oldest one,
    /// never the newest: the degradation is one extra chirp for the evicted identity, not a
    /// silence swallowing the fault the operator just acknowledged.
    #[test]
    fn overflow_evicts_the_oldest_not_the_newest() {
        let names: [&'static str; ACK_CAPACITY + 1] = ["a", "b", "c", "d", "e", "f", "g", "h", "i"];
        let full: AckSet = names
            .iter()
            .fold(AckSet::empty(), |set: AckSet, name: &&'static str| {
                set.insert(key(name))
            });
        assert!(!full.contains(key("a")));
        assert!(full.contains(key("i")));
    }

    /// Clearing an acknowledged set forgets everything in it — what recovery to `Healthy` does.
    #[test]
    fn clear_forgets_every_acknowledgement() {
        let acked: AckSet = AckSet::empty().insert(key("display"));
        assert!(!acked.clear().contains(key("display")));
    }
}
