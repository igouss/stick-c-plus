//! The transcript HUD: the newest activity, word-wrapped into the bottom band.
//!
//! The band is the rows from [`Layout::band_first_row`](crate::layout::Layout::band_first_row)
//! down, less the one column the scroll indicator stands in. Entries are drawn **newest first,
//! from the top of the band**: the newest line is the one a glance is for, and putting it at a
//! fixed place means the eye does not have to find where the text ends. The newest entry's
//! lines are bright and everything behind them is dim.
//!
//! Every row of the band is painted on every pass, blank ones included, so a shorter transcript
//! erases the longer one it replaced without a full-screen clear and therefore without a flash.
//!
//! ## The indicator reports the whole transcript, not the band
//!
//! The host holds more than four entries and the band shows fewer lines than that. The
//! indicator's thumb is `shown / total` of the track, so a band that is showing two of eleven
//! entries says so — otherwise a full-looking band is indistinguishable from the whole story.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
use platform_display::{text_field, RenderError};

use crate::backdrop;
use crate::layout::Layout;
use crate::palette;
use crate::view::{Entry, Transcript};
use crate::wrap::wrap;

/// What stands in the band when the host has sent no activity yet.
///
/// A word rather than an empty band: a blank region on a small panel reads as a firmware that
/// has stopped, and the difference between "quiet" and "broken" is the whole point of a HUD.
const QUIET: &str = "-- quiet --";

/// The scroll indicator's track width, in pixels.
const TRACK_WIDTH: u32 = 4;

/// The shortest thumb that is still visibly a thumb.
const MIN_THUMB: u32 = 4;

/// Draw the transcript band and its scroll indicator.
pub fn render<D>(
    target: &mut D,
    layout: &Layout,
    transcript: &Transcript,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let rows: usize = layout.band_rows();
    let cols: usize = layout.band_cols();
    let first: usize = layout.band_first_row;

    let mut used: usize = 0;
    let mut shown: usize = 0;

    // Each entry contributes as many wrapped lines as it needs, and the band takes the first
    // `rows` of that stream — so one long newest entry may fill the band on its own, which is
    // the right answer: it is the thing that just happened.
    let lines = transcript
        .newest_first()
        .iter()
        .enumerate()
        .flat_map(|(index, entry): (usize, &Entry)| {
            wrap(entry.as_str(), cols).map(move |line: &str| (index, line))
        })
        .take(rows);

    for (row, (index, line)) in lines.enumerate() {
        let colour: Rgb565 = if index == 0 {
            palette::PRIMARY
        } else {
            palette::DIM
        };
        text_field(
            target,
            layout.row_origin(first + row),
            colour,
            cols,
            layout.align,
            format_args!("{line}"),
        )?;
        used = row + 1;
        shown = index + 1;
    }

    if transcript.is_empty() && rows > 0 {
        text_field(
            target,
            layout.row_origin(first),
            palette::DIM,
            cols,
            layout.align,
            format_args!("{QUIET}"),
        )?;
        used = 1;
    }

    // Blank the rest of the band in place, so a shrinking transcript erases itself.
    (used..rows).try_for_each(|row: usize| {
        text_field(
            target,
            layout.row_origin(first + row),
            palette::PRIMARY,
            cols,
            layout.align,
            format_args!(""),
        )
    })?;

    scroll(target, layout, first, rows, shown, transcript.total())
}

