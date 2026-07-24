//! Fitting the frond to the glass: the projection from the field's 400×400 canvas to the
//! panel's pixels.
//!
//! The field ([`plume_core`]) lives in the original's 400×400 space, where the frond is centred
//! near `(200, 203)` and stands about 226 tall — a portrait shape, which is the M5StickC Plus's
//! shape on end. So the projection is a single uniform scale about the frond's centre, landing
//! it in the middle of the panel: the same scale on both axes, so the frond keeps its
//! proportions rather than being stretched to fill a differently-shaped canvas.
//!
//! [`SCALE`] is set by the width, the binding dimension: at 0.75 the frond's ~180-wide body
//! spans the panel's 135 columns with its outermost barbs grazing the edge, and its height
//! lands comfortably inside the 240 rows. A point that still falls outside — a flung barb tip, a
//! point sent to infinity by a near-zero divisor — is reported and dropped by the canvas, the
//! same nothing the original's off-screen `circle` draws.

use embedded_graphics::prelude::Size;
use plume_core::FieldPoint;

/// The uniform canvas-to-panel scale. Chosen by width — see the module docs — so the frond
/// fills the 135 columns and sits inside the 240 rows without distortion.
pub const SCALE: f32 = 0.75;

/// The frond's centre in canvas space, mapped to the centre of the panel. Measured from the
/// field over a full period, not guessed: the robust middle of the point cloud.
const CANVAS_CENTRE_X: f32 = 200.0;
/// The frond's vertical centre in canvas space — a little below the geometric middle, where the
/// body actually sits.
const CANVAS_CENTRE_Y: f32 = 203.0;

/// Where a field point lands on a `canvas`-shaped panel, or `None` if it is not a drawable
/// coordinate.
///
/// `None` is returned only for a non-finite point — the field sends points to infinity at the
/// indices where `k` passes through zero, and an infinity is not a pixel. A merely *off-canvas*
/// finite point is still `Some`: it is a real coordinate, just past the edge, and the canvas
/// clips it. Keeping those two cases distinct is what stops an infinity being truncated into a
/// stray pixel at the origin.
pub fn project(point: FieldPoint, canvas: Size) -> Option<(i32, i32)> {
    if !point.x.is_finite() || !point.y.is_finite() {
        return None;
    }
    let x: f32 = (point.x - CANVAS_CENTRE_X) * SCALE + canvas.width as f32 / 2.0;
    let y: f32 = (point.y - CANVAS_CENTRE_Y) * SCALE + canvas.height as f32 / 2.0;
    Some((x as i32, y as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// A finite point past the edge still projects — it is a real coordinate for the canvas to
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
}
