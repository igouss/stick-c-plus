//! The approval screen: what Claude Code is asking, how long it has waited, and which button
//! answers.
//!
//! It takes over the transcript band — the whole band, every row — because a pending permission
//! prompt is the only thing on the device that is *blocking someone*. The creature stays where
//! it is above; only the band changes, so the screen the owner already knows does not rearrange
//! itself at the moment they most need to read it fast.
//!
//! ## The counter is not a countdown
//!
//! It counts **up**, from when the prompt arrived, and it never expires here. The hook holds its
//! own deadline host-side and fails safe on its own; a device that drew a countdown would be
//! claiming an authority it does not have, and a bar reaching zero on the glass would read as
//! "denied" when the truth is "the host stopped waiting". So: an honest elapsed time, which goes
//! [`HOT_AFTER_S`](crate::view::HOT_AFTER_S) red when it is worth hurrying.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_display::{text_field, FieldAlign, RenderError, FONT};

use crate::layout::Layout;
use crate::palette;
use crate::view::PromptView;
use crate::wrap::{wrap, Cut};

/// The rows the header and the footer take, leaving the rest for the hint.
const CHROME_ROWS: usize = 2;

/// The narrowest half-band that still fits the long button labels.
const WIDE_FOOTER_COLS: usize = 9;

/// Columns the elapsed reading and its trailing space claim from the header row.
///
/// [`Waited`] is at most three characters by construction, and the tool name gets whatever is
/// left — cut, rather than allowed to push the row off the glass. The wire supplies the tool
/// name, so its length is not this crate's to assume.
const ELAPSED_COLS: usize = 4;

/// Draw the approval screen into the band.
pub fn render<D>(
    target: &mut D,
    layout: &Layout,
    prompt: &PromptView,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let rows: usize = layout.band_rows();
    let cols: usize = layout.band_cols();
    let first: usize = layout.band_first_row;

    header(target, layout, prompt, first, cols)?;

    // Whatever the band has left between the header and the footer goes to the hint. A band of
    // two rows has none, and the hint is simply not shown — the tool name and the buttons are
    // what an answer actually needs.
    let hint_rows: usize = rows.saturating_sub(CHROME_ROWS);
    hint(target, layout, prompt, first + 1, hint_rows, cols)?;

    footer(target, layout, first + rows - 1, cols)
}

/// The top row: how long it has waited, and what it is about.
fn header<D>(
    target: &mut D,
    layout: &Layout,
    prompt: &PromptView,
    row: usize,
    cols: usize,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let colour: Rgb565 = if prompt.is_hot() {
        palette::PROMPT_HOT
    } else {
        palette::PROMPT_WARM
    };
    text_field(
        target,
        layout.row_origin(row),
        colour,
        cols,
        layout.align,
        format_args!(
            "{} {}",
            Waited(prompt.waiting_s),
            Cut(prompt.tool.as_str(), cols.saturating_sub(ELAPSED_COLS))
        ),
    )
}

/// The middle rows: the hint, wrapped, and blanked out to the full band so a shorter hint erases
/// the longer one it replaced.
fn hint<D>(
    target: &mut D,
    layout: &Layout,
    prompt: &PromptView,
    first: usize,
    rows: usize,
    cols: usize,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    // With a single row to spend, the hint is CUT rather than wrapped. A word-wrapped one-liner
    // breaks at the last space that fits and leaves the rest of the row empty — `rm -rf` where
    // the whole question is which path follows it. A cut row is a fragment either way, and the
    // full row carries more of the command.
    if rows == 1 {
        return text_field(
            target,
            layout.row_origin(first),
            palette::PRIMARY,
            cols,
            layout.align,
            format_args!("{}", Cut(prompt.hint.as_str(), cols)),
        );
    }

    let mut used: usize = 0;
    for (row, line) in wrap(prompt.hint.as_str(), cols).take(rows).enumerate() {
        text_field(
            target,
            layout.row_origin(first + row),
            palette::PRIMARY,
            cols,
            layout.align,
            format_args!("{line}"),
        )?;
        used = row + 1;
    }
    (used..rows).try_for_each(|row: usize| {
        text_field(
            target,
            layout.row_origin(first + row),
            palette::PRIMARY,
            cols,
            layout.align,
            format_args!(""),
        )
    })
}

/// The bottom row: which button does what.
///
/// Two fields rather than one line, because the two halves are different colours — green means
/// yes and red means no, and that is the part a hurried glance actually reads. Each half is a
/// padded field of its own, so it erases in place like every other field on the glass.
fn footer<D>(
    target: &mut D,
    layout: &Layout,
    row: usize,
    cols: usize,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let half: usize = cols / 2;
    let (approve, deny): (&str, &str) = if half >= WIDE_FOOTER_COLS {
        ("A ALLOW", "B DENY")
    } else {
        ("A OK", "B NO")
    };
    let origin: Point = layout.row_origin(row);
    text_field(
        target,
        origin,
        palette::APPROVE,
        half,
        FieldAlign::Left,
        format_args!("{approve}"),
    )?;
    text_field(
        target,
        origin + Point::new((FONT.character_size.width * half as u32) as i32, 0),
        palette::DENY,
        cols - half,
        FieldAlign::Left,
        format_args!("{deny}"),
    )
}

/// How long a prompt has waited, as the header shows it.
///
/// Seconds while there are two digits of them, then whole minutes — a four-digit second count
/// would push the tool name off a thirteen-column canvas, and by then the exact second has
/// stopped being the interesting number anyway.
struct Waited(u32);