/// Draw the scroll indicator beside the band: a full-height track with a thumb sized by how
/// much of the transcript is on the glass.
///
/// The track is repainted every pass, which is what erases the previous thumb — the same
/// erase-in-place discipline the text fields use.
fn scroll<D>(
    target: &mut D,
    layout: &Layout,
    first_row: usize,
    rows: usize,
    shown: usize,
    total: usize,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    if rows == 0 {
        return Ok(());
    }
    // The rows the band spans, as one region — the height is the layout's arithmetic, not this
    // renderer's. Only the width is the bar's own: it is a fixed track beside the text, not a
    // full-width row.
    let band: Rectangle = layout.rows_rect(first_row, rows);
    let height: u32 = band.size.height;
    let at: Point = Point::new(layout.scroll_x() + 3, band.top_left.y);

    // Nothing behind the band means the thumb is the whole track: all of the story is on the
    // glass. `shown >= total` rather than `total == 0` so an empty transcript takes this branch
    // too, instead of dividing by zero.
    let thumb: u32 = if shown >= total {
        height
    } else {
        ((height as u64 * shown as u64 / total as u64) as u32).max(MIN_THUMB)
    };
    let thumb: Rectangle = Rectangle::new(at, Size::new(TRACK_WIDTH, thumb.min(height)));

    // The thumb, then the track *around* it — not a full track with the thumb painted over.
    // The band repaints on every animation frame, so a thumb drawn over its own track would be
    // 250 pixels blinking at the creature's cadence, for the whole life of the screen.
    backdrop::fill(target, thumb, palette::SCROLL_THUMB)?;
    backdrop::behind(
        target,
        Rectangle::new(at, Size::new(TRACK_WIDTH, height)),
        [thumb],
        palette::SCROLL_TRACK,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::LANDSCAPE;
    use platform_display::testing::Framebuffer;

    fn painted(transcript: &Transcript) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        render(&mut fb, &LANDSCAPE, transcript).expect("a framebuffer render cannot fail");
        fb
    }

    /// Zero: an empty transcript says so rather than leaving the band blank — a blank region on
    /// a small panel reads as a stopped firmware.
    #[test]
    fn an_empty_transcript_says_it_is_quiet() {
        let quiet: Framebuffer = painted(&Transcript::oldest_first(&[]));
        let mut blank: Framebuffer = Framebuffer::new();
        // The band with its text rows painted empty — everything the quiet band draws EXCEPT
        // the word. If the two matched, the word never reached the glass.
        scroll(
            &mut blank,
            &LANDSCAPE,
            LANDSCAPE.band_first_row,
            LANDSCAPE.band_rows(),
            0,
            0,
        )
        .expect("a framebuffer render cannot fail");
        assert_ne!(quiet.pixels(), blank.pixels());
        assert!(quiet.lit_pixels() > 0);
    }

    /// One: an entry reaches the glass.
    #[test]
    fn one_entry_paints_the_band() {
        assert!(painted(&Transcript::oldest_first(&["ran the tests"])).lit_pixels() > 0);
    }

    /// Many: a different transcript is a different picture — the band actually reports.
    #[test]
    fn a_changed_transcript_paints_differently() {
        assert_ne!(
            painted(&Transcript::oldest_first(&["ran the tests"])).pixels(),
            painted(&Transcript::oldest_first(&[
                "ran the tests",
                "wrote the crate"
            ]))
            .pixels()
        );
    }

    /// A long entry wraps instead of running off the glass — nothing escapes the canvas.
    #[test]
    fn a_long_entry_wraps_and_nothing_escapes() {
        let long: &str =
            "Bash: cargo test --workspace --all-features and then some more words besides";
        assert_eq!(painted(&Transcript::oldest_first(&[long])).escaped(), 0);
    }

    /// A shrinking transcript erases the longer one it replaced: two entries then one paints the
    /// same picture as one entry alone, so no leftover row survives underneath.
    #[test]
    fn a_shrinking_transcript_erases_what_it_replaced() {
        let mut fb: Framebuffer = Framebuffer::new();
        render(
            &mut fb,
            &LANDSCAPE,
            &Transcript::oldest_first(&["older line", "newer line"]),
        )
        .expect("a framebuffer render cannot fail");
        render(
            &mut fb,
            &LANDSCAPE,
            &Transcript::oldest_first(&["newer line"]),
        )
        .expect("a framebuffer render cannot fail");
        assert_eq!(
            fb.pixels(),
            painted(&Transcript::oldest_first(&["newer line"])).pixels()
        );
    }

    /// The indicator reports the WHOLE transcript: eleven entries behind a band showing two is a
    /// different picture from two entries and nothing behind them, even though the visible text
    /// is identical.
    #[test]
    fn the_indicator_distinguishes_a_deep_transcript_from_a_shallow_one() {
        let shallow: Transcript = Transcript::oldest_first(&["older", "newer"]);
        let deep: Transcript = Transcript::oldest_first(&[
            "a", "b", "c", "d", "e", "f", "g", "h", "i", "older", "newer",
        ]);
        assert_ne!(painted(&shallow).pixels(), painted(&deep).pixels());
    }
}
