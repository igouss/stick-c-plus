//! Every colour the buddy's glass uses, named once.
//!
//! Named here rather than at each call site so a screen is described by *what a colour means*
//! — the newest transcript line, a prompt gone cold, a disabled entry — and two screens cannot
//! drift into two slightly different greys.
//!
//! ## Colour is not re-derived here
//!
//! These are plain [`Rgb565`] values handed to [`platform_display`], which is already on
//! `ColorOrder::Rgb`, and the sprite palettes are quantized to `Rgb565` at generation time.
//! There is no channel swap anywhere in this crate, and there must never be one: a wrong
//! colour order is a property of the panel adapter, fixed in exactly one place. See
//! `kb/findings/st7789-wants-rgb-colour-order`.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;

/// The canvas behind everything. The sprite renderer paints its own background against this,
/// and every text field's opaque background is this colour.
pub const BACKGROUND: Rgb565 = Rgb565::BLACK;

/// A heading — the screen's own name, and a page title.
pub const TITLE: Rgb565 = Rgb565::CYAN;

/// The primary reading on a screen: the newest transcript line, a stat's value, the passkey.
pub const PRIMARY: Rgb565 = Rgb565::WHITE;

/// Something present but past: an older transcript line, a page's secondary text.
///
/// A mid grey rather than a dark one. The dim entries still have to be *readable* on a 240×135
/// panel at arm's length — dim means "not the newest", not "not for reading".
pub const DIM: Rgb565 = Rgb565::new(14, 28, 14);

/// A label naming the value beside it.
pub const LABEL: Rgb565 = Rgb565::new(18, 36, 18);

/// A prompt that has just arrived and is still inside its comfortable window.
pub const PROMPT_WARM: Rgb565 = Rgb565::YELLOW;

/// A prompt that has been waiting long enough to be worth hurrying — the hot state.
pub const PROMPT_HOT: Rgb565 = Rgb565::RED;

/// The approve action, on the A button.
pub const APPROVE: Rgb565 = Rgb565::GREEN;

/// The deny action, on the B button.
pub const DENY: Rgb565 = Rgb565::RED;

/// A live link to the bridge.
pub const LINKED: Rgb565 = Rgb565::GREEN;

/// No link to the bridge — the buddy is on its own.
pub const UNLINKED: Rgb565 = Rgb565::new(18, 18, 8);

/// A lit cell of a meter — a mood heart, a fed pip, an energy bar.
pub const METER_LIT: Rgb565 = Rgb565::new(0, 50, 20);

/// An unlit cell of a meter. Drawn rather than left blank, so a meter's *length* is legible
/// and a reading of two out of five does not look like a reading of two out of two.
pub const METER_DARK: Rgb565 = Rgb565::new(6, 12, 6);

/// The mood meter's lit cells — hearts, so red.
pub const MOOD_LIT: Rgb565 = Rgb565::new(28, 6, 10);

/// The fed meter's lit cells.
pub const FED_LIT: Rgb565 = Rgb565::new(31, 44, 0);

/// The scroll indicator's track — how much transcript there is.
pub const SCROLL_TRACK: Rgb565 = Rgb565::new(5, 10, 5);

/// The scroll indicator's thumb — how much of it is on the glass.
pub const SCROLL_THUMB: Rgb565 = Rgb565::new(20, 40, 20);

/// A destructive confirmation — the reset overlay's frame and heading.
pub const DANGER: Rgb565 = Rgb565::RED;
