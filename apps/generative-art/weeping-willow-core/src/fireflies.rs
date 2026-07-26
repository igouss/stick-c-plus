//! The fireflies: a swarm of glow-bugs that wander through the canopy and pulse, some behind the
//! tree and some in front.
//!
//! The scene's life. Where the [`Tree`](crate::Tree) sways, the swarm drifts: each bug wanders a
//! slow closed loop around its own patch of the canopy and glows in and out, all as a pure function
//! of the same sway phase the tree reads. Two touches give the swarm depth and continuity:
//!
//! - **behind and in front.** Each bug is fixed to the [`foreground`](Firefly::foreground) or the
//!   background, so the display draws some behind the wood and foliage and some over them — the swarm
//!   surrounds the tree rather than floating on a single plane.
//! - **it never jumps.** Every bug's wander and pulse advance at *integer* multiples of the phase, so
//!   a full breath ([`PERIOD_MS`](crate::PERIOD_MS)) returns the whole swarm exactly to its start —
//!   the loop is seamless however long it runs, the same reason the tree's sway wraps.
//!
//! There is no state and no history — a bug at phase `φ` is a pure function of its folded wander and
//! `φ`. Every bug's loop (its centre, radii, frequencies, phases and side) is folded once into the
//! [`Swarm`] at startup from a per-index hash, so a frame costs, per bug, four [`SinTable`] lookups
//! and a handful of multiplies — no `sqrt`, no transcendental, no division on the hot path.

use core::f32::consts::TAU;

use alloc::boxed::Box;
use alloc::vec::Vec;

use platform_numerics::SinTable;

/// How many glow-bugs drift through the scene. Enough to feel alive around the canopy without
/// crowding it or costing more than a rounding error beside the foliage.
pub const FIREFLY_COUNT: usize = 18;

/// The band of the canopy the bugs' wander centres are scattered across, as `[0, 1]` fractions — the
/// crown and the space just around it, so the swarm haunts the foliage rather than the bare sky or
/// the ground.
const CENTRE_X: (f32, f32) = (0.10, 0.90);
const CENTRE_Y: (f32, f32) = (0.26, 0.82);
/// How far a bug wanders from its centre, as `[0, 1]` fractions — the radii of its slow loop. Small
/// enough to read as a drift, wide enough that the bugs plainly move.
const RADIUS_X: (f32, f32) = (0.05, 0.16);
const RADIUS_Y: (f32, f32) = (0.04, 0.13);

/// One glow-bug at a phase: where it is, how brightly it glows, and which side of the tree it is on.
///
/// Carried by value — a plain result. Turning the fraction position into a pixel and the glow into a
/// warm colour, and honouring the side by draw order, is the display crate's business.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Firefly {
    /// The bug's x as a fraction of the panel width. It may wander a little past `[0, 1]`; the
    /// display clips what falls off the edge.
    pub x: f32,
    /// The bug's y as a fraction of the panel height.
    pub y: f32,
    /// The bug's glow, `0.0` (dark, blinked off) to `1.0` (full) — the brightness the display scales
    /// its warm colour by.
    pub glow: f32,
    /// Whether this bug is in front of the tree (drawn over the foliage) or behind it (drawn under
    /// the wood).
    pub foreground: bool,
}

/// One bug's phase-invariant loop: the closed wander it traces and the pulse it glows on.
#[derive(Clone, Copy)]
struct Bug {
    /// The centre of the bug's wander, in `[0, 1]` fractions.
    centre_x: f32,
    centre_y: f32,
    /// The radii of the bug's wander loop.
    radius_x: f32,
    radius_y: f32,
    /// The integer angular frequencies of the wander in `x` and `y` — different, so the loop is a
    /// Lissajous curve, not a plain circle. Integer, so a full breath closes the loop exactly.
    freq_x: f32,
    freq_y: f32,
    /// The wander's phase offsets in `x` and `y`, so no two bugs trace the same loop in step.
    phase_x: f32,
    phase_y: f32,
    /// The integer frequency of the glow pulse and its phase offset — integer, so the blink returns
    /// with the loop each breath.
    pulse_freq: f32,
    pulse_phase: f32,
    /// Which side of the tree the bug is on.
    foreground: bool,
}

impl Bug {
    /// The bug at phase `φ`: its wandered position, its pulsed glow, its fixed side.
    fn at(&self, phi: f32, table: &SinTable) -> Firefly {
        Firefly {
            x: self.centre_x + self.radius_x * table.sin(self.freq_x * phi + self.phase_x),
            y: self.centre_y + self.radius_y * table.sin(self.freq_y * phi + self.phase_y),
            // 0.5 + 0.5·sin maps the pulse onto [0, 1]: the bug swells to full and blinks to dark.
            glow: 0.5 + 0.5 * table.sin(self.pulse_freq * phi + self.pulse_phase),
            foreground: self.foreground,
        }
    }
}

/// The swarm's phase-invariant capital: every bug's folded wander, folded once at startup.
///
/// Just the [`FIREFLY_COUNT`] bugs on a **heap** slice, `collect`ed at construction so it is never a
/// stack temporary (see the crate root). Held by the display for the app's life and swept each frame
/// by [`at`](Self::at).
pub struct Swarm {
    /// The [`FIREFLY_COUNT`] bugs' wander loops.
    bugs: Box<[Bug]>,
}

