//! The plume sketch: the feathered frond, projected onto the panel and plotted into the frame.
//!
//! A faithful move of the standalone plume's rendering into the gallery. The picture is a pure
//! function of the elapsed clock: [`plume_core::phase`] turns the clock into an animation phase,
//! [`plume_core::plume`] turns the phase into a cloud of points in the original 400×400 canvas
//! space, and [`project`] lands each point on the panel. The frond is white on the frame's black
//! ground — a single bit of colour, but plotted into the same `Rgb565` frame the colour sketches
//! use, so the whole gallery blits through one path.
//!
//! ## Fitting the frond to the glass
//!
//! The field lives in the original's 400×400 space, where the frond is centred near `(200, 203)`
//! and stands about 226 tall — a portrait shape, which is the M5StickC Plus's shape on end. So
//! the projection is a single uniform scale about the frond's centre, landing it in the middle of
//! the panel: the same scale on both axes, so the frond keeps its proportions rather than being
//! stretched to fill a differently-shaped canvas. [`SCALE`] is set by the width, the binding
//! dimension: at 0.75 the frond's ~180-wide body spans the panel's 135 columns with its outermost
//! barbs grazing the edge, and its height lands comfortably inside the 240 rows.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::{RgbColor, Size};
use plume_core::FieldPoint;

use crate::canvas::Canvas;

/// The colour the lit frond is drawn in. White, faithful to the source's translucent-white dots.
pub const PLUME_COLOUR: Rgb565 = Rgb565::WHITE;

/// A frond point already projected onto the panel: the pixel to light, and whether to light its
/// right neighbour too — the barbs' bright two-pixel spine.
///
/// This is the frond-compute port's output currency ([`FrondCompute`](crate::FrondCompute)): the
/// projection arithmetic is already spent, so plotting one ([`plot_screen`]) is a bare pixel set.
/// Producing it ahead of the plot is what lets the two-core evaluator do the projection on its
/// worker and buffer the result compactly — `Option<ScreenPoint>` is six bytes (the `wide` bool's
/// niche carries the `None`), half the twelve a canvas [`FieldPoint`] costs, which is the heap the
/// larger frond needs. The coordinates are `i16`: every drawable pixel sits inside a 240-row panel,
/// and the saturating `f32`-to-`i16` cast in [`project`] sends a far-off finite point to the edge of
/// the type, still off-canvas and still clipped, exactly as a wider integer would.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ScreenPoint {
    /// Column on the panel. May be negative or past the right edge — the canvas clips it.
    pub x: i16,
    /// Row on the panel.
    pub y: i16,
    /// Whether to light the pixel to the right as well — the original's fat dot.
    pub wide: bool,
}

// The far-half buffer stores one `Option<ScreenPoint>` per point; the whole memory win rests on
// that being six bytes, not eight. It is — the `wide` bool's niche encodes `None` — but the saving
// is load-bearing, so pin it: if a layout change ever costs the niche, this fails the build loudly
// rather than silently doubling the buffer.
const _: () = assert!(core::mem::size_of::<Option<ScreenPoint>>() == 6);

/// The uniform canvas-to-panel scale. Chosen by width — see the module docs — so the frond fills
/// the 135 columns and sits inside the 240 rows without distortion.
pub const SCALE: f32 = 0.75;

/// The frond's centre in canvas space, mapped to the centre of the panel. Measured from the field
/// over a full period, not guessed: the robust middle of the point cloud.
const CANVAS_CENTRE_X: f32 = 200.0;
/// The frond's vertical centre in canvas space — a little below the geometric middle, where the
/// body actually sits.
const CANVAS_CENTRE_Y: f32 = 203.0;

/// Where a field point lands on a `canvas`-shaped panel as a drawable [`ScreenPoint`], or `None` if
/// it is not a drawable coordinate.
///
/// `None` is returned only for a non-finite point — the field sends points to infinity at the
/// indices where `k` passes through zero, and an infinity is not a pixel. A merely *off-canvas*
/// finite point is still `Some`: it is a real coordinate, just past the edge, which the frame
/// clips. Keeping those two cases distinct is what stops an infinity being truncated into a stray
/// pixel at the origin.
///
/// The coordinates land in `i16`. Every drawable pixel is inside the panel, and the `f32`-to-`i16`
/// cast saturates (Rust's float-to-int cast has since 1.45), so a far-off finite point — the field
/// can throw one hundreds of units wide near a `k`-zero without reaching infinity — pins to the edge
/// of the type, still comfortably off-canvas and still clipped, exactly as the old wider `i32` held
/// it. The picture is unchanged; only the width of the carried coordinate is.
pub fn project(point: FieldPoint, canvas: Size) -> Option<ScreenPoint> {
    if !point.x.is_finite() || !point.y.is_finite() {
        return None;
    }
    let x: f32 = (point.x - CANVAS_CENTRE_X) * SCALE + canvas.width as f32 / 2.0;
    let y: f32 = (point.y - CANVAS_CENTRE_Y) * SCALE + canvas.height as f32 / 2.0;
    Some(ScreenPoint {
        x: x as i16,
        y: y as i16,
        wide: point.wide,
    })
}

