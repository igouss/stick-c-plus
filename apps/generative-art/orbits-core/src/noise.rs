//! The static noise texture: a fixed grain the bloom is multiplied by, so the comet reads as a
//! spray of light rather than a smooth blob.
//!
//! The source multiplies each cell by `noise(x, y)` — p5's Perlin noise, sampled on the `10`-unit
//! grid. At that spacing p5's noise (whose features are about one unit across) is already
//! decorrelated cell-to-cell, so the texture is, in effect, one fixed random value per cell. This
//! reproduces that with a hash: a pure function of the cell's grid coordinates, deterministic and
//! the same every frame, computed on read rather than baked into a table — so the sketch adds no
//! heap on a board whose free store is already tight, and the "static" in "static noise" is a
//! property of the function, not of a buffer that has to be built and kept.

/// A fixed grain in `[0, 1]` for the grid cell at column `col`, row `row`.
///
/// A pure hash of the two coordinates — the same integer-mix (multiply, xor-shift) a
/// [`SplitMix`](https://prng.di.unimi.it/splitmix64.c)-style generator uses, run on a seed woven
/// from `col` and `row`. Deterministic (a cell's grain never changes), well spread (neighbouring
/// cells get unrelated values, the decorrelation the source samples out of Perlin noise), and free
/// of state — no seed to carry, no table to allocate.
pub fn noise(col: u16, row: u16) -> f32 {
    let mut hash: u32 = (col as u32)
        .wrapping_mul(0x0100_0193)
        .wrapping_add((row as u32).wrapping_mul(0x9e37_79b1))
        .wrapping_add(0x85eb_ca6b);
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x7feb_352d);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x846c_a68b);
    hash ^= hash >> 16;
    hash as f32 / u32::MAX as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic: the same cell hashes to the same grain every call — the texture is static, so
    /// a frame and the next frame see identical noise and only the bloom moves.
    #[test]
    fn a_cell_is_the_same_every_time() {
        assert_eq!(noise(7, 12), noise(7, 12));
    }

    /// In range: every grain is a valid brightness fraction in `[0, 1]`, so multiplying the bloom by
    /// it only ever dims, never brightens past full or goes negative.
    #[test]
    fn every_grain_is_a_fraction() {
        let corners: [(u16, u16); 4] = [(0, 0), (0, 49), (49, 0), (49, 49)];
        assert!(corners
            .into_iter()
            .all(|(col, row): (u16, u16)| (0.0..=1.0).contains(&noise(col, row))));
    }

    /// Spread: two neighbouring cells get unrelated grains — the texture is not a constant or a
    /// smooth ramp, which is what makes the comet grainy rather than a solid diamond.
    #[test]
    fn neighbours_differ() {
        assert_ne!(noise(20, 20), noise(21, 20));
        assert_ne!(noise(20, 20), noise(20, 21));
    }
}
