//! The overlays: the menu, a settings entry, and the reset confirmation.
//!
//! Drawn as a bordered panel one character in from every edge, so a frame of the screen
//! underneath stays visible on all four sides. That frame is the whole cue: it reads as
//! *something is over this*, where a full-bleed redraw would read as having navigated away.
//!
//! At most one overlay is on the glass, and which one is not a taste — see
//! [`Overlay::priority`](crate::view::Overlay::priority). They nest: reset is opened from
//! settings and settings from the menu, so the innermost is the one the owner is looking at, and
//! a compositor that drew the menu over its own confirmation dialog would hide the question it
//! was asking.
//!
//! ## The menu scrolls
//!
//! There are five entries and the landscape panel has three rows for them. So the entry list is
//! a **window** around the cursor rather than the first N: the cursor is always on the glass,
//! which is the one property a menu you cannot see all of has to have.

use buddy_core::{MenuEntry, MENU_ENTRIES};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use platform_display::{text_field, FieldAlign, RenderError};

use crate::backdrop;
use crate::layout::Layout;
use crate::page;
use crate::palette;
use crate::view::{BuddyView, Overlay};

/// Draw whatever overlay `view` has open. `Overlay::None` draws nothing at all.
pub fn render<D>(
    target: &mut D,
    layout: &Layout,
    view: &BuddyView,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    match view.overlay {
        Overlay::None => Ok(()),
        Overlay::Menu { cursor } => {
            let inner: Layout = frame(target, layout, palette::TITLE)?;
            menu(target, &inner, cursor)
        }
        Overlay::Settings { entry } => {
            let inner: Layout = frame(target, layout, palette::TITLE)?;
            settings(target, &inner, view, entry)
        }
        Overlay::Reset => {
            let inner: Layout = frame(target, layout, palette::DANGER)?;
            reset(target, &inner)
        }
    }
}

/// Paint the panel — its border, and its background everywhere its own rows are not — and return
/// the inset grid to draw on.
///
/// The background is what hides the screen underneath; without it the overlay's text would be
/// drawn *through* a creature. But it is painted **around** the rows rather than under them: an
/// overlay sits over the home screen, which repaints on the creature's animation clock, so a
/// panel that cleared itself and then wrote its text would blink its whole contents twenty times
/// a second for as long as the menu was open.
///
/// Every row of the inset grid is written by the caller — [`page::lines`] blanks the ones its
/// text does not reach — so the whole row band is spoken for, and the backdrop owes it nothing.
fn frame<D>(
    target: &mut D,
    layout: &Layout,
    border: Rgb565,
) -> Result<Layout, RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let (at, size): (Point, Size) = layout.panel();
    let inner: Layout = layout.inset();

    Rectangle::new(at, size)
        .into_styled(PrimitiveStyle::with_stroke(border, 1))
        .draw(target)
        .map_err(RenderError::Draw)?;

    // Inside the border, never over it: the ring is one pixel and the backdrop starts after it.
    let interior: Rectangle = Rectangle::new(
        at + Point::new(1, 1),
        Size::new(size.width.saturating_sub(2), size.height.saturating_sub(2)),
    );
    backdrop::behind(target, interior, inner.all_rows(), palette::BACKGROUND)?;

    Ok(inner)
}

/// The settings menu: a title, then a window of entries with the cursor highlighted.
fn menu<D>(target: &mut D, inner: &Layout, cursor: u8) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_field(
        target,
        inner.row_origin(0),
        palette::TITLE,
        inner.cols,
        inner.align,
        format_args!("MENU"),
    )?;

    let visible: usize = (inner.rows - 1).min(MENU_ENTRIES.len());
    let first: usize = window_start(usize::from(cursor), visible, MENU_ENTRIES.len());

    MENU_ENTRIES
        .iter()
        .enumerate()
        .skip(first)
        .take(visible)
        .try_for_each(|(index, entry): (usize, &MenuEntry)| {
            let selected: bool = index == usize::from(cursor);
            let colour: Rgb565 = if selected {
                palette::PRIMARY
            } else {
                palette::DIM
            };
            text_field(
                target,
                inner.row_origin(1 + index - first),
                colour,
                inner.cols,
                FieldAlign::Left,
                // The marker, not just a colour: a cursor that existed only as a shade would be
                // invisible on a photograph, in a golden, and to anyone reading the panel at an
                // angle.
                format_args!(
                    "{} {}",
                    if selected { ">" } else { " " },
                    entry_word(*entry)
                ),
            )
        })?;

    page::blank(target, inner, 1 + visible, inner.rows - 1 - visible)
}