impl Swarm {
    /// Fold every bug's loop once, each from a per-index hash: its centre and radii, its integer
    /// wander and pulse frequencies, its phase offsets and its side. Deterministic, so the swarm is
    /// the same every run; spread, so no two bugs share a loop. `collect`ed onto the heap so the fold
    /// builds no array on the stack.
    pub fn new() -> Self {
        let bugs: Box<[Bug]> = (0..FIREFLY_COUNT).map(bug).collect::<Vec<Bug>>().into();
        Self { bugs }
    }

    /// Every bug at phase `φ`, read through `table`.
    ///
    /// The render loop's per-frame call. Borrows `self` and `table` for the life of the iterator; the
    /// display sweeps it twice a frame — once for the background bugs, once for the foreground — so
    /// the swarm brackets the tree.
    pub fn at<'a>(&'a self, phi: f32, table: &'a SinTable) -> impl Iterator<Item = Firefly> + 'a {
        self.bugs.iter().map(move |bug: &Bug| bug.at(phi, table))
    }
}

impl Default for Swarm {
    fn default() -> Self {
        Self::new()
    }
}

/// The `index`th bug's folded loop, hashed from its index so the swarm is spread yet deterministic.
fn bug(index: usize) -> Bug {
    Bug {
        centre_x: span(CENTRE_X, hash(index, 12.9898)),
        centre_y: span(CENTRE_Y, hash(index, 78.233)),
        radius_x: span(RADIUS_X, hash(index, 39.425)),
        radius_y: span(RADIUS_Y, hash(index, 27.162)),
        // 1, 2 or 3 turns of wander a breath — integer, so the loop closes each period.
        freq_x: 1.0 + libm::floorf(hash(index, 51.317) * 3.0),
        freq_y: 1.0 + libm::floorf(hash(index, 63.771) * 3.0),
        phase_x: hash(index, 91.113) * TAU,
        phase_y: hash(index, 17.053) * TAU,
        // 3 to 6 blinks a breath — integer, so the pulse returns with the wander.
        pulse_freq: 3.0 + libm::floorf(hash(index, 45.239) * 4.0),
        pulse_phase: hash(index, 83.191) * TAU,
        foreground: hash(index, 7.777) > 0.5,
    }
}

/// A `[0, 1)` value scaled onto `(lo, hi)`.
fn span(range: (f32, f32), t: f32) -> f32 {
    range.0 + (range.1 - range.0) * t
}

/// A bug's per-parameter hash in `[0, 1)`: a deterministic pseudo-random value from its index and a
/// salt, so each parameter of each bug is its own spread value. The classic
/// `frac(sin(i · salt) · 43758.5453)` shader hash, paid once per bug at startup.
fn hash(index: usize, salt: f32) -> f32 {
    let mixed: f32 = libm::sinf((index as f32 + 1.0) * salt) * 43_758.547;
    mixed - libm::floorf(mixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zero: a bug's glow never leaves `[0, 1]` — the `0.5 + 0.5·sin` pulse the display scales its
    /// colour by, so the warm colour never over- or under-saturates.
    #[test]
    fn a_glow_stays_in_the_unit_range() {
        let swarm: Swarm = Swarm::new();
        let table: SinTable = SinTable::new();
        let in_range: bool = swarm
            .at(1.3, &table)
            .all(|bug: Firefly| (0.0..=1.0).contains(&bug.glow));
        assert!(in_range, "a glow left the unit range");
    }

    /// One: the swarm drifts — one bug's position at two phases differs, so the scene is alive and
    /// not a fixed constellation.
    #[test]
    fn a_bug_drifts_with_the_phase() {
        let swarm: Swarm = Swarm::new();
        let table: SinTable = SinTable::new();
        let still: Firefly = swarm.at(0.0, &table).next().expect("a first bug");
        let drifted: Firefly = swarm.at(1.0, &table).next().expect("a first bug");
        assert!(
            still.x != drifted.x || still.y != drifted.y,
            "the bug did not drift"
        );
    }

    /// The swarm brackets the tree: at least one bug is in front and at least one behind, so the
    /// display has both a background and a foreground layer to draw.
    #[test]
    fn the_swarm_has_both_sides() {
        let swarm: Swarm = Swarm::new();
        let table: SinTable = SinTable::new();
        let front: usize = swarm
            .at(0.0, &table)
            .filter(|b: &Firefly| b.foreground)
            .count();
        assert!(front > 0, "no bug is in front of the tree");
        assert!(front < FIREFLY_COUNT, "no bug is behind the tree");
    }

    /// Many: the swarm is the whole set of bugs — every one, once.
    #[test]
    fn the_swarm_is_every_bug() {
        let swarm: Swarm = Swarm::new();
        let table: SinTable = SinTable::new();
        assert_eq!(swarm.at(0.0, &table).count(), FIREFLY_COUNT);
    }

    /// A full turn of phase returns the whole swarm to its start: every wander and pulse advances at
    /// an integer frequency, so `φ = 2π` lands back on `φ = 0` and the motion never teleports when
    /// the clock wraps. A non-integer frequency would leave a bug far from its start here; the
    /// tolerance is the sine table's own quantisation, not slack in the rule.
    #[test]
    fn a_full_turn_returns_the_swarm() {
        let swarm: Swarm = Swarm::new();
        let table: SinTable = SinTable::new();
        let closes: bool = swarm.at(0.0, &table).zip(swarm.at(TAU, &table)).all(
            |(start, wrapped): (Firefly, Firefly)| {
                (start.x - wrapped.x).abs() < 2e-2
                    && (start.y - wrapped.y).abs() < 2e-2
                    && (start.glow - wrapped.glow).abs() < 2e-2
            },
        );
        assert!(closes, "the swarm did not return over a full turn");
    }
}
