//! [`Sketch`] — the gallery's running order, as an identity rather than a renderer.
//!
//! A `Sketch` names which generative piece is on the glass. It carries no pixels and no state:
//! the display crate turns a `Sketch` plus a phase into a frame, and the [`Selector`] turns a
//! button press into the next `Sketch`. Keeping the order in the domain makes it pure,
//! exhaustively matched by the display, and host-tested — so a missing piece or a broken wrap is
//! a failing test here, not a surprise on the glass.
//!
//! [`Selector`]: crate::Selector

/// One piece in the gallery, in running order.
///
/// The variants *are* the order: [`ALL`](Sketch::ALL) lists them once, and everything else —
/// the display's exhaustive match, the [`Selector`](crate::Selector)'s wrap — is derived from
/// that one list, so the order cannot disagree with itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Sketch {
    /// The feathered frond ported from its *Dwitter* — the gallery's first piece, and the one
    /// the [`Selector`](crate::Selector) starts on.
    Plume,
    /// A grid of breathing, sign-pulsing nested squares.
    Squares,
    /// HSB radial folding quads, hue by distance from the centre.
    Fan,
    /// An `acos`/`cos` distance-field bloom over a static noise texture.
    Orbits,
    /// An original frond — designed for this pipeline from the first line, not ported.
    Willow,
}

impl Sketch {
    /// Every sketch, once, in gallery order. The single source of the running order: the
    /// [`Selector`](crate::Selector) cycles this and the display matches on it, so neither can
    /// hold an order that disagrees with the other.
    pub const ALL: [Sketch; 5] = [
        Sketch::Plume,
        Sketch::Squares,
        Sketch::Fan,
        Sketch::Orbits,
        Sketch::Willow,
    ];

    /// This sketch's position in the running order.
    ///
    /// Total by construction: every `Sketch` is in [`ALL`](Sketch::ALL) because `ALL` lists the
    /// whole enum, which the round-trip test below pins. The `expect` documents that invariant
    /// rather than guarding a real failure.
    pub fn ordinal(self) -> usize {
        Self::ALL
            .iter()
            .position(|&s: &Sketch| s == self)
            .expect("every Sketch is listed in Sketch::ALL")
    }

    /// The next sketch in the running order, wrapping after the last back to the first.
    ///
    /// Derived from [`ALL`](Sketch::ALL) alone, so the gallery's cycle is exactly its declared
    /// order — advance never has an opinion the list does not.
    pub fn next(self) -> Sketch {
        Self::ALL[(self.ordinal() + 1) % Self::ALL.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ALL` lists every variant exactly once — the invariant [`Sketch::ordinal`] and the
    /// display's exhaustive match both rest on. The witness array below is compiler-checked
    /// exhaustive, so a new variant left out of `ALL` fails here before `ordinal` can panic on
    /// it at runtime.
    #[test]
    fn all_lists_every_variant_exactly_once() {
        let every_variant: [Sketch; 5] = [
            Sketch::Plume,
            Sketch::Squares,
            Sketch::Fan,
            Sketch::Orbits,
            Sketch::Willow,
        ];
        // The witness above must itself cover the enum: adding a variant makes this match
        // non-exhaustive and the test stops compiling until the witness (and `ALL`) are updated.
        fn is_exhaustive(sketch: Sketch) {
            match sketch {
                Sketch::Plume | Sketch::Squares | Sketch::Fan | Sketch::Orbits | Sketch::Willow => {
                }
            }
        }
        every_variant.into_iter().for_each(is_exhaustive);

        for variant in every_variant {
            let count: usize = Sketch::ALL
                .iter()
                .filter(|&&s: &&Sketch| s == variant)
                .count();
            assert_eq!(count, 1, "{variant:?} must appear exactly once in ALL");
        }
        assert_eq!(Sketch::ALL.len(), every_variant.len());
    }

    /// Zero offset: a sketch's ordinal round-trips through [`Sketch::ALL`] back to itself, so the
    /// two directions of the order — name to index, index to name — agree.
    #[test]
    fn ordinal_round_trips_through_all() {
        for &sketch in &Sketch::ALL {
            assert_eq!(Sketch::ALL[sketch.ordinal()], sketch);
        }
    }

    /// One step: from the first piece, `next` is the second.
    #[test]
    fn next_advances_one_place() {
        assert_eq!(Sketch::Plume.next(), Sketch::Squares);
    }

    /// The last wraps to the first — the property the gallery is built on.
    #[test]
    fn the_last_sketch_wraps_to_the_first() {
        assert_eq!(Sketch::Willow.next(), Sketch::Plume);
    }

    /// Many: `next` applied `ALL.len()` times is the identity, visiting every sketch exactly
    /// once on the way. A cycle that skipped or repeated a piece would trip one assertion.
    #[test]
    fn cycling_the_whole_order_returns_to_the_start() {
        let mut sketch: Sketch = Sketch::Plume;
        let mut visits: [u8; Sketch::ALL.len()] = [0; Sketch::ALL.len()];
        for _ in 0..Sketch::ALL.len() {
            visits[sketch.ordinal()] += 1;
            sketch = sketch.next();
        }
        assert_eq!(
            sketch,
            Sketch::Plume,
            "the cycle did not return to the start"
        );
        assert!(
            visits.iter().all(|&n: &u8| n == 1),
            "every sketch is visited exactly once per cycle, got {visits:?}"
        );
    }
}