/// The first entry of the window that keeps `cursor` on the glass.
///
/// Pure arithmetic, and separately tested: an off-by-one here is a cursor that vanishes at one
/// end of the list, which is exactly the bug a "the menu paints pixels" test sails past.
pub fn window_start(cursor: usize, visible: usize, total: usize) -> usize {
    if visible >= total {
        return 0;
    }
    // Keep the cursor inside the window, then clamp the window inside the list.
    cursor.saturating_sub(visible - 1).min(total - visible)
}

/// The word for a menu entry, inside the narrow panel's columns.
const fn entry_word(entry: MenuEntry) -> &'static str {
    match entry {
        MenuEntry::Species => "Creature",
        MenuEntry::Owner => "Owner",
        MenuEntry::Status => "Status",
        MenuEntry::Unpair => "Unpair",
        MenuEntry::Close => "Close",
    }
}

/// A settings entry opened: its name, and what it currently says.
fn settings<D>(
    target: &mut D,
    inner: &Layout,
    view: &BuddyView,
    entry: MenuEntry,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_field(
        target,
        inner.row_origin(0),
        palette::TITLE,
        inner.cols,
        inner.align,
        format_args!("{}", entry_word(entry)),
    )?;

    let rows: usize = inner.rows - 1;
    match entry {
        MenuEntry::Species => {
            text_field(
                target,
                inner.row_origin(1),
                palette::PRIMARY,
                inner.cols,
                inner.align,
                format_args!("#{}", view.species.get()),
            )?;
            page::lines(target, inner, 2, rows - 1, "B cycles the creature.")
        }
        MenuEntry::Owner => {
            text_field(
                target,
                inner.row_origin(1),
                palette::PRIMARY,
                inner.cols,
                inner.align,
                format_args!("{}", view.device.owner),
            )?;
            page::lines(target, inner, 2, rows - 1, "Set from the host.")
        }
        MenuEntry::Status => {
            text_field(
                target,
                inner.row_origin(1),
                palette::PRIMARY,
                inner.cols,
                inner.align,
                format_args!("{}", view.device.name),
            )?;
            page::lines(
                target,
                inner,
                2,
                rows - 1,
                if view.device.linked {
                    "Bridge link up."
                } else {
                    "No bridge link."
                },
            )
        }
        MenuEntry::Unpair => page::lines(target, inner, 1, rows, "B forgets the bond."),
        MenuEntry::Close => page::lines(target, inner, 1, rows, "B closes the menu."),
    }
}