impl core::fmt::Display for Waited {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            seconds if seconds < 100 => write!(f, "{seconds}s"),
            seconds => write!(f, "{}m", (seconds / 60).min(99)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{LANDSCAPE, PORTRAIT};
    use crate::view::{Hint, Tool, HOT_AFTER_S};
    use platform_display::testing::Framebuffer;

    fn prompt(waiting_s: u32) -> PromptView {
        PromptView {
            tool: Tool::new("Bash"),
            hint: Hint::new("cargo test --workspace"),
            waiting_s,
        }
    }

    fn painted(prompt: &PromptView) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        render(&mut fb, &LANDSCAPE, prompt).expect("a framebuffer render cannot fail");
        fb
    }

    /// One: a prompt reaches the glass.
    #[test]
    fn a_prompt_paints_pixels() {
        assert!(painted(&prompt(1)).lit_pixels() > 0);
    }

    /// The counter reaches the glass — a header that ignored the seconds would paint the same
    /// picture twice.
    #[test]
    fn each_second_paints_differently() {
        assert_ne!(painted(&prompt(3)).pixels(), painted(&prompt(4)).pixels());
    }

    /// The screen turns hot at ten seconds and not before. Compared at nine against ten — one
    /// second apart, so the only thing that can differ is the colour and the digit.
    #[test]
    fn the_screen_turns_hot_at_the_threshold() {
        assert!(!prompt(HOT_AFTER_S - 1).is_hot());
        assert!(prompt(HOT_AFTER_S).is_hot());
        assert_ne!(
            painted(&prompt(HOT_AFTER_S - 1)).pixels(),
            painted(&prompt(HOT_AFTER_S)).pixels()
        );
    }

    /// Many: the tool and the hint both reach the glass, so the owner is answering about the
    /// right thing.
    #[test]
    fn the_tool_and_the_hint_both_reach_the_glass() {
        let mut other_tool: PromptView = prompt(1);
        other_tool.tool = Tool::new("WebFetch");
        let mut other_hint: PromptView = prompt(1);
        other_hint.hint = Hint::new("rm -rf /tmp/build");
        assert_ne!(painted(&prompt(1)).pixels(), painted(&other_tool).pixels());
        assert_ne!(painted(&prompt(1)).pixels(), painted(&other_hint).pixels());
    }

    /// A one-row band fills its row with the head of the hint rather than breaking at the first
    /// space — on a landscape band the path after `rm -rf` is the whole question, and a
    /// word-wrapped one-liner would drop it.
    #[test]
    fn a_single_hint_row_is_cut_rather_than_word_wrapped() {
        assert_eq!(
            LANDSCAPE.band_rows() - CHROME_ROWS,
            1,
            "the landscape band gives the hint exactly one row"
        );
        let mut long: PromptView = prompt(1);
        long.hint = Hint::new("rm -rf target/xtensa-esp32-espidf/release");
        let mut head_only: PromptView = prompt(1);
        head_only.hint = Hint::new("rm -rf");
        assert_ne!(
            painted(&long).pixels(),
            painted(&head_only).pixels(),
            "the row stopped at the first break instead of filling"
        );
    }

    /// A shrinking hint erases the longer one it replaced — no leftover row survives beneath.
    #[test]
    fn a_shorter_hint_erases_the_longer_one() {
        let mut long: PromptView = prompt(1);
        long.hint = Hint::new("a much longer command line that wraps onto a second row");
        let short: PromptView = prompt(1);

        let mut fb: Framebuffer = Framebuffer::new();
        render(&mut fb, &LANDSCAPE, &long).expect("a framebuffer render cannot fail");
        render(&mut fb, &LANDSCAPE, &short).expect("a framebuffer render cannot fail");
        assert_eq!(fb.pixels(), painted(&short).pixels());
    }

    /// Nothing escapes either canvas, at the widest tool, the longest hint and the largest
    /// counter — the three fields that could run off an edge.
    #[test]
    fn the_widest_prompt_stays_on_both_canvases() {
        let mut widest: PromptView = prompt(u32::MAX);
        widest.tool = Tool::new("a-very-long-tool-name-indeed");
        widest.hint = Hint::new(
            "/home/elendal/code/m5/stick-c-plus/apps/buddy/buddy-display/src/approval.rs",
        );
        assert_eq!(painted(&widest).escaped(), 0);

        let mut turned: Framebuffer = Framebuffer::sized(PORTRAIT.canvas);
        render(&mut turned, &PORTRAIT, &widest).expect("a framebuffer render cannot fail");
        assert_eq!(turned.escaped(), 0);
    }

    /// The elapsed reading stays two digits wide: seconds while there are two digits of them,
    /// then minutes, then a cap. This is what keeps the tool name on a thirteen-column canvas.
    #[test]
    fn the_elapsed_reading_stays_short() {
        assert_eq!(alloc::format!("{}", Waited(0)), "0s");
        assert_eq!(alloc::format!("{}", Waited(99)), "99s");
        assert_eq!(alloc::format!("{}", Waited(100)), "1m");
        assert_eq!(alloc::format!("{}", Waited(u32::MAX)), "99m");
        // Three characters at most, which is what leaves room for the tool name beside it.
        assert!(alloc::format!("{}", Waited(u32::MAX)).len() < ELAPSED_COLS);
    }
}
