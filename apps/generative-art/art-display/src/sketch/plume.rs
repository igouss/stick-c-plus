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

/// The uniform canvas-to-panel scale. Chosen by width — see the module docs — so the frond fills
/// the 135 columns and sits inside the 240 rows without distortion.
pub const SCALE: f32 = 0.75;

/// The frond's centre in canvas space, mapped to the centre of the panel. Measured from the field
/// over a full period, not guessed: the robust middle of the point cloud.
const CANVAS_CENTRE_X: f32 = 200.0;
/// The frond's vertical centre in canvas space — a little below the geometric middle, where the
/// body actually sits.
const CANVAS_CENTRE_Y: f32 = 203.0;

/// Where a field point lands on a `canvas`-shaped panel, or `None` if it is not a drawable
/// coordinate.
///
/// `None` is returned only for a non-finite point — the field sends points to infinity at the
/// indices where `k` passes through zero, and an infinity is not a pixel. A merely *off-canvas*
/// finite point is still `Some`: it is a real coordinate, just past the edge, which the frame
/// clips. Keeping those two cases distinct is what stops an infinity being truncated into a stray
/// pixel at the origin.
pub fn project(point: FieldPoint, canvas: Size) -> Option<(i32, i32)> {
    if !point.x.is_finite() || !point.y.is_finite() {
        return None;
    }
    let x: f32 = (point.x - CANVAS_CENTRE_X) * SCALE + canvas.width as f32 / 2.0;
    let y: f32 = (point.y - CANVAS_CENTRE_Y) * SCALE + canvas.height as f32 / 2.0;
    Some((x as i32, y as i32))
}

/// Plot one computed frond point into `frame`, which the caller has already reset to the ground.
///
/// The pure projection-and-plot atom of the plume: project the point onto the canvas and, if it
/// lands, light a pixel — a fat point lighting its neighbour too, the original's two-pixel dot that
/// is the barbs' bright spine. The point is the frond already evaluated at the frame's phase; *how*
/// it was computed — one core or two — is the caller's business (see
/// [`FrondCompute`](crate::FrondCompute)), and this plot is identical either way.
///
/// One point at a time so a caller can stream a core's half straight from
/// [`iter_range`](plume_core::PlumeField::iter_range) with no intermediate buffer — the plume's
/// answer to the ESP32's tight heap.
///
/// Generic over the [`Canvas`] so the same plot lands the frond in a host [`Frame`](crate::Frame)
/// for the goldens and in the firmware's wire-order buffer on the glass — the point does not know
/// which, and cannot draw them differently.
pub fn plot_point<C: Canvas>(frame: &mut C, point: FieldPoint, canvas: Size) {
    if let Some((x, y)) = project(point, canvas) {
        frame.set(x, y, PLUME_COLOUR);
        if point.wide {
            frame.set(x + 1, y, PLUME_COLOUR);
        }
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
        assert_eq!(project(centre, PORTRAIT), Some((67, 120)));
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
        let (x, _y): (i32, i32) = project(far, PORTRAIT).expect("a finite point projects");
        assert!(x as u32 >= PORTRAIT.width, "expected off the right edge");
    }

    /// Plot each of `points` into a fresh portrait frame and count the pixels lit — the plot's
    /// whole observable effect.
    fn lit_after_plotting(points: &[FieldPoint]) -> usize {
        use platform_display::testing::Framebuffer;
        let mut frame: Frame = Frame::new();
        frame.reset(PORTRAIT, Rgb565::BLACK);
        points
            .iter()
            .for_each(|&point: &FieldPoint| plot_point(&mut frame, point, PORTRAIT));
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