/// The reset confirmation — the one destructive thing the device can be asked to do.
///
/// Both buttons are labelled, and the safe one is named as well as the dangerous one: a dialog
/// that only says how to confirm is a dialog people confirm.
fn reset<D>(target: &mut D, inner: &Layout) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_field(
        target,
        inner.row_origin(0),
        palette::DANGER,
        inner.cols,
        inner.align,
        // A question, because the buttons underneath are YES and NO. The object is on the line
        // below; the upright panel holds eleven characters, and "FORGET BOND?" is twelve.
        format_args!("FORGET?"),
    )?;
    page::lines(
        target,
        inner,
        1,
        inner.rows - 2,
        "The host must pair again.",
    )?;
    let last: usize = inner.rows - 1;
    let half: usize = inner.cols / 2;
    let origin: Point = inner.row_origin(last);
    text_field(
        target,
        origin,
        palette::DENY,
        half,
        FieldAlign::Left,
        format_args!("A YES"),
    )?;
    text_field(
        target,
        origin
            + Point::new(
                (platform_display::FONT.character_size.width * half as u32) as i32,
                0,
            ),
        palette::APPROVE,
        inner.cols - half,
        FieldAlign::Left,
        format_args!("B NO"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{LANDSCAPE, PORTRAIT};
    use buddy_core::SpeciesIndex;
    use platform_display::testing::Framebuffer;

    fn view(overlay: Overlay) -> BuddyView {
        let mut view: BuddyView = BuddyView::resting(SpeciesIndex::new(0));
        view.overlay = overlay;
        view
    }

    fn painted(layout: &Layout, overlay: Overlay) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::sized(layout.canvas);
        render(&mut fb, layout, &view(overlay)).expect("a framebuffer render cannot fail");
        fb
    }

    /// Zero: no overlay draws nothing at all, so the screen underneath is the whole picture.
    #[test]
    fn no_overlay_draws_nothing() {
        assert_eq!(painted(&LANDSCAPE, Overlay::None).lit_pixels(), 0);
    }

    /// One: an open overlay paints.
    #[test]
    fn an_open_overlay_paints_pixels() {
        assert!(painted(&LANDSCAPE, Overlay::Menu { cursor: 0 }).lit_pixels() > 0);
    }

    /// The panel leaves the screen underneath showing around its edge — the cue that says
    /// "something is over this" rather than "you have navigated away".
    #[test]
    fn the_panel_leaves_a_frame_of_the_screen_underneath() {
        let (at, size): (Point, Size) = LANDSCAPE.panel();
        assert!(at.x > 0 && at.y > 0);
        assert!(at.x as u32 + size.width < LANDSCAPE.canvas.width);
        assert!(at.y as u32 + size.height < LANDSCAPE.canvas.height);
    }

    /// Many: moving the cursor is a different picture — the highlight actually tracks it.
    #[test]
    fn moving_the_cursor_paints_differently() {
        assert_ne!(
            painted(&LANDSCAPE, Overlay::Menu { cursor: 0 }).pixels(),
            painted(&LANDSCAPE, Overlay::Menu { cursor: 1 }).pixels()
        );
    }

    /// THE ONE A PIXEL-COUNT TEST MISSES: the cursor stays inside the window at both ends of a
    /// list longer than the panel. Stated on the pure arithmetic, at zero, one and many.
    #[test]
    fn the_window_always_contains_the_cursor() {
        // A list that fits needs no window at all.
        assert_eq!(window_start(4, 5, 5), 0);
        // A cursor inside the first window does not scroll it.
        assert_eq!(window_start(0, 3, 5), 0);
        assert_eq!(window_start(2, 3, 5), 0);
        // Past it, the window follows — and stops at the end of the list.
        assert_eq!(window_start(3, 3, 5), 1);
        assert_eq!(window_start(4, 3, 5), 2);
        assert_eq!(window_start(9, 3, 5), 2);
    }

    /// The last entry is drawn when it is selected — the failure the window exists to prevent,
    /// asserted on the picture rather than on the arithmetic.
    #[test]
    fn the_last_entry_is_on_the_glass_when_it_is_selected() {
        let last: u8 = (MENU_ENTRIES.len() - 1) as u8;
        assert_ne!(
            painted(&LANDSCAPE, Overlay::Menu { cursor: last }).pixels(),
            painted(&LANDSCAPE, Overlay::Menu { cursor: 0 }).pixels()
        );
    }

    /// The three overlays are three different pictures.
    #[test]
    fn the_three_overlays_are_distinct() {
        let menu: Framebuffer = painted(&LANDSCAPE, Overlay::Menu { cursor: 0 });
        let settings: Framebuffer = painted(
            &LANDSCAPE,
            Overlay::Settings {
                entry: MenuEntry::Species,
            },
        );
        let reset: Framebuffer = painted(&LANDSCAPE, Overlay::Reset);
        assert_ne!(menu.pixels(), settings.pixels());
        assert_ne!(settings.pixels(), reset.pixels());
        assert_ne!(menu.pixels(), reset.pixels());
    }

    /// Every settings entry has a page of its own, and none escapes either canvas.
    #[test]
    fn every_overlay_stays_on_both_canvases() {
        let escaped: usize = MENU_ENTRIES
            .iter()
            .map(|entry: &MenuEntry| {
                let overlay: Overlay = Overlay::Settings { entry: *entry };
                painted(&LANDSCAPE, overlay).escaped() + painted(&PORTRAIT, overlay).escaped()
            })
            .sum::<usize>()
            + painted(&LANDSCAPE, Overlay::Reset).escaped()
            + painted(&PORTRAIT, Overlay::Reset).escaped()
            + painted(&LANDSCAPE, Overlay::Menu { cursor: 4 }).escaped()
            + painted(&PORTRAIT, Overlay::Menu { cursor: 4 }).escaped();
        assert_eq!(escaped, 0);
    }
}
