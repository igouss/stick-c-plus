//! Colors — the domain's palette primitives.
//!
//! Deliberately hand-rolled rather than pulled from the `rgb`/`smart-leds`
//! crates: the domain must not depend on the driver ecosystem. Adapters convert
//! [`Rgb`] to whatever the wire format needs at the boundary.

/// An 8-bit-per-channel RGB color — the value a strip pixel holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// The unlit pixel.
    pub const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };

    /// A color from its channels.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// Hue / Saturation / Value.
///
/// `hue` spans the full color wheel as a `u8` (`0..=255` ≙ `0..360°`), matching
/// the 8-bit hue convention FastLED and NightDriver use, so hue arithmetic
/// wraps for free.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Hsv {
    pub hue: u8,
    pub sat: u8,
    pub val: u8,
}

impl Hsv {
    /// A color in HSV space.
    pub const fn new(hue: u8, sat: u8, val: u8) -> Self {
        Self { hue, sat, val }
    }
}

impl From<Hsv> for Rgb {
    fn from(hsv: Hsv) -> Self {
        hsv_to_rgb(hsv)
    }
}

/// Convert HSV to RGB with integer math only (no `libm`, no FPU dependency).
///
/// Six-sector spectrum conversion. Two invariants hold and are property-tested:
/// - the brightest channel equals `val`,
/// - the dimmest channel equals `val * (255 - sat) / 255`.
///
/// `sat == 0` short-circuits to grayscale `(val, val, val)`.
pub fn hsv_to_rgb(hsv: Hsv) -> Rgb {
    let Hsv { hue, sat, val } = hsv;

    if sat == 0 {
        return Rgb::new(val, val, val);
    }

    // Six sectors of ~43 hue units each; `remainder` is the position within the
    // sector, scaled to 0..=255.
    let region: u8 = hue / 43;
    let remainder: u16 = ((hue - region * 43) as u16) * 6;

    let (val, sat) = (val as u16, sat as u16);
    let p: u8 = (val * (255 - sat) / 255) as u8;
    let q: u8 = (val * (255 - sat * remainder / 255) / 255) as u8;
    let t: u8 = (val * (255 - sat * (255 - remainder) / 255) / 255) as u8;
    let val: u8 = val as u8;

    match region {
        0 => Rgb::new(val, t, p),
        1 => Rgb::new(q, val, p),
        2 => Rgb::new(p, val, t),
        3 => Rgb::new(p, q, val),
        4 => Rgb::new(t, p, val),
        _ => Rgb::new(val, p, q),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn black_is_all_zero() {
        assert_eq!(Rgb::BLACK, Rgb::new(0, 0, 0));
    }

    #[test]
    fn zero_saturation_is_grayscale() {
        assert_eq!(hsv_to_rgb(Hsv::new(200, 0, 123)), Rgb::new(123, 123, 123));
    }

    #[test]
    fn full_value_full_sat_hue_zero_is_red() {
        assert_eq!(hsv_to_rgb(Hsv::new(0, 255, 255)), Rgb::new(255, 0, 0));
    }

    proptest! {
        /// The rule: value is the peak channel.
        #[test]
        fn value_is_the_peak_channel(hue in 0u8..=255, sat in 1u8..=255, val in 0u8..=255) {
            let c = hsv_to_rgb(Hsv::new(hue, sat, val));
            prop_assert_eq!(c.r.max(c.g).max(c.b), val);
        }

        /// The rule: saturation sets the floor equally on the dim channel.
        #[test]
        fn saturation_sets_the_floor(hue in 0u8..=255, sat in 1u8..=255, val in 0u8..=255) {
            let c = hsv_to_rgb(Hsv::new(hue, sat, val));
            let floor = (val as u16 * (255 - sat as u16) / 255) as u8;
            prop_assert_eq!(c.r.min(c.g).min(c.b), floor);
        }
    }
}
