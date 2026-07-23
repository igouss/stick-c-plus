//! The species registry: the ordered list, and total resolution of a persisted index.
//!
//! [`REGISTRY`] is the ORDERED species list; index 0 is [`CLAUDEPIX`]. buddy-core persists the
//! selected index to NVS and puts it on the wire, so the order is a stable **compatibility
//! surface**, not a cosmetic choice — appending is safe, reordering is a breaking change.
//!
//! [`resolve`] is TOTAL: an in-range index yields `Some`, and both an out-of-range index and
//! the [`GIF_SENTINEL`](buddy_core::GIF_SENTINEL) (`0xFF`) yield `None` — the display layer then
//! falls back to the installed GIF pet. It never panics.

use buddy_core::{SpeciesIndex, GIF_SENTINEL};

use crate::claudepix::CLAUDEPIX;
use crate::species::Species;

/// Every built-in creature, in persisted-index order. Index 0 = [`CLAUDEPIX`].
pub static REGISTRY: &[&Species] = &[&CLAUDEPIX];

/// Resolve a persisted [`SpeciesIndex`] to its creature, or `None` for an out-of-range index or
/// the GIF sentinel. Total and panic-free.
///
/// The sentinel is rejected explicitly, ahead of the range check, so the "sentinel means the
/// installed GIF, never a built-in" contract holds independently of how many creatures the
/// registry ever grows to; every other out-of-range index falls out of the total
/// [`slice::get`], which yields `None` rather than panicking.
pub fn resolve(index: SpeciesIndex) -> Option<&'static Species> {
    let raw: u8 = index.get();
    if raw == GIF_SENTINEL {
        return None;
    }
    REGISTRY.get(raw as usize).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::species::STATE_COUNT;
    use buddy_core::{PersonaState, GIF_SENTINEL};
    use proptest::prelude::*;

    /// The seven states in ordinal order — the table the totality property walks.
    const STATES: [PersonaState; STATE_COUNT] = [
        PersonaState::Sleep,
        PersonaState::Idle,
        PersonaState::Busy,
        PersonaState::Attention,
        PersonaState::Celebrate,
        PersonaState::Dizzy,
        PersonaState::Heart,
    ];

    /// Invariant 3: the order is a compatibility surface — index 0 is claudepix and the length
    /// is asserted, so an accidental reorder or insertion fails here rather than silently
    /// shifting the persisted NVS/wire index.
    #[test]
    fn registry_index_zero_is_claudepix_and_length_is_stable() {
        assert_eq!(REGISTRY.len(), 1);
        assert_eq!(REGISTRY[0].name(), "claudepix");
    }

    proptest! {
        /// Invariant 4 (TOTALITY over the whole `u8` index space): `resolve` is `Some` exactly
        /// for an in-range, non-sentinel index and `None` otherwise, and never panics.
        #[test]
        fn resolve_is_some_exactly_for_in_range_indices(raw in 0u8..=255) {
            let got: Option<&'static Species> = resolve(SpeciesIndex::new(raw));
            let in_range: bool = raw != GIF_SENTINEL && (raw as usize) < REGISTRY.len();
            prop_assert_eq!(got.is_some(), in_range);
        }

        /// Invariant 1 (TOTALITY over species × all seven states): every species in the registry
        /// answers every persona state with a non-empty sprite set.
        #[test]
        fn every_species_answers_every_state_with_a_non_empty_set(
            species_idx in 0usize..REGISTRY.len(),
            state_idx in 0usize..STATE_COUNT,
        ) {
            let species: &Species = REGISTRY[species_idx];
            let state: PersonaState = STATES[state_idx];
            prop_assert!(!species.sprites(state).is_empty());
        }
    }
}
