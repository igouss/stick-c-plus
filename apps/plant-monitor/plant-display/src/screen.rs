//! The observation screen: an [`Observation`] as two rows of text.
//!
//! The whole of what the plant monitor's glass ever says, and the only place that
//! decides it. Device-independent by construction — it draws into any
//! [`DrawTarget`], which is what lets the on-target panel and a host framebuffer
//! render *the same code* rather than two copies that drift.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use plant_core::{Observation, ProbeFault};
use platform_core::ScreenRotation;
use platform_display::{sprite, text_line, RenderError};

use crate::layout::{LINE_WIDTH, PCT_Y, RAW_Y, SPRITE_ORIGIN, SPRITE_SCALE, TEXT_X};
use crate::scene::{self, Scene};

/// A [`ProbeFault`] as a label that fits [`LINE_WIDTH`](crate::LINE_WIDTH) characters.
///
/// The domain's own [`Display`](core::fmt::Display) for a fault is a full sentence,
/// written for a log line or a Home Assistant text sensor. A 240-pixel-wide panel
/// cannot show a sentence, so this boundary chooses words for its own pixel budget —
/// which is exactly the sort of decision a boundary exists to make. The words stay
/// faithful to the domain's caution: `RAW HIGH` reports what was *observed*, and does
/// not assert "corroded", which the reading alone cannot establish.
pub const fn fault_label(fault: ProbeFault) -> &'static str {
    match fault {
        ProbeFault::OverRange => "RAW HIGH",
        ProbeFault::UnderRange => "RAW LOW",
        ProbeFault::Unreadable => "ADC ERROR",
    }
}

/// Render the latest observation: two rows of text, and the creature that stands for it.
///
/// `elapsed_ms` is how long this observation has been on the glass — the creature's
/// animation clock. A [`Fresh`](Observation::Fresh) observation ignores it entirely and
/// shows a motionless creature, so a healthy monitor repaints nothing and may sleep; see
/// [`crate::scene`].
///
/// No full-screen clear — each line paints its own row over an opaque background, so
/// only the two text rows change and there is no flash. Numbers are right-aligned in
/// a fixed field, so the digits hold a steady column as the value's width changes.
///
/// Each unavailable state gets its **own words, in red**. An operator glancing at the
/// glass should be able to tell a lying probe from a stopped sampler without attaching
/// a serial cable — that distinction is the whole reason this takes an [`Observation`]
/// rather than an `Option<Measurement>`. `NeverSampled` is white, not red: a device
/// that has simply not finished its first cycle is not faulty.
pub fn render<D>(
    target: &mut D,
    observation: Observation,
    elapsed_ms: u64,
    // This screen is landscape-only for now: it takes the rotation the platform supplies
    // and does not yet honour it. A portrait layout is its own bead.
    _rotation: ScreenRotation,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text(target, observation)?;
    creature(target, observation, elapsed_ms)
}

/// Paint the creature for `observation` in the panel's right-hand region.
///
/// Drawn **opaque** (see [`sprite::draw_onto`]): each frame overwrites its own 100×100 box
/// against the black background, so an animating creature never smears the limbs of the
/// frame before it, and never needs a clear that would flash.
fn creature<D>(
    target: &mut D,
    observation: Observation,
    elapsed_ms: u64,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let scene: Scene = scene::scene(observation);
    let index: usize = scene::frame_index(observation, elapsed_ms);
    sprite::draw_onto(
        target,
        scene.sprite,
        &scene.sprite.frames()[index],
        SPRITE_ORIGIN,
        SPRITE_SCALE,
        Rgb565::BLACK,
    )
}

/// Draw one plant-monitor text row: the platform [`text_line`] primitive bound to this
/// app's left column ([`TEXT_X`]) and field width ([`LINE_WIDTH`]), so the eight call sites
/// below carry only their row, colour, and content.
fn line<D>(
    target: &mut D,
    y: i32,
    color: Rgb565,
    content: core::fmt::Arguments<'_>,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_line(target, Point::new(TEXT_X, y), color, LINE_WIDTH, content)
}

