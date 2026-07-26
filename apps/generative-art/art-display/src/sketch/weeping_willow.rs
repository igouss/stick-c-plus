//! The weeping-willow-*tree* sketch: the wood stroked in bark, then the swaying canopy plotted over
//! it in foliage greens.
//!
//! The gallery's second original piece, and a *tree* where the [`willow`](super::willow) is a
//! curtain. The picture is a pure function of the elapsed clock and a folded [`Tree`]: the tree hands
//! this module its [`wood`](weeping_willow_core::Tree::wood) — a fixed set of tapering limbs — and,
//! at [`weeping_willow_core::phase`] of the clock, its [`sway`](weeping_willow_core::Tree::sway): a
//! stream of [`FrondPoint`]s, each a point on a hanging frond in `[0, 1]` fractions with a depth from
//! bough to tip. This module strokes each limb in bark, then plots each frond point in the green its
//! depth names — deep at the bough, light at the drooping tip.
//!
//! ## Wood first, then foliage
//!
//! The wood is drawn before the canopy, so the fronds hang *in front of* the branches they fall from,
//! the way a real willow's do. The limbs are stroked through `embedded-graphics`' [`Line`] at the
//! pixel width their taper names — a handful of short strokes, cheap beside the canopy — and the
//! fronds are plotted point by point, each bridged one pixel down to the next so a near-vertical
//! frond reads as a strand and not a dotted line.
//!
//! ## Fitting the tree to the glass — nothing to fit
//!
//! Like the curtain and unlike the ported sketches, the tree is authored for no particular canvas:
//! its wood and fronds arrive in `[0, 1]` fractions of the panel's width and height. So there is no
//! square source to letterbox or crop — the fractions scale straight onto whatever panel the render
//! is handed, `x` by its width and `y` by its height. A frond tip the sway carries a little past the
//! edge is clipped by [`Canvas::set`]; nothing else falls off.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use weeping_willow_core::{phase, Firefly, FrondPoint, Segment, SinTable, Swarm, Tree};

use crate::canvas::Canvas;
use crate::colour::blend;
use platform_core::Tick;

/// The wood's colour: a willow bark brown. `(90, 57, 41)` on `[0, 255]`, quantised to `Rgb565` —
/// red-dominant, so the trunk and boughs never read as foliage. Drawn solid; only the fronds carry a
/// gradient.
const BARK: Rgb565 = Rgb565::new(11, 14, 5);

/// A frond's colour at its **bough** end: a deep willow green. `(34, 74, 28)` on `[0, 255]`, as
/// `[0, 1]` fractions — the shaded top of the canopy, where a frond leaves the wood.
const ROOT_GREEN: (f32, f32, f32) = (0.133, 0.290, 0.110);

/// A frond's colour at its **tip**: a light yellow-green. `(165, 210, 95)` on `[0, 255]`, as
/// `[0, 1]` fractions — the sunlit drooping ends. A point's depth ramps between these two.
const TIP_GREEN: (f32, f32, f32) = (0.647, 0.824, 0.373);

/// A firefly's colour at **full glow**: a warm yellow-green, brighter and more golden than the
/// foliage so a bug reads as a spark of light and not a leaf. `(235, 255, 128)` on `[0, 255]`, as
/// `[0, 1]` fractions — a bug's glow scales black up to this.
const FIREFLY_GLOW: (f32, f32, f32) = (0.92, 1.0, 0.50);

/// Below this glow a firefly is blinked **off** and draws nothing — so a dark bug never punches a
/// black hole through the foliage it floats in front of; it simply is not there this frame.
const GLOW_FLOOR: f32 = 0.05;

/// The moon's disc: a pale, faintly-warm ivory, the brightest thing in the scene. `(224, 230, 208)`
/// on `[0, 255]`, quantised to `Rgb565` — whiter and bluer than any firefly, so it reads as cold
/// moonlight and not a warm bug.
const MOON: Rgb565 = Rgb565::new(27, 57, 25);

