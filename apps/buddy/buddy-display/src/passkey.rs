//! The pairing passkey takeover.
//!
//! Whenever a passkey is active it is the **only** thing on the glass. Not a banner, not an
//! overlay over the creature — the whole panel. The peer is waiting to be told six digits, the
//! window is seconds long, and anything else on the screen at that moment is competing with the
//! one piece of information the device exists to show.
//!
//! ## Why there is a passkey screen at all
//!
//! The BLE spike used a fixed passkey so pairing was reproducible across flashes. That is fatal
//! in the product: a static, published number defeats the MITM protection LE Secure Connections
//! bonding exists to provide — an attacker who knows the constant can complete the pairing. The
//! real firmware generates a fresh random passkey per pairing, and it can only do that because
//! there is somewhere to *show* it. This screen is what makes the random passkey possible.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_display::{text_field, FieldAlign, Magnified, RenderError, FONT};

use crate::layout::Layout;
use crate::page;
use crate::palette;

/// Digits in a BLE passkey — the range the Bluetooth spec fixes at `000000..=999999`.
pub const PASSKEY_DIGITS: usize = 6;

/// The width one row of magnified digits needs, at scale 1.
const DIGIT_SPAN: u32 = FONT.character_size.width * PASSKEY_DIGITS as u32;

/// The largest magnification the digits are ever drawn at.
///
/// Capped rather than "as big as the width allows", because the digits share the screen with a
/// title above and a hint below: the wide canvas has room for 4× across but only ~95 px of
/// height between those two rows, and 4× is 80 px of glyph. Three leaves the picture breathing.
const MAX_DIGIT_SCALE: u32 = 3;

/// How much larger than body text the digits are drawn on `canvas`.
///
/// Not a decoration, and not a constant. The board's one font is `FONT_10X20`, which put six
/// digits in a seventh of the panel's height — reported from the glass as hard to read, and this
/// is the one screen where that is fatal rather than untidy: the owner reads a number across a
/// desk and types it on another machine before BlueZ closes a thirty-second window without
/// asking.
///
/// Derived from the canvas rather than written down per shape, so it cannot overflow by
/// construction: the wide canvas takes [`MAX_DIGIT_SCALE`], the narrow one takes the 2× that
/// fits its 135 px, and a future panel gets the right answer without editing this file.
const fn digit_scale(canvas: Size) -> u32 {
    let fits: u32 = canvas.width / DIGIT_SPAN;
    if fits < 1 {
        1
    } else if fits > MAX_DIGIT_SCALE {
        MAX_DIGIT_SCALE
    } else {
        fits
    }
}

/// Both canvases really do magnify, and neither overflows — checked at build time rather than
/// discovered on the glass, which is how the first attempt at this (a flat 3×, 180 px of digits
/// on a 135 px canvas) was caught.
const _: () = {
    let wide: Size = platform_display::SCREEN_SIZE;
    let narrow: Size = Size::new(wide.height, wide.width);
    assert!(
        digit_scale(wide) * DIGIT_SPAN <= wide.width,
        "the magnified passkey runs off the wide canvas"
    );
    assert!(
        digit_scale(narrow) * DIGIT_SPAN <= narrow.width,
        "the magnified passkey runs off the narrow canvas"
    );
    assert!(
        digit_scale(narrow) > 1,
        "the narrow canvas gained no magnification at all — the whole point of this screen"
    );
};

