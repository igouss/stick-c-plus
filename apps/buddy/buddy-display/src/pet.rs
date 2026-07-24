//! The pet screen: how the creature is doing, and how it came to be doing that.
//!
//! Two pages. The stats page is the readings — mood, fed, energy, level, the approve/deny
//! counts, the naps and the tokens. The how-to page explains what moves them, because a pet
//! whose numbers change for reasons the owner cannot guess is a random number generator with a
//! face.
//!
//! ## The meters place themselves
//!
//! A label and its meter sit on one row when the canvas is wide enough to hold both, and on two
//! rows when it is not — decided from the layout's own width rather than by a per-shape branch.
//! The ten-cell fed meter is the only one that has to stack, and only on the narrow canvas; a
//! hand-written portrait variant would have had to be kept in step with the landscape one every
//! time a meter changed size.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use platform_display::{text_field, RenderError, FONT};

use crate::backdrop;
use crate::layout::Layout;
use crate::meter::{meter, width};
use crate::page;
use crate::palette;
use crate::units::{Compact, Span};
use crate::view::{BuddyView, PetPage, StatsView};

/// Cells in the mood meter — `buddy_core::mood_tier` is `0..=4`.
const MOOD_CELLS: usize = 5;
/// Cells in the fed meter — `buddy_core::fed_progress` is `0..=9`.
const FED_CELLS: usize = 10;
/// Cells in the energy meter — `buddy_core::energy_tier` is `0..=5`.
const ENERGY_CELLS: usize = 6;

/// The label field every meter's name is padded to: `ENERGY` is the longest at six, plus a
/// column of air so the widest label does not butt against its meter.
const LABEL_COLS: usize = 7;

/// How the pet works, in the owner's terms. Wrapped by [`page`], so the same words land on both
/// canvases.
const HOW_TO: &str = "Tokens feed it - a level every 50k.\n\
                      Fast answers make it happy; denials do not.\n\
                      Face down is a nap: naps restore energy.\n\
                      Shake it to make it dizzy.";

/// Draw the pet screen at `page`.
pub fn render<D>(
    target: &mut D,
    layout: &Layout,
    view: &BuddyView,
    page: PetPage,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    match page {
        PetPage::Stats => stats(target, layout, &view.stats),
        PetPage::HowTo => {
            crate::page::render(target, layout, "HOW IT WORKS", palette::TITLE, HOW_TO)
        }
    }
}

/// The stats page: the title with the level, three meters, then the counts.
fn stats<D>(target: &mut D, layout: &Layout, stats: &StatsView) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_field(
        target,
        layout.row_origin(0),
        palette::TITLE,
        layout.cols,
        layout.align,
        format_args!("PET  LV {}", Compact(stats.level)),
    )?;

    let mut row: usize = 1;
    row += gauge(
        target,
        layout,
        row,
        "MOOD",
        MOOD_CELLS,
        stats.mood,
        palette::MOOD_LIT,
    )?;
    row += gauge(
        target,
        layout,
        row,
        "FED",
        FED_CELLS,
        stats.fed,
        palette::FED_LIT,
    )?;
    row += gauge(
        target,
        layout,
        row,
        "ENERGY",
        ENERGY_CELLS,
        stats.energy,
        palette::METER_LIT,
    )?;

    counts(target, layout, row, stats)
}