/// The moonlight glow around the disc: a cool silver-blue, faded to nothing across the reach. As
/// `[0, 1]` fractions — a pixel's distance from the moon scales black up to this.
const MOONLIGHT: (f32, f32, f32) = (0.55, 0.62, 0.74);

/// The moon's place and size in the night sky, as fractions of the panel: high in the sky and off to
/// one side, clear of the canopy's crown, so it hangs *behind* the tree rather than among the fronds.
const MOON_X: f32 = 0.76;
const MOON_Y: f32 = 0.14;
/// The bright disc's radius, and how far past it the moonlight washes — both fractions of the panel
/// width, so the glow is a soft circle whatever the panel's aspect.
const MOON_RADIUS: f32 = 0.075;
const MOONLIGHT_REACH: f32 = 0.26;

/// Draw the weeping willow at `elapsed` into `canvas`, which the caller has reset to the ground.
///
/// Draws the scene back to front: the moon and its moonlight in the night sky, then the fireflies
/// behind the tree, then the wood stroked in bark, then the swaying canopy of fronds, then the
/// fireflies in front — so the moon hangs behind everything, the swarm brackets the tree, and the
/// foliage hangs over the wood. The moon is static; the rest is a pure function of this frame's phase,
/// with the [`Tree`] and [`Swarm`] folded once and held by the gallery. Generic over the [`Canvas`],
/// so the scene lands identically in a host [`Frame`](crate::Frame) for the goldens and in the
/// firmware's wire-order buffer on the glass.
pub fn render<C: Canvas>(
    canvas: &mut C,
    tree: &Tree,
    swarm: &Swarm,
    table: &SinTable,
    elapsed: Tick,
    size: Size,
) {
    let width: f32 = size.width as f32;
    let height: f32 = size.height as f32;
    let phi: f32 = phase(elapsed);

    // Back to front, each layer drawn through its own `#[inline(never)]` function. The whole scene
    // inlined into one frame sums every layer's locals and peaks near the 8 KiB display stack (see
    // `firmware-green-host-not-device`); out-of-line, each layer pays its own short frame in turn and
    // the peak is the deepest single layer, not their sum.
    plot_moon(canvas, width, height); // the moon and its moonlight, furthest back
    plot_swarm(canvas, swarm, table, phi, width, height, false); // fireflies behind the tree
    plot_wood(canvas, tree, width, height); // the trunk and boughs
    plot_canopy(canvas, tree, table, phi, width, height); // the swaying fronds, over the wood
    plot_swarm(canvas, swarm, table, phi, width, height, true); // fireflies in front of the tree
}

/// Plot one layer of the swarm into `canvas`: the [`foreground`](Firefly::foreground) bugs, or the
/// background ones, chosen by `foreground`. Drawn out-of-line (see [`render`]) so the swarm's frame
/// never joins the rest of the scene's on the display stack.
#[inline(never)]
fn plot_swarm<C: Canvas>(
    canvas: &mut C,
    swarm: &Swarm,
    table: &SinTable,
    phi: f32,
    width: f32,
    height: f32,
    foreground: bool,
) {
    swarm
        .at(phi, table)
        .filter(|bug: &Firefly| bug.foreground == foreground)
        .for_each(|bug: Firefly| plot_firefly(canvas, &bug, width, height));
}

/// Stroke the whole wood — the trunk and every bough — in bark. Out-of-line; see [`render`].
#[inline(never)]
fn plot_wood<C: Canvas>(canvas: &mut C, tree: &Tree, width: f32, height: f32) {
    tree.wood()
        .iter()
        .for_each(|limb: &Segment| stroke_limb(canvas, limb, width, height));
}

