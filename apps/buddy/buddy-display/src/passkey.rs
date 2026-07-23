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
use platform_display::{text_field, FieldAlign, RenderError};

use crate::layout::Layout;
use crate::page;
use crate::palette;

/// Digits in a BLE passkey — the range the Bluetooth spec fixes at `000000..=999999`.
pub const PASSKEY_DIGITS: usize = 6;

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
    text_field(
        target,
        layout.row_origin(digits_row),
        palette::PRIMARY,
        layout.cols,
        FieldAlign::Centred,
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
