//! The wood's vocabulary: a point on the panel and a straight, tapering limb of wood.
//!
//! Both are in **normalised `[0, 1]` fractions** — `x` a fraction of the panel width, `y` a fraction
//! of its height, `y` growing *downward* as the display's does — because the tree is original and
//! authored for no particular pixel size. The [`skeleton`](crate::skeleton) builds the trunk and
//! boughs out of these, and the display scales them onto whatever panel it has.

/// A point on the panel, in `[0, 1]` fractions of the width and height.
///
/// The whole geometry — every bough sample, every frond anchor and every swaying frond point — is a
/// `Point2`. It is the tree's single spatial type, kept free of any pixel or `embedded-graphics`
/// notion so the domain never learns the panel's size.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Point2 {
    /// A fraction of the panel width, `0.0` at the left edge.
    pub x: f32,
    /// A fraction of the panel height, `0.0` at the top edge, growing downward.
    pub y: f32,
}

impl Point2 {
    /// A point at `(x, y)`.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// The point a fraction `t` of the way from `self` to `other`: `self` at `t = 0`, `other` at
    /// `t = 1`, a straight line between.
    ///
    /// The straight interpolation the boughs' Bézier and the frond-length ramp are built from, and
    /// the spread the rim points are laid out along. In the precise `a·(1 − t) + b·t` form, so the
    /// endpoints land *exactly* on `self` and `other` — the fork the trunk and boughs must share is
    /// then bit-identical, not merely close. Paid at startup only.
    pub fn lerp(self, other: Point2, t: f32) -> Point2 {
        let inv: f32 = 1.0 - t;
        Point2 {
            x: self.x * inv + other.x * t,
            y: self.y * inv + other.y * t,
        }
    }
}

/// One straight limb of the wood: a segment from `a` to `b`, its half-width shrinking from
/// `half_width_a` to `half_width_b` along it.
///
/// The trunk is a few fat, near-vertical limbs; a bough is a chain of thin ones arcing outward. Both
/// taper — the half-width (a fraction of the panel width) is largest at the root and least at the
/// tip — so the wood narrows as it climbs, the way real wood does. The display strokes each limb at
/// the pixel width the half-widths name; nothing here knows pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Segment {
    /// The limb's root end.
    pub a: Point2,
    /// The limb's tip end.
    pub b: Point2,
    /// Half the limb's drawn thickness at `a`, a fraction of the panel width.
    pub half_width_a: f32,
    /// Half the limb's drawn thickness at `b`, a fraction of the panel width.
    pub half_width_b: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zero: a lerp at `t = 0` is the start point — the anchored end of every interpolation.
    #[test]
    fn lerp_at_zero_is_the_start() {
        let a: Point2 = Point2::new(0.2, 0.8);
        let b: Point2 = Point2::new(0.9, 0.1);
        assert_eq!(a.lerp(b, 0.0), a);
    }

    /// One: a lerp at `t = 1` is the end point — the far end of every interpolation.
    #[test]
    fn lerp_at_one_is_the_end() {
        let a: Point2 = Point2::new(0.2, 0.8);
        let b: Point2 = Point2::new(0.9, 0.1);
        assert_eq!(a.lerp(b, 1.0), b);
    }

    /// A lerp at `t = 0.5` is the midpoint — a straight line, so half the way is half of each
    /// coordinate.
    #[test]
    fn lerp_halfway_is_the_midpoint() {
        let a: Point2 = Point2::new(0.0, 0.0);
        let b: Point2 = Point2::new(1.0, 0.4);
        assert_eq!(a.lerp(b, 0.5), Point2::new(0.5, 0.2));
    }
}