/// Plot the swaying canopy over the wood: each frond point in the green its depth names, bridged one
/// pixel down so a near-vertical frond draws solid rather than dotted. Out-of-line; see [`render`].
#[inline(never)]
fn plot_canopy<C: Canvas>(
    canvas: &mut C,
    tree: &Tree,
    table: &SinTable,
    phi: f32,
    width: f32,
    height: f32,
) {
    tree.sway(phi, table).for_each(|point: FrondPoint| {
        let px: i32 = libm::roundf(point.x * width) as i32;
        let py: i32 = libm::roundf(point.y * height) as i32;
        let colour: Rgb565 = blend(ROOT_GREEN, TIP_GREEN, point.depth);
        canvas.set(px, py, colour);
        canvas.set(px, py + 1, colour);
    });
}

/// Plot one firefly onto `canvas`: a warm point of light at its `[0, 1]` position, scaled by its
/// glow, skipped entirely when it has blinked below [`GLOW_FLOOR`].
///
/// A foreground bug also gets a dim four-pixel halo, so the near swarm reads brighter and closer than
/// the far. A blinked-off bug draws nothing at all — never a black pixel — so it cannot punch a hole
/// through the foliage it floats before.
fn plot_firefly<C: Canvas>(canvas: &mut C, bug: &Firefly, width: f32, height: f32) {
    if bug.glow < GLOW_FLOOR {
        return;
    }
    let px: i32 = libm::roundf(bug.x * width) as i32;
    let py: i32 = libm::roundf(bug.y * height) as i32;
    let core: Rgb565 = blend((0.0, 0.0, 0.0), FIREFLY_GLOW, bug.glow);
    canvas.set(px, py, core);
    if bug.foreground {
        let halo: Rgb565 = blend((0.0, 0.0, 0.0), FIREFLY_GLOW, bug.glow * 0.45);
        canvas.set(px + 1, py, halo);
        canvas.set(px - 1, py, halo);
        canvas.set(px, py + 1, halo);
        canvas.set(px, py - 1, halo);
    }
}

/// Stroke one wood limb onto `canvas` in [`BARK`]: walk its `[0, 1]` segment in pixel steps and, at
/// each, lay a short *perpendicular* run of pixels as wide as the limb's tapering half-width there — a
/// filled thick line, tapered end to end.
///
/// A hand-rolled brush rather than an `embedded-graphics` stroked line, and deliberately so: the
/// styled-primitive path is far deeper on the stack than a flat [`Canvas::set`] loop. Stroking the
/// perpendicular rather than a full square per step keeps the cost to the limb's *area*, not its
/// bounding box — roughly an order of magnitude fewer writes on the fat trunk, which is what keeps the
/// scene inside its frame budget. `O(1)` in stack, and [`Canvas::set`] clips whatever falls off panel.
fn stroke_limb<C: Canvas>(canvas: &mut C, limb: &Segment, width: f32, height: f32) {
    let ax: f32 = limb.a.x * width;
    let ay: f32 = limb.a.y * height;
    let run: f32 = limb.b.x * width - ax;
    let rise: f32 = limb.b.y * height - ay;
    let length: f32 = libm::sqrtf(run * run + rise * rise);
    let steps: i32 = (libm::ceilf(length) as i32).max(1);
    // The unit perpendicular to the limb — the direction its thickness spreads. `length` is a real
    // span (limbs are sub-divisions of the trunk and boughs), so it is never zero here.
    let perp_x: f32 = -rise / length;
    let perp_y: f32 = run / length;
    (0..=steps).for_each(|step: i32| {
        let t: f32 = step as f32 / steps as f32;
        let centre_x: f32 = ax + run * t;
        let centre_y: f32 = ay + rise * t;
        let half_width: f32 = limb.half_width_a + (limb.half_width_b - limb.half_width_a) * t;
        let radius: i32 = libm::floorf(half_width * width) as i32;
        (-radius..=radius).for_each(|offset: i32| {
            let x: i32 = libm::roundf(centre_x + perp_x * offset as f32) as i32;
            let y: i32 = libm::roundf(centre_y + perp_y * offset as f32) as i32;
            canvas.set(x, y, BARK);
        });
    });
}