/// One labelled meter, on one row or two depending on what the canvas can hold. Returns the rows
/// it used.
fn gauge<D>(
    target: &mut D,
    layout: &Layout,
    row: usize,
    label: &str,
    cells: usize,
    lit: u8,
    colour: Rgb565,
) -> Result<usize, RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let band_height: u32 = FONT.character_size.height;
    let beside_x: i32 = layout.left + (FONT.character_size.width * LABEL_COLS as u32) as i32;
    let fits_beside: bool = beside_x as u32 + width(cells) <= layout.canvas.width;

    let label_band: Rectangle = layout.row_rect(row, LABEL_COLS);
    let at: Point = if fits_beside {
        Point::new(beside_x, layout.row_y(row))
    } else {
        Point::new(layout.left, layout.row_y(row + 1))
    };
    let meter_band: Rectangle = Rectangle::new(at, Size::new(width(cells), band_height));

    // The label and the meter are each opaque over their own band, so the background is painted
    // only AROUND them — never underneath, which would show the owner a cleared row before the
    // reading arrived. Beside the label the two bands are adjacent and merge into one band on one
    // row; stacked, they are two bands a row apart — which is the order `backdrop::behind` wants
    // either way. One branch decides the rows used and the bands together, because they are the
    // same decision: a third band later must not be able to change one and not the other.
    let (bands, used): (&[Rectangle], usize) = if fits_beside {
        (
            &[Rectangle::new(
                label_band.top_left,
                Size::new(label_band.size.width + meter_band.size.width, band_height),
            )],
            1,
        )
    } else {
        (&[label_band, meter_band], 2)
    };
    backdrop::behind(
        target,
        layout.rows_rect(row, used),
        bands.iter().copied(),
        palette::BACKGROUND,
    )?;

    text_field(
        target,
        layout.row_origin(row),
        palette::LABEL,
        LABEL_COLS,
        platform_display::FieldAlign::Left,
        format_args!("{label}"),
    )?;
    meter(target, at, band_height, cells, usize::from(lit), colour)?;
    Ok(used)
}