/// Plot one already-projected frond point into `frame`, which the caller has reset to the ground.
///
/// The bare plot atom: light the pixel, and its right neighbour too if the point is fat — the
/// original's two-pixel dot that is the barbs' bright spine. No arithmetic; the projection was spent
/// upstream, in [`project`], so the point arrives ready to draw. *How* it was computed and projected
/// — one core or two — is the caller's business (see [`FrondCompute`](crate::FrondCompute)), and
/// this plot is identical either way.
///
/// One point at a time so a caller can stream a core's half straight through with no intermediate
/// buffer — the plume's answer to the ESP32's tight heap. Generic over the [`Canvas`] so the same
/// plot lands the frond in a host [`Frame`](crate::Frame) for the goldens and in the firmware's
/// wire-order buffer on the glass — the point does not know which, and cannot draw them differently.
pub fn plot_screen<C: Canvas>(frame: &mut C, point: ScreenPoint) {
    let x: i32 = point.x as i32;
    let y: i32 = point.y as i32;
    frame.set(x, y, PLUME_COLOUR);
    if point.wide {
        frame.set(x + 1, y, PLUME_COLOUR);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Frame;

    /// The 135×240 portrait canvas the plume is drawn on.
    const PORTRAIT: Size = Size::new(135, 240);

    /// The frond's centre lands at the centre of the panel — the whole point of the offset.
    #[test]
    fn the_centre_maps_to_the_panel_centre() {
        let centre: FieldPoint = FieldPoint {
            x: CANVAS_CENTRE_X,
            y: CANVAS_CENTRE_Y,
            wide: false,
        };
        assert_eq!(
            project(centre, PORTRAIT),
            Some(ScreenPoint {
                x: 67,
                y: 120,
                wide: false
            })
        );
    }

    /// An infinite point — the field's degenerate index — is dropped, not truncated to a pixel.
    #[test]
    fn an_infinite_point_is_dropped() {
        let gone: FieldPoint = FieldPoint {
            x: f32::INFINITY,
            y: 0.0,
            wide: false,
        };
        assert_eq!(project(gone, PORTRAIT), None);
    }

    /// A finite point past the edge still projects — it is a real coordinate for the frame to
    /// clip, not a non-coordinate to drop here.
    #[test]
    fn a_finite_off_canvas_point_still_projects() {
        let far: FieldPoint = FieldPoint {
            x: 5_000.0,
            y: 203.0,
            wide: false,
        };
        let point: ScreenPoint = project(far, PORTRAIT).expect("a finite point projects");
        assert!(
            point.x as u32 >= PORTRAIT.width,
            "expected off the right edge"
        );
    }

    /// Plot each of `points` into a fresh portrait frame and count the pixels lit — the plot's
    /// whole observable effect, projecting each first exactly as the renderer does.
    fn lit_after_plotting(points: &[FieldPoint]) -> usize {
        use platform_display::testing::Framebuffer;
        let mut frame: Frame = Frame::new();
        frame.reset(PORTRAIT, Rgb565::BLACK);
        points.iter().for_each(|&point: &FieldPoint| {
            if let Some(screen) = project(point, PORTRAIT) {
                plot_screen(&mut frame, screen);
            }
        });
        let mut fb: Framebuffer = Framebuffer::sized(PORTRAIT);
        frame.blit(&mut fb).expect("a framebuffer blit cannot fail");
        fb.lit_pixels()
    }

    /// Zero: the field's degenerate ∞ point lights nothing — plot drops it exactly as project does.
    #[test]
    fn an_infinite_point_lights_nothing() {
        let gone: FieldPoint = FieldPoint {
            x: f32::INFINITY,
            y: 0.0,
            wide: false,
        };
        assert_eq!(lit_after_plotting(&[gone]), 0);
    }

    /// One: a narrow point on the canvas lights exactly one pixel.
    #[test]
    fn a_narrow_point_lights_one_pixel() {
        let p: FieldPoint = FieldPoint {
            x: CANVAS_CENTRE_X,
            y: CANVAS_CENTRE_Y,
            wide: false,
        };
        assert_eq!(lit_after_plotting(&[p]), 1);
    }

    /// Many: a wide point lights two — itself and its right neighbour, the barbs' bright spine.
    #[test]
    fn a_wide_point_lights_its_neighbour_too() {
        let p: FieldPoint = FieldPoint {
            x: CANVAS_CENTRE_X,
            y: CANVAS_CENTRE_Y,
            wide: true,
        };
        assert_eq!(lit_after_plotting(&[p]), 2);
    }
}
