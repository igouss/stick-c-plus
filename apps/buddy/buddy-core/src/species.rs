//! The selected species — a plain domain value, and its NVS persistence key.
//!
//! The domain owns *which* creature is chosen, as a bare index in `0..N`. It does **not** own
//! the art: the `(species, state) -> &Sprite` binding is a display-layer policy, and the
//! domain never references a sprite. With one creature today the index cycles a set of one;
//! the species-art bead grows `N`.

/// The NVS key under which the selected species index is persisted.
pub const SPECIES_NVS_KEY: &str = "species";

/// The sentinel index meaning "use the installed GIF pet instead of a built-in species".
pub const GIF_SENTINEL: u8 = 0xFF;

/// The selected creature, as an index into the species registry (`0..N`).
///
/// A domain value object, nothing more: the display layer maps it to art. The registry order
/// is the persisted NVS value, so the numeric index is a stable contract.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpeciesIndex(u8);

impl SpeciesIndex {
    /// Wrap a raw registry index.
    pub const fn new(index: u8) -> Self {
        SpeciesIndex(index)
    }

    /// The raw registry index.
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Advance to the next species, wrapping within `0..count`.
    ///
    /// A `count` of `0` is a degenerate registry with nothing to cycle to, so the index is
    /// returned unchanged. The `+ 1` is widened so the wrap never overflows `u8`, even from
    /// [`GIF_SENTINEL`].
    pub fn cycle(self, count: u8) -> Self {
        if count == 0 {
            return self;
        }
        let next: u8 = ((u16::from(self.0) + 1) % u16::from(count)) as u8;
        SpeciesIndex(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// A degenerate registry of zero has nothing to cycle to.
    #[test]
    fn cycling_an_empty_registry_is_a_no_op() {
        let index: SpeciesIndex = SpeciesIndex::new(0);
        assert_eq!(index.cycle(0).get(), 0);
    }

    /// A registry of one always wraps back to itself.
    #[test]
    fn a_registry_of_one_wraps_to_itself() {
        let index: SpeciesIndex = SpeciesIndex::new(0);
        assert_eq!(index.cycle(1).get(), 0);
    }

    /// A registry of many advances then wraps at the end.
    #[test]
    fn cycling_advances_then_wraps() {
        assert_eq!(SpeciesIndex::new(0).cycle(3).get(), 1);
        assert_eq!(SpeciesIndex::new(2).cycle(3).get(), 0);
    }

    /// The GIF sentinel cycles without overflowing `u8`.
    #[test]
    fn the_sentinel_cycles_without_overflow() {
        // (255 + 1) % 3 == 1 — the point is it wraps into range without overflowing u8.
        let cycled: u8 = SpeciesIndex::new(GIF_SENTINEL).cycle(3).get();
        assert_eq!(cycled, 1);
        assert!(cycled < 3);
    }

    proptest! {
        /// A cycled index is always a valid slot in a non-empty registry.
        #[test]
        fn a_cycled_index_is_always_in_range(start in 0u8..=255, count in 1u8..=255) {
            let cycled: u8 = SpeciesIndex::new(start).cycle(count).get();
            prop_assert!(cycled < count);
        }
    }
}