/// The counts under the meters, and the blank rows below them.
fn counts<D>(
    target: &mut D,
    layout: &Layout,
    first: usize,
    stats: &StatsView,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let rows: usize = layout.rows.saturating_sub(first);
    // Three readings, each on its own row on the narrow canvas and paired up on the wide one —
    // the same rule the meters follow, from the same measurement.
    let pairs: bool = layout.cols >= 20;
    let mut used: usize = 0;

    let mut line =
        |target: &mut D, text: core::fmt::Arguments<'_>| -> Result<(), RenderError<D::Error>> {
            if used >= rows {
                return Ok(());
            }
            text_field(
                target,
                layout.row_origin(first + used),
                palette::DIM,
                layout.cols,
                layout.align,
                text,
            )?;
            used += 1;
            Ok(())
        };

    if pairs {
        line(
            target,
            format_args!(
                "OK {} NO {}  x{}",
                Compact(stats.approvals),
                Compact(stats.denials),
                Compact(stats.naps)
            ),
        )?;
        line(
            target,
            format_args!(
                "TOK {}  NAP {}",
                Compact(stats.tokens_today),
                Span(stats.nap_minutes)
            ),
        )?;
    } else {
        line(target, format_args!("OK {}", Compact(stats.approvals)))?;
        line(target, format_args!("NO {}", Compact(stats.denials)))?;
        line(target, format_args!("NAPS {}", Compact(stats.naps)))?;
        line(target, format_args!("TOK {}", Compact(stats.tokens_today)))?;
        line(target, format_args!("NAP {}", Span(stats.nap_minutes)))?;
    }

    page::blank(target, layout, first + used, rows - used)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{LANDSCAPE, PORTRAIT};
    use buddy_core::SpeciesIndex;
    use platform_display::testing::Framebuffer;

    fn view() -> BuddyView {
        let mut view: BuddyView = BuddyView::resting(SpeciesIndex::new(0));
        view.stats = StatsView {
            mood: 3,
            fed: 6,
            energy: 4,
            level: 7,
            approvals: 41,
            denials: 3,
            naps: 5,
            nap_minutes: 134,
            tokens_today: 128_400,
        };
        view
    }

    fn painted(layout: &Layout, page: PetPage, view: &BuddyView) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::sized(layout.canvas);
        render(&mut fb, layout, view, page).expect("a framebuffer render cannot fail");
        fb
    }

    /// One: the stats page paints.
    #[test]
    fn the_stats_page_paints_pixels() {
        assert!(painted(&LANDSCAPE, PetPage::Stats, &view()).lit_pixels() > 0);
    }

    /// Many: the two pages are two different pictures.
    #[test]
    fn the_two_pages_are_different_pictures() {
        assert_ne!(
            painted(&LANDSCAPE, PetPage::Stats, &view()).pixels(),
            painted(&LANDSCAPE, PetPage::HowTo, &view()).pixels()
        );
    }

    /// Every reading reaches the glass — a stats page that dropped one would look right and be
    /// wrong, which is the failure a "it paints pixels" test cannot see.
    #[test]
    fn every_reading_reaches_the_glass() {
        let base: Framebuffer = painted(&LANDSCAPE, PetPage::Stats, &view());
        let changed = |mutate: fn(&mut StatsView)| -> Framebuffer {
            let mut view: BuddyView = view();
            mutate(&mut view.stats);
            painted(&LANDSCAPE, PetPage::Stats, &view)
        };
        assert_ne!(
            base.pixels(),
            changed(|s: &mut StatsView| s.mood = 1).pixels()
        );
        assert_ne!(
            base.pixels(),
            changed(|s: &mut StatsView| s.fed = 1).pixels()
        );
        assert_ne!(
            base.pixels(),
            changed(|s: &mut StatsView| s.energy = 1).pixels()
        );
        assert_ne!(
            base.pixels(),
            changed(|s: &mut StatsView| s.level = 8).pixels()
        );
        assert_ne!(
            base.pixels(),
            changed(|s: &mut StatsView| s.approvals = 42).pixels()
        );
        assert_ne!(
            base.pixels(),
            changed(|s: &mut StatsView| s.denials = 4).pixels()
        );
        assert_ne!(
            base.pixels(),
            changed(|s: &mut StatsView| s.naps = 6).pixels()
        );
        assert_ne!(
            base.pixels(),
            changed(|s: &mut StatsView| s.nap_minutes = 200).pixels()
        );
        assert_ne!(
            base.pixels(),
            changed(|s: &mut StatsView| s.tokens_today = 9).pixels()
        );
    }

    /// THE ONE THE FIRST DRAFT FAILED: the stacked meter survives the row blanking beneath it.
    /// On the narrow canvas the fed meter sits on its own row, and a blank drawn after it erased
    /// it — a page that looked almost right, with one reading silently missing.
    #[test]
    fn the_stacked_meter_survives_on_the_narrow_canvas() {
        let mut empty: BuddyView = view();
        empty.stats.fed = 0;
        assert_ne!(
            painted(&PORTRAIT, PetPage::Stats, &view()).pixels(),
            painted(&PORTRAIT, PetPage::Stats, &empty).pixels(),
            "the fed meter is not on the narrow canvas at all"
        );
    }

    /// Nothing escapes either canvas, at the largest readings — including the ten-cell fed meter
    /// that has to stack on the narrow one.
    #[test]
    fn the_largest_readings_stay_on_both_canvases() {
        let mut biggest: BuddyView = view();
        biggest.stats = StatsView {
            mood: u8::MAX,
            fed: u8::MAX,
            energy: u8::MAX,
            level: u32::MAX,
            approvals: u32::MAX,
            denials: u32::MAX,
            naps: u32::MAX,
            nap_minutes: u32::MAX,
            tokens_today: u32::MAX,
        };
        assert_eq!(painted(&LANDSCAPE, PetPage::Stats, &biggest).escaped(), 0);
        assert_eq!(painted(&PORTRAIT, PetPage::Stats, &biggest).escaped(), 0);
        assert_eq!(painted(&LANDSCAPE, PetPage::HowTo, &biggest).escaped(), 0);
        assert_eq!(painted(&PORTRAIT, PetPage::HowTo, &biggest).escaped(), 0);
    }

    /// The narrow canvas draws the same page differently rather than clipping the wide one.
    #[test]
    fn the_narrow_canvas_draws_its_own_stats_page() {
        let turned: Framebuffer = painted(&PORTRAIT, PetPage::Stats, &view());
        assert_ne!(
            turned.size(),
            painted(&LANDSCAPE, PetPage::Stats, &view()).size()
        );
        assert!(turned.lit_pixels() > 0);
    }
}
