//! The board's canvas geometry, and the two text primitives every app draws through.
//!
//! Facts about the *picture* on this panel, not about the panel: how wide the rotated
//! canvas is, and how a fixed-width field is drawn so a shorter value erases the longer
//! one it replaces. The panel's own facts — the CGRAM offset, the colour inversion, the
//! SPI pins — stay in the driven adapter, the only party that knows them.
//!
//! Neither primitive assumes a layout: they take an explicit `origin` and (for
//! [`text_line`]) a field `width`, so the plant monitor's two rows and the pomodoro
//! timer's clock are the same primitive placed differently — their metrics cannot drift.

use core::fmt::Write as _;

use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle, MonoTextStyleBuilder};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Baseline, Text};

use crate::error::RenderError;

/// The drawable canvas, in landscape — what a [`DrawTarget`] sees.
///
/// The M5StickC Plus panel is natively 135×240 (portrait); the adapter rotates it 90°,
/// so everything drawn here works in 240×135. This is the single source of truth for that
/// size: the adapter derives its own native `display_size` by swapping these axes back, so
/// the two can never drift apart.
pub const SCREEN_SIZE: Size = Size::new(240, 135);

/// The one font every text line is drawn in — 10×20 monospace ASCII. Exposed so an app's
/// layout arithmetic (row spacing, right-edge fit) references the *same* glyph metrics the
/// renderer uses, rather than a copy that could disagree.
pub const FONT: MonoFont<'static> = FONT_10X20;

/// Stack capacity of a rendered line: the widest field any app pads to, plus headroom,
/// so a line is built with no heap allocation on the render path.
pub const LINE_CAP: usize = 16;

/// A stack-allocated line buffer. The render path formats into this instead of a heap
/// `String`, so a redraw allocates nothing on the ESP32's scarce SRAM.
type Line = heapless::String<LINE_CAP>;

/// Draw one baseline-top text line at `origin`, padded to `width` characters with an
/// opaque black background so it overwrites its whole field in place.
///
/// The opaque background is the whole mechanism by which a shorter new value's trailing
/// spaces erase the leftover glyphs of a longer old one — so a redraw touches only its own
/// field, with no full-screen clear and therefore no flash. `width` must be less than
/// [`LINE_CAP`]; content that would exceed [`LINE_CAP`] is refused as
/// [`RenderError::LineOverflow`] rather than silently truncated.
pub fn text_line<D>(
    target: &mut D,
    origin: Point,
    color: Rgb565,
    width: usize,
    content: core::fmt::Arguments<'_>,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut line: Line = Line::new();
    line.write_fmt(content)
        .map_err(|_| RenderError::LineOverflow)?;
    // Pad to a fixed field so a shorter value fully erases the longer one it replaces.
    // `push` only fails at LINE_CAP, which a sane `width` is well inside.
    while line.len() < width && line.push(' ').is_ok() {}

    let style: MonoTextStyle<'_, Rgb565> = MonoTextStyleBuilder::new()
        .font(&FONT)
        .text_color(color)
        .background_color(Rgb565::BLACK)
        .build();
    Text::with_baseline(line.as_str(), origin, style, Baseline::Top)
        .draw(target)
        .map_err(RenderError::Draw)?;
    Ok(())
}

/// Draw a text line with a *transparent* background, over whatever is beneath.
///
/// The colour-check labels sit on top of their bands, so they must not paint a black box
/// around themselves — unlike [`text_line`], whose opaque background is the whole mechanism
/// by which a value erases its predecessor.
pub fn text_overlay<D>(
    target: &mut D,
    at: Point,
    color: Rgb565,
    label: &str,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let style: MonoTextStyle<'_, Rgb565> = MonoTextStyleBuilder::new()
        .font(&FONT)
        .text_color(color)
        .build();
    Text::with_baseline(label, at, style, Baseline::Top)
        .draw(target)
        .map_err(RenderError::Draw)?;
    Ok(())
}