/// Draw the passkey takeover.
pub fn render<D>(target: &mut D, layout: &Layout, passkey: u32) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    // The digits sit on the middle row of the canvas, centred whichever way up the board is —
    // the one screen where the landscape layout's usual flush-left rule is wrong, because there
    // is nothing beside the number to align it against.
    let digits_row: usize = layout.rows / 2;

    page::blank(target, layout, 0, layout.rows)?;
    text_field(
        target,
        layout.row_origin(0),
        palette::TITLE,
        layout.cols,
        FieldAlign::Centred,
        format_args!("PAIRING"),
    )?;

    // The digits, several times body size, through a magnifying target — so the field padding,
    // the overflow check and the glyph rendering are the same code every other row uses.
    //
    // The field is `PASSKEY_DIGITS` wide rather than `layout.cols`: a full-width field would be
    // several times the canvas once magnified. The number is a fixed six characters, so the field
    // is exactly the value and the centring is done here, in target space, where the magnified
    // width is known. `page::blank` above has already cleared the row, so nothing is relying on
    // the padding to erase a previous value.
    let scale: u32 = digit_scale(layout.canvas);
    let across: u32 = layout.canvas.width.saturating_sub(DIGIT_SPAN * scale) / 2;
    // Grown about its own centre line, so the digits stay on the row the layout chose rather
    // than sliding down the canvas as the scale goes up.
    let lift: i32 = (FONT.character_size.height * (scale - 1) / 2) as i32;
    let digits_origin: Point = Point::new(
        across as i32,
        (layout.row_origin(digits_row).y - lift).max(0),
    );
    let mut big: Magnified<'_, D> = Magnified::new(target, scale, digits_origin);
    text_field(
        &mut big,
        Point::new(0, 0),
        palette::PRIMARY,
        PASSKEY_DIGITS,
        FieldAlign::Left,
        // Zero-padded: a passkey of 42 is typed as `000042`, and a bare `42` would be typed
        // wrong by everyone who has not read the spec.
        format_args!("{:0width$}", passkey % 1_000_000, width = PASSKEY_DIGITS),
    )?;
    text_field(
        target,
        layout.row_origin(layout.rows - 1),
        palette::DIM,
        layout.cols,
        FieldAlign::Centred,
        format_args!("type on host"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{LANDSCAPE, PORTRAIT};
    use platform_display::testing::Framebuffer;

    fn painted(layout: &Layout, passkey: u32) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::sized(layout.canvas);
        render(&mut fb, layout, passkey).expect("a framebuffer render cannot fail");
        fb
    }

    /// One: a passkey reaches the glass.
    #[test]
    fn a_passkey_paints_pixels() {
        assert!(painted(&LANDSCAPE, 482_913).lit_pixels() > 0);
    }

    /// The digits are drawn LARGER than body text, on both canvases.
    ///
    /// The defect this screen was reported with: at body size the six digits were "a little
    /// difficult to read", and a passkey nobody can read has not been shown at all. A golden
    /// pins the arrangement and cannot pin this — so it is asserted as the property it is,
    /// against the same font the rest of the screen uses.
    #[test]
    fn the_digits_are_larger_than_body_text() {
        assert!(
            digit_scale(LANDSCAPE.canvas) > 1,
            "landscape did not magnify"
        );
        assert!(digit_scale(PORTRAIT.canvas) > 1, "portrait did not magnify");
        // The wide canvas can afford more than the narrow one, and takes it.
        assert!(digit_scale(LANDSCAPE.canvas) > digit_scale(PORTRAIT.canvas));
    }

    /// A magnified passkey still fits: nothing runs off either canvas, for every digit shape.
    ///
    /// `000000` and `888888` are the extremes of ink, and a passkey is exactly six characters
    /// wide whatever its value — so a picture that escapes for one escapes for all, and this
    /// pins the edge of the field rather than the edge of one number.
    #[test]
    fn a_magnified_passkey_stays_on_both_canvases() {
        assert_eq!(painted(&LANDSCAPE, 0).escaped(), 0);
        assert_eq!(painted(&PORTRAIT, 0).escaped(), 0);
        assert_eq!(painted(&LANDSCAPE, 888_888).escaped(), 0);
        assert_eq!(painted(&PORTRAIT, 888_888).escaped(), 0);
        assert_eq!(painted(&PORTRAIT, 999_999).escaped(), 0);
    }

    /// Magnifying multiplied the ink rather than merely moving it — the digits really are
    /// bigger, by the square of the scale, which is what "larger" has to mean on a panel.
    #[test]
    fn magnifying_multiplies_the_ink_in_the_digits() {
        // The title and hint rows are drawn identically at both scales, so the growth in lit
        // pixels is the digits' alone — compare a canvas against the same canvas' own digits.
        let landscape: usize = painted(&LANDSCAPE, 482_913).lit_pixels();
        let portrait: usize = painted(&PORTRAIT, 482_913).lit_pixels();
        assert!(
            landscape > portrait,
            "the wide canvas magnifies more, so it must carry more ink in the same six digits"
        );
    }

    /// Many: two passkeys are two different pictures — the digits actually come from the
    /// argument, which is the whole point of a per-pairing random key.
    #[test]
    fn two_passkeys_paint_differently() {
        assert_ne!(
            painted(&LANDSCAPE, 482_913).pixels(),
            painted(&LANDSCAPE, 482_914).pixels()
        );
    }

    /// Zero: a passkey with leading zeros is padded to six digits, so the owner types what the
    /// peer is expecting rather than a short number.
    #[test]
    fn a_small_passkey_is_padded_to_six_digits() {
        // `000042` and `000420` differ only in the padding; if the digits were not padded they
        // would be drawn as `42` and `420` and this would still pass — so also pin that the
        // padded picture is NOT the picture of the unpadded value drawn elsewhere.
        assert_ne!(
            painted(&LANDSCAPE, 42).pixels(),
            painted(&LANDSCAPE, 420).pixels()
        );
        assert_ne!(
            painted(&LANDSCAPE, 42).pixels(),
            painted(&LANDSCAPE, 420_000).pixels()
        );
    }

    /// Nothing escapes either canvas, at the largest passkey.
    #[test]
    fn the_takeover_stays_on_both_canvases() {
        assert_eq!(painted(&LANDSCAPE, 999_999).escaped(), 0);
        assert_eq!(painted(&PORTRAIT, 999_999).escaped(), 0);
    }
}