/// Paint the two text rows for `observation`.
fn text<D>(target: &mut D, observation: Observation) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    match observation {
        Observation::Fresh(measurement) => {
            line(
                target,
                RAW_Y,
                Rgb565::WHITE,
                format_args!("RAW  {:>4}", measurement.raw()),
            )?;
            line(
                target,
                PCT_Y,
                Rgb565::WHITE,
                format_args!("SOIL {:>3}%", measurement.percent()),
            )?;
        }
        Observation::Faulted(fault) => {
            line(target, RAW_Y, Rgb565::RED, format_args!("FAULT"))?;
            line(
                target,
                PCT_Y,
                Rgb565::RED,
                format_args!("{}", fault_label(fault)),
            )?;
        }
        Observation::Stale => {
            line(target, RAW_Y, Rgb565::RED, format_args!("STALE"))?;
            line(target, PCT_Y, Rgb565::RED, format_args!("NO SAMPLE"))?;
        }
        Observation::NeverSampled => {
            line(target, RAW_Y, Rgb565::WHITE, format_args!("SOIL --"))?;
            line(target, PCT_Y, Rgb565::WHITE, format_args!("starting"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use plant_core::{Measurement, Moisture};
    use platform_display::testing::Framebuffer;
    use proptest::prelude::*;

    fn measurement(raw: u16, percent: u8) -> Measurement {
        Measurement::new(
            raw,
            Moisture::new(percent).expect("test percent is 0..=100"),
        )
    }

    /// Every observation, so a test can sweep the whole state space without a loop
    /// in the test body (cyclomatic complexity 1 per case).
    const FRESH: Observation = Observation::Fresh(Measurement::new(
        2048,
        match Moisture::new(50) {
            Some(m) => m,
            None => unreachable!(),
        },
    ));

    /// Paint `observation` at the instant it appeared — the state every screen starts in.
    fn painted(observation: Observation) -> Framebuffer {
        painted_at(observation, 0)
    }

    /// Paint `observation` after it has been on the glass for `elapsed_ms`.
    fn painted_at(observation: Observation, elapsed_ms: u64) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        render(&mut fb, observation, elapsed_ms, ScreenRotation::Deg0)
            .expect("a framebuffer render cannot fail");
        fb
    }

    /// Zero: nothing rendered leaves the canvas black.
    #[test]
    fn an_unrendered_canvas_is_blank() {
        assert_eq!(Framebuffer::new().lit_pixels(), 0);
    }

    /// One: a fresh observation puts ink on the glass.
    #[test]
    fn a_fresh_observation_paints_pixels() {
        assert!(painted(FRESH).lit_pixels() > 0);
    }

    /// Many: all four states paint, and no two of them paint the same picture. If any
    /// pair collided, an operator could not tell those states apart by looking — which
    /// is the entire purpose of `Observation` over `Option`.
    #[test]
    fn a_faulted_observation_differs_from_a_stale_one() {
        assert_ne!(
            painted(Observation::Faulted(ProbeFault::OverRange)).pixels(),
            painted(Observation::Stale).pixels(),
            "a lying probe and a dead sampler must not look alike"
        );
    }

    #[test]
    fn each_fault_paints_its_own_distinct_reason() {
        assert_ne!(
            painted(Observation::Faulted(ProbeFault::OverRange)).pixels(),
            painted(Observation::Faulted(ProbeFault::UnderRange)).pixels()
        );
        assert_ne!(
            painted(Observation::Faulted(ProbeFault::UnderRange)).pixels(),
            painted(Observation::Faulted(ProbeFault::Unreadable)).pixels()
        );
        assert_ne!(
            painted(Observation::Faulted(ProbeFault::OverRange)).pixels(),
            painted(Observation::Faulted(ProbeFault::Unreadable)).pixels()
        );
    }

    #[test]
    fn a_never_sampled_observation_differs_from_a_stale_one() {
        assert_ne!(
            painted(Observation::NeverSampled).pixels(),
            painted(Observation::Stale).pixels(),
            "a booting device must not look like a dead one"
        );
    }

    /// Two distinct measurements paint distinct pictures — the number actually
    /// reaches the pixels, rather than a label that ignores it.
    #[test]
    fn two_different_measurements_paint_differently() {
        assert_ne!(
            painted(Observation::Fresh(measurement(100, 10))).pixels(),
            painted(Observation::Fresh(measurement(3000, 90))).pixels()
        );
    }

    /// Rendering is a pure function of the observation: the same value twice paints
    /// the same pixels. This is what the display loop's change-suppression assumes.
    #[test]
    fn rendering_the_same_observation_twice_paints_the_same_pixels() {
        assert_eq!(painted(FRESH).pixels(), painted(FRESH).pixels());
    }

    /// **The power rule, at the pixel level.** A healthy screen is byte-identical no
    /// matter how long it has been up, so the render loop finds nothing to repaint and
    /// the device may sleep between samples. Asserted on pixels, not on the `Motion` tag:
    /// a still creature that nevertheless jittered one pixel would keep the CPU awake
    /// forever, and only this test would notice.
    #[test]
    fn a_healthy_screen_never_changes_however_long_it_has_been_up() {
        assert_eq!(
            painted_at(FRESH, 0).pixels(),
            painted_at(FRESH, 3_000).pixels()
        );
        assert_eq!(
            painted_at(FRESH, 0).pixels(),
            painted_at(FRESH, u64::MAX).pixels()
        );
    }

    /// And the converse: an unhealthy screen *does* move, so a fault is visible from
    /// across a room. Anchored on the sprite's own first hold, so it cannot go vacuous if
    /// the vendored art changes its timing.
    #[test]
    fn an_unhealthy_screen_animates() {
        let advanced: u64 =
            u64::from(crate::scene::scene(Observation::Stale).sprite.frames()[0].hold_ms());
        assert_ne!(
            painted_at(Observation::Stale, 0).pixels(),
            painted_at(Observation::Stale, advanced).pixels(),
            "the stale creature never moved: a dead sampler looks like a healthy one"
        );
    }

    /// An animating creature must overwrite its own box, not smear. Painting frame N over
    /// frame N-1 must equal painting frame N onto a blank canvas.
    #[test]
    fn an_animating_creature_erases_its_previous_frame() {
        let advanced: u64 =
            u64::from(crate::scene::scene(Observation::Stale).sprite.frames()[0].hold_ms());
        let mut canvas: Framebuffer = Framebuffer::new();
        render(&mut canvas, Observation::Stale, 0, ScreenRotation::Deg0).expect("the first frame");
        render(
            &mut canvas,
            Observation::Stale,
            advanced,
            ScreenRotation::Deg0,
        )
        .expect("the second frame");

        assert_eq!(
            canvas.pixels(),
            painted_at(Observation::Stale, advanced).pixels(),
            "the previous frame's limbs survived: the creature is compositing, not overwriting"
        );
    }

    /// Repaint `first`, then `second`, on one canvas — as the panel does, since the
    /// display loop never clears between frames. The result must equal `second`
    /// painted onto a blank canvas: no glyph of `first` survives.
    fn repaint(first: Observation, second: Observation) -> Framebuffer {
        let mut canvas: Framebuffer = Framebuffer::new();
        render(&mut canvas, first, 0, ScreenRotation::Deg0).expect("the first paint");
        render(&mut canvas, second, 0, ScreenRotation::Deg0).expect("the second paint");
        canvas
    }

    // ---- The fixed-width padding contract, observed at the pixel level. ----
    //
    // Each pair below is chosen so the two strings differ in LENGTH. That is not
    // incidental: `"RAW  {:>4}"` already right-aligns 4095 and 7 to the same nine
    // characters, so a `4095 -> 7` test passes with the padding deleted entirely.
    // This suite was written that way first, and a mutation run caught it. The
    // padding only ever does work when the rendered lengths actually differ.

    /// A healthy probe that faults: `RAW  4095` (9 chars) gives way to `FAULT` (5).
    /// Without padding, `095` stays on the glass beside the word FAULT — the most
    /// realistic form of this bug, and the most misleading.
    #[test]
    fn a_fault_erases_the_measurement_it_replaces() {
        let observed: Framebuffer = repaint(
            Observation::Fresh(measurement(4095, 100)),
            Observation::Faulted(ProbeFault::OverRange),
        );
        assert_eq!(
            observed.pixels(),
            painted(Observation::Faulted(ProbeFault::OverRange)).pixels(),
            "digits of the old reading survived under the FAULT line"
        );
    }

    /// One fault giving way to a shorter-named fault: `ADC ERROR` (9) → `RAW LOW` (7).
    #[test]
    fn a_shorter_fault_label_erases_the_longer_one() {
        let observed: Framebuffer = repaint(
            Observation::Faulted(ProbeFault::Unreadable),
            Observation::Faulted(ProbeFault::UnderRange),
        );
        assert_eq!(
            observed.pixels(),
            painted(Observation::Faulted(ProbeFault::UnderRange)).pixels(),
            "the wider label left glyphs behind"
        );
    }

    /// A five-digit count giving way to a one-digit one: `RAW  65535` (10, the full
    /// field) → `RAW     7` (9). Only the pad column separates them.
    #[test]
    fn a_narrower_raw_count_erases_the_wider_one() {
        let observed: Framebuffer = repaint(
            Observation::Fresh(measurement(u16::MAX, 50)),
            Observation::Fresh(measurement(7, 50)),
        );
        assert_eq!(
            observed.pixels(),
            painted(Observation::Fresh(measurement(7, 50))).pixels(),
            "the fifth digit survived the repaint"
        );
    }

    /// Staleness giving way to the boot placeholder: `NO SAMPLE` (9) → `starting` (8).
    #[test]
    fn the_boot_placeholder_erases_the_stale_line() {
        let observed: Framebuffer = repaint(Observation::Stale, Observation::NeverSampled);
        assert_eq!(
            observed.pixels(),
            painted(Observation::NeverSampled).pixels(),
            "the trailing E of NO SAMPLE survived"
        );
    }

    /// Nothing is drawn outside the canvas, in any of the four states. The test
    /// framebuffer counts escaping pixels rather than clipping them, so a glyph that
    /// ran off the edge fails here instead of quietly disappearing.
    #[test]
    fn no_observation_escapes_the_canvas() {
        assert_eq!(painted(FRESH).escaped(), 0);
        assert_eq!(
            painted(Observation::Faulted(ProbeFault::Unreadable)).escaped(),
            0
        );
        assert_eq!(painted(Observation::Stale).escaped(), 0);
        assert_eq!(painted(Observation::NeverSampled).escaped(), 0);
    }

    proptest! {
        /// The layout holds for *every* measurement, not just the ones we thought of:
        /// no raw count and no percent can push a glyph off the canvas. `u16::MAX` is
        /// wider than the 12-bit ADC can ever produce, so this covers a future
        /// converter too.
        #[test]
        fn no_measurement_ever_escapes_the_canvas(raw: u16, percent in 0u8..=100) {
            let fb: Framebuffer = painted(Observation::Fresh(measurement(raw, percent)));
            prop_assert_eq!(fb.escaped(), 0);
        }

        /// And every measurement puts *some* ink on the glass — a render that silently
        /// painted nothing would pass every "does not escape" test ever written.
        #[test]
        fn every_measurement_paints_something(raw: u16, percent in 0u8..=100) {
            let fb: Framebuffer = painted(Observation::Fresh(measurement(raw, percent)));
            prop_assert!(fb.lit_pixels() > 0);
        }
    }

    /// Fault labels are distinct, and each fits the field the layout pads to. A label
    /// wider than `LINE_WIDTH` would not be erased by the next value's padding.
    #[test]
    fn every_fault_label_is_distinct_and_fits_the_field() {
        let over: &str = fault_label(ProbeFault::OverRange);
        let under: &str = fault_label(ProbeFault::UnderRange);
        let unreadable: &str = fault_label(ProbeFault::Unreadable);

        assert_ne!(over, under);
        assert_ne!(under, unreadable);
        assert_ne!(over, unreadable);
        assert!(over.len() <= LINE_WIDTH);
        assert!(under.len() <= LINE_WIDTH);
        assert!(unreadable.len() <= LINE_WIDTH);
    }
}