/// How many brightness steps the moonlight glow is quantised into. The per-pixel `blend` (three soft
/// `roundf`s) is the whole moon's cost on this FPU, so it is paid once per step into a small table at
/// the top of the frame, not once per glow pixel — a glow pixel then only indexes the table. Near the
/// number of distinct `Rgb565` colours the ramp can hold, so the glow is smooth, not banded.
const MOONLIGHT_LEVELS: usize = 40;

/// Plot the moon and its moonlight into `canvas`: a bright ivory disc in the night sky, ringed by a
/// soft silver-blue glow that washes out to nothing across the reach.
///
/// Walks the square that bounds the glow and colours each pixel by its distance from the moon's
/// centre — inside the disc it is [`MOON`], beyond it a fading [`MOONLIGHT`] glow, past the reach it
/// is left as night. Deliberately cheap on this soft-float FPU: distances are compared *squared* (no
/// per-pixel `sqrt`), the divide is hoisted to one reciprocal, and the glow colours are pre-blended
/// into a [`MOONLIGHT_LEVELS`] table so a glow pixel costs only a multiply, a cast and a table lookup.
/// `O(1)` in stack, and [`Canvas::set`] clips whatever falls off the panel. Out-of-line; see
/// [`render`].
#[inline(never)]
fn plot_moon<C: Canvas>(canvas: &mut C, width: f32, height: f32) {
    let centre_x: i32 = libm::roundf(MOON_X * width) as i32;
    let centre_y: i32 = libm::roundf(MOON_Y * height) as i32;
    let disc_squared: f32 = (MOON_RADIUS * width) * (MOON_RADIUS * width);
    let reach: f32 = MOON_RADIUS * width + MOONLIGHT_REACH * width;
    let reach_squared: f32 = reach * reach;
    // One reciprocal for the whole moon, so a glow pixel multiplies instead of dividing (soft-divide
    // is slow); the glow table is pre-blended, so a glow pixel never blends.
    let inverse_span: f32 = 1.0 / (reach_squared - disc_squared);
    let glow: [Rgb565; MOONLIGHT_LEVELS] = core::array::from_fn(|level: usize| {
        let fade: f32 = (level + 1) as f32 / MOONLIGHT_LEVELS as f32; // (0, 1], faint → full
        blend((0.0, 0.0, 0.0), MOONLIGHT, fade * fade) // squared: a soft, quick falloff
    });
    let bound: i32 = libm::ceilf(reach) as i32;
    (-bound..=bound).for_each(|offset_y: i32| {
        (-bound..=bound).for_each(|offset_x: i32| {
            let distance_squared: f32 = (offset_x * offset_x + offset_y * offset_y) as f32;
            if distance_squared <= disc_squared {
                canvas.set(centre_x + offset_x, centre_y + offset_y, MOON);
            } else if distance_squared <= reach_squared {
                // Fade is 1 at the disc's edge, 0 at the reach — the table index into the glow.
                let fade: f32 = (reach_squared - distance_squared) * inverse_span;
                let level: usize =
                    ((fade * MOONLIGHT_LEVELS as f32) as usize).min(MOONLIGHT_LEVELS - 1);
                canvas.set(centre_x + offset_x, centre_y + offset_y, glow[level]);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Frame;
    use crate::gallery::{canvas_size, GROUND_COLOUR};
    use platform_core::ScreenRotation;
    use platform_display::testing::Framebuffer;

    /// The portrait quarter turn the gallery is pinned to.
    const PORTRAIT: ScreenRotation = ScreenRotation::Deg90;

    /// Blit the whole scene — tree and swarm — at `elapsed` into a fresh portrait framebuffer, the
    /// whole host path.
    fn painted(elapsed: Tick) -> Framebuffer {
        let table: SinTable = SinTable::new();
        let tree: Tree = Tree::new();
        let swarm: Swarm = Swarm::new();
        let canvas: Size = canvas_size(PORTRAIT);
        let mut frame: Frame = Frame::new();
        frame.reset(canvas, GROUND_COLOUR);
        render(&mut frame, &tree, &swarm, &table, elapsed, canvas);
        let mut fb: Framebuffer = Framebuffer::sized(canvas);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        fb
    }

    /// One: the tree puts ink on the glass — the wood and fronds are drawn, not skipped.
    #[test]
    fn the_tree_paints_pixels() {
        assert!(painted(0).lit_pixels() > 0);
    }

    /// The canopy is **green**: some lit pixel is green-dominant *and* low in blue, so it is foliage
    /// and not the pale-blue moon or a yellow-green firefly — the fronds, not only the wood, reached
    /// the glass.
    #[test]
    fn the_canopy_is_green() {
        let fb: Framebuffer = painted(0);
        let has_green: bool = fb.pixels().iter().any(|c: &Rgb565| {
            // Green field is six bits (0..63), red and blue five (0..31); halve green to compare on
            // the same scale, so this asks a true "greener than", not a wider-field artefact. Foliage
            // is deep in blue (≤12); the moon (25) and fireflies (16) are not, so the blue cap keeps
            // this test about the fronds alone.
            c.g() / 2 >= c.r() && c.g() / 2 >= c.b() && c.b() <= 14 && *c != Rgb565::BLACK
        });
        assert!(has_green, "no green foliage reached the glass");
    }

    /// The moon lights the sky: a pale, blue-tinged pixel reaches the glass that nothing else in the
    /// scene can be — foliage and fireflies are both deep in blue, so a bright pixel with real blue is
    /// the moon's disc.
    #[test]
    fn the_moon_lights_the_sky() {
        let fb: Framebuffer = painted(0);
        let has_moon: bool = fb
            .pixels()
            .iter()
            .any(|c: &Rgb565| c.r() >= 24 && c.g() >= 52 && c.b() >= 22);
        assert!(has_moon, "the moon did not light the sky");
    }

    /// The tree stands on **bark**: the wood's brown reaches the glass, so the piece is a tree and not
    /// a bare canopy. A frond overdrawing the whole trunk would trip this.
    #[test]
    fn the_trunk_is_brown() {
        assert!(
            painted(0).pixels().contains(&BARK),
            "no bark reached the glass"
        );
    }

    /// The scene sparks with fireflies: at some moment a bug's warm glow reaches the glass — a pixel
    /// brighter and more golden than any foliage or bark can be (`r ≥ 24` and `g ≥ 55`, which the
    /// deepest tip green never reaches). Sampled across the breath, since a given bug may be blinked
    /// off at any one instant.
    #[test]
    fn the_scene_sparks_with_fireflies() {
        let sparks = |fb: &Framebuffer| -> bool {
            // Bright, warm and low in blue: a firefly, not the pale-blue moon (whose blue is higher).
            fb.pixels()
                .iter()
                .any(|c: &Rgb565| c.r() >= 24 && c.g() >= 55 && c.b() < 20)
        };
        let a: Framebuffer = painted(0);
        let b: Framebuffer = painted(weeping_willow_core::PERIOD_MS / 3);
        let c: Framebuffer = painted(2 * weeping_willow_core::PERIOD_MS / 3);
        assert!(
            sparks(&a) || sparks(&b) || sparks(&c),
            "no firefly lit the scene across a breath"
        );
    }

    /// Nothing the scene draws escapes the canvas — a frond tip the sway carries past the edge, a
    /// limb, or a firefly's halo at the border is all clipped to the panel.
    #[test]
    fn nothing_escapes_the_canvas() {
        assert_eq!(painted(weeping_willow_core::PERIOD_MS / 4).escaped(), 0);
    }

    /// Many: three moments of the tree paint three distinct pictures — the canopy actually sways, it
    /// does not freeze.
    #[test]
    fn the_canopy_sways_over_time() {
        let a: Framebuffer = painted(0);
        let b: Framebuffer = painted(weeping_willow_core::PERIOD_MS / 4);
        let c: Framebuffer = painted(weeping_willow_core::PERIOD_MS / 2);
        assert_ne!(a.pixels(), b.pixels());
        assert_ne!(b.pixels(), c.pixels());
        assert_ne!(a.pixels(), c.pixels());
    }
}
