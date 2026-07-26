//! Colour helpers: turn a sketch's abstract colour (a hue, a brightness) into an [`Rgb565`] the
//! [`Canvas`](crate::Canvas) port speaks.
//!
//! The domains deal in colour as a *quantity* — the fan's hue is a distance on `[0, 1)` — never in
//! a panel's pixel encoding. This module is the seam that turns those quantities into [`Rgb565`],
//! so the panel's colour depth is a fact of the adapter and never of the domain. The panel is
//! driven in 12-bit RGB444 on the board, but the [`Canvas`](crate::Canvas) port is [`Rgb565`] and
//! the wire adapter narrows it, so full `Rgb565` precision is produced here.

use embedded_graphics::pixelcolor::Rgb565;

/// The hue wheel as a full-saturation, full-value [`Rgb565`], for a hue in `[0, 1)`.
///
/// The `HSB(1)` colour the fan is authored in with `S = V = 1`: the hue alone names the colour. The
/// wheel is the six-segment ramp — red, yellow, green, cyan, blue, magenta and back — so a hue
/// sweeping `0 → 1` sweeps the spectrum once. A hue at or past `1.0` (which the fan's wrap should
/// never produce, but a caller might) folds back onto the wheel rather than running off its end.
pub fn hsv_to_rgb565(hue: f32) -> Rgb565 {
    let wrapped: f32 = hue - libm::floorf(hue); // onto [0, 1)
    let sector_pos: f32 = wrapped * 6.0;
    let sector: i32 = libm::floorf(sector_pos) as i32;
    let frac: f32 = sector_pos - sector as f32;

    // Full S and V collapse the general HSB→RGB to a ramp of `frac` and `1 - frac` between the two
    // channels the current sector rides; the third is pinned at 0 or 1.
    let rise: f32 = frac;
    let fall: f32 = 1.0 - frac;
    let (r, g, b): (f32, f32, f32) = match sector {
        0 => (1.0, rise, 0.0),
        1 => (fall, 1.0, 0.0),
        2 => (0.0, 1.0, rise),
        3 => (0.0, fall, 1.0),
        4 => (rise, 0.0, 1.0),
        // Sector 5 and the `wrapped == 0` degenerate that `floorf` can round to 6 both land here;
        // at the wrap boundary `frac` is ~0, so this is red either way, matching sector 0.
        _ => (1.0, 0.0, fall),
    };
    rgb565(r, g, b)
}

/// A neutral grey as an [`Rgb565`], for a brightness `level` in `[0, 1]`.
///
/// Equal parts of all three channels, so `0.0` is black, `1.0` is white, and between them a true
/// grey — the ramp the orbits sketch's `fill(c · noise)` wants, where a cell's colour is a single
/// brightness and not a hue. A `level` outside `[0, 1]` is clamped by [`rgb565`], so a caller's
/// over-range brightness saturates at black or white rather than wrapping.
pub fn grey_to_rgb565(level: f32) -> Rgb565 {
    rgb565(level, level, level)
}

/// Blend between two `[0, 1]` RGB colours by `t`, as an [`Rgb565`] — `from` at `t = 0`, `to` at
/// `t = 1`, a straight line per channel between.
///
/// The ramp a sketch wears along a gradient: the willow's tendrils run from a deep green root to a
/// light tip as `t` (a point's depth) climbs `0 → 1`. Blended in full `f32` channels and quantised
/// once at the end, so the ramp is smooth rather than stepping between two coarse `Rgb565`
/// endpoints. `t` outside `[0, 1]` is clamped, so a caller's over-range gradient parameter rests at
/// an endpoint rather than overshooting the colours.
pub fn blend(from: (f32, f32, f32), to: (f32, f32, f32), t: f32) -> Rgb565 {
    let t: f32 = t.clamp(0.0, 1.0);
    let lerp = |a: f32, b: f32| -> f32 { a + (b - a) * t };
    rgb565(lerp(from.0, to.0), lerp(from.1, to.1), lerp(from.2, to.2))
}

/// Pack three `[0, 1]` channel intensities into an [`Rgb565`], rounding each to its field width
/// (five bits of red and blue, six of green).
fn rgb565(r: f32, g: f32, b: f32) -> Rgb565 {
    let quantise =
        |channel: f32, max: f32| -> u8 { libm::roundf(channel.clamp(0.0, 1.0) * max) as u8 };
    Rgb565::new(quantise(r, 31.0), quantise(g, 63.0), quantise(b, 31.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics::prelude::RgbColor;

    /// Zero: hue zero is pure red — the fan's centre, and the wheel's origin.
    #[test]
    fn hue_zero_is_red() {
        assert_eq!(hsv_to_rgb565(0.0), Rgb565::RED);
    }

    /// One: a third of the way round the wheel is pure green — the primary at sector two's start.
    #[test]
    fn hue_a_third_is_green() {
        assert_eq!(hsv_to_rgb565(1.0 / 3.0), Rgb565::GREEN);
    }

    /// Two-thirds round is pure blue — the last primary, so the three equal thirds of the wheel are
    /// the three primaries.
    #[test]
    fn hue_two_thirds_is_blue() {
        assert_eq!(hsv_to_rgb565(2.0 / 3.0), Rgb565::BLUE);
    }

    /// The wheel wraps: a hue one full turn on is the same colour as the hue itself, so a caller's
    /// out-of-range hue never runs off the end of the ramp.
    #[test]
    fn the_wheel_wraps_at_one() {
        assert_eq!(hsv_to_rgb565(0.2), hsv_to_rgb565(1.2));
    }

    /// Zero: no brightness is black — the orbits ground, where no diamond reaches.
    #[test]
    fn grey_zero_is_black() {
        assert_eq!(grey_to_rgb565(0.0), Rgb565::BLACK);
    }

    /// One: full brightness is white — a cell saturated at the source's fill ceiling.
    #[test]
    fn grey_one_is_white() {
        assert_eq!(grey_to_rgb565(1.0), Rgb565::WHITE);
    }

    /// A grey is truly grey — its three channels sit at the same fraction of their fields, so it has
    /// no colour cast, only a brightness. Half brightness reaches roughly half of each field.
    #[test]
    fn a_grey_has_no_colour_cast() {
        let half: Rgb565 = grey_to_rgb565(0.5);
        assert_eq!(half.r(), 16, "red {}", half.r());
        assert_eq!(half.g(), 32, "green {}", half.g());
        assert_eq!(half.b(), 16, "blue {}", half.b());
    }

    /// Zero: a blend at `t = 0` is the `from` colour — the willow's root end of its ramp.
    #[test]
    fn blend_at_zero_is_from() {
        assert_eq!(blend((1.0, 0.0, 0.0), (0.0, 0.0, 1.0), 0.0), Rgb565::RED);
    }

    /// One: a blend at `t = 1` is the `to` colour — the willow's tip end of its ramp.
    #[test]
    fn blend_at_one_is_to() {
        assert_eq!(blend((1.0, 0.0, 0.0), (0.0, 0.0, 1.0), 1.0), Rgb565::BLUE);
    }

    /// A blend sits between its endpoints: halfway from black to white reaches roughly half of each
    /// field, the same midpoint a grey does — so the ramp is a true interpolation, not a jump.
    #[test]
    fn blend_halfway_is_the_midpoint() {
        let mid: Rgb565 = blend((0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 0.5);
        assert_eq!((mid.r(), mid.g(), mid.b()), (16, 32, 16));
    }
}
