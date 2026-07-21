//! The orientation screen: a face label, the pitch and roll, and three signed axis bars.
//!
//! Device-independent by construction — it draws into any [`DrawTarget`], so the on-target
//! panel and a host framebuffer render *the same code*.
//!
//! The picture answers two questions at two speeds. The header answers "which way is this
//! thing pointing?" in words and degrees, readable at a glance from across a desk. The three
//! bars answer "what is the sensor actually saying?", continuously and with the sign shown as
//! *direction* — so a board being turned makes the bars sweep, and the relationship between
//! the movement in your hand and the movement on the glass is visible rather than inferred.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use orientation_core::{Facing, Signal};
use platform_display::{signed_bar, text_line, RenderError};

use crate::layout::{
    row_bar, row_origin, row_value_origin, ANGLES_ORIGIN, ANGLES_WIDTH, AXIS_NAME_WIDTH,
    BAR_FULL_SCALE_MG, FACING_ORIGIN, FACING_WIDTH, VALUE_WIDTH,
};
use crate::view::OrientationView;

/// The colour of each axis, in the screen's stacking order: the X/Y/Z = red/green/blue
/// convention every 3D tool uses, so the bars are readable without consulting a key.
const AXIS_COLOURS: [Rgb565; 3] = [Rgb565::RED, Rgb565::GREEN, Rgb565::BLUE];

/// The colour of a bar's centre line — dim, so the axis is present without competing with the
/// bar that grows from it.
const AXIS_LINE_COLOUR: Rgb565 = Rgb565::new(8, 16, 8);

/// How far a colour is knocked down when the reading behind it is no longer being confirmed.
///
/// A quarter brightness: still legible, plainly not live. The numbers stay on the glass
/// because the last thing the sensor said is genuinely useful — it is the *claim* that they
/// are current which has to go.
const STALE_DIMMING: u8 = 4;

/// `colour` at a quarter brightness, for a reading that is a memory rather than a measurement.
///
/// Not `const`: `RgbColor`'s channel accessors are trait methods, and const traits are not on
/// stable. It folds at the call sites that matter anyway.
fn dimmed(colour: Rgb565) -> Rgb565 {
    Rgb565::new(
        colour.r() / STALE_DIMMING,
        colour.g() / STALE_DIMMING,
        colour.b() / STALE_DIMMING,
    )
}

/// `colour` as it should be drawn under `signal` — full strength while the sensor answers,
/// dimmed once it has stopped.
fn under(signal: Signal, colour: Rgb565) -> Rgb565 {
    if signal.is_live() {
        colour
    } else {
        dimmed(colour)
    }
}

/// The colour of the label field, in three steps of decreasing confidence.
///
/// White once the board is settled on a face; amber while it is being moved or held between
/// faces — the name is an apology rather than a fact; red when the sensor has stopped
/// answering altogether, where there is no name at all, only the last one. A glance tells you
/// which of the three you are looking at before you read the word.
const fn label_colour(facing: Facing, signal: Signal) -> Rgb565 {
    match signal {
        Signal::Lost => Rgb565::RED,
        Signal::Live if facing.is_resting() => Rgb565::WHITE,
        Signal::Live => Rgb565::new(31, 40, 0),
    }
}

/// Render the orientation screen: the face label, the angles, and the three axis bars.
///
/// `elapsed_ms` is how long this state has been current. Nothing on this screen animates, so
/// it is unused — the parameter is the generic render loop's contract, kept so this app drives
/// the *same* loop the pomodoro timer and the plant monitor do.
pub fn render<D>(
    target: &mut D,
    view: OrientationView,
    _elapsed_ms: u64,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    facing_label(target, view)?;
    angles(target, view)?;
    axis_rows(target, view)
}

/// Paint the label field: the resting-face name, or that the sensor has gone quiet.
fn facing_label<D>(target: &mut D, view: OrientationView) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_line(
        target,
        FACING_ORIGIN,
        label_colour(view.facing, view.signal),
        FACING_WIDTH,
        format_args!("{}", view.label()),
    )
}

/// Paint the pitch and roll, both signed so the direction of the tilt is on the glass.
///
/// Each angle is zero-padded to its own widest value — pitch to a quarter turn, roll to a
/// half — so the two fields never change width. Left-aligned text that grew from `R+45` to
/// `R+180` would shove itself sideways on the glass every time the board passed 100°, and on
/// a readout that repaints many times a second that reads as a flicker rather than as data.
fn angles<D>(target: &mut D, view: OrientationView) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_line(
        target,
        ANGLES_ORIGIN,
        under(view.signal, Rgb565::CYAN),
        ANGLES_WIDTH,
        format_args!("P{:+03} R{:+04}", view.pitch_deg, view.roll_deg),
    )
}

/// Paint the three axis rows: name, bar, value.
fn axis_rows<D>(target: &mut D, view: OrientationView) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    view.axes().iter().enumerate().try_for_each(
        |(index, (name, value_mg)): (usize, &(&'static str, i32))| {
            axis_row(target, index as i32, name, *value_mg, view.signal)
        },
    )
}

/// Paint one axis row: its one-letter name, its centre-zero bar, and its milli-g value.
///
/// The whole row is drawn under `signal`, so a readout nothing is feeding fades together
/// rather than leaving three confident bars beneath a red label.
fn axis_row<D>(
    target: &mut D,
    index: i32,
    name: &str,
    value_mg: i32,
    signal: Signal,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let colour: Rgb565 = under(signal, AXIS_COLOURS[index as usize]);

    text_line(
        target,
        row_origin(index),
        colour,
        AXIS_NAME_WIDTH,
        format_args!("{name}"),
    )?;
    signed_bar(
        target,
        row_bar(index),
        value_mg,
        BAR_FULL_SCALE_MG,
        colour,
        AXIS_LINE_COLOUR,
        Rgb565::BLACK,
    )?;
    // Right-aligned in its field, so the three rows' digits line up as a column and a value
    // growing from `+0` to `+1000` does not drag its own text across the glass.
    text_line(
        target,
        row_value_origin(index),
        under(signal, Rgb565::WHITE),
        VALUE_WIDTH,
        format_args!("{value_mg:>+width$}", width = VALUE_WIDTH),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use orientation_core::{Orientation, Reading, SIGNAL_TIMEOUT_MS};
    use platform_core::{Acceleration, ONE_G_MG};
    use platform_display::testing::Framebuffer;

    /// A live view of the board reading these axes.
    fn view_of(x_mg: i32, y_mg: i32, z_mg: i32) -> OrientationView {
        OrientationView::of(&Reading::aged(
            Orientation::of(Acceleration::new(x_mg, y_mg, z_mg)),
            0,
        ))
    }

    /// The same board, aged past the point where the readout still vouches for it.
    fn stale_view_of(x_mg: i32, y_mg: i32, z_mg: i32) -> OrientationView {
        OrientationView::of(&Reading::aged(
            Orientation::of(Acceleration::new(x_mg, y_mg, z_mg)),
            SIGNAL_TIMEOUT_MS,
        ))
    }

    /// Paint `view` into a fresh framebuffer and hand it back for inspection.
    fn painted(view: OrientationView) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        render(&mut fb, view, 0).expect("a framebuffer render cannot fail");
        fb
    }

    /// Zero: an unrendered canvas is blank — the baseline every other assertion rests on.
    #[test]
    fn an_unrendered_canvas_is_blank() {
        assert_eq!(Framebuffer::new().lit_pixels(), 0);
    }

    /// One: the pose the board sits in puts ink on the glass.
    #[test]
    fn a_resting_board_paints_pixels() {
        assert!(painted(view_of(0, 0, ONE_G_MG)).lit_pixels() > 0);
    }

    /// Many: three different poses paint three distinct pictures — a glance tells them apart,
    /// which is the entire purpose of the screen.
    #[test]
    fn three_poses_paint_three_distinct_pictures() {
        let back: Framebuffer = painted(view_of(0, 0, ONE_G_MG));
        let upright: Framebuffer = painted(view_of(ONE_G_MG, 0, 0));
        let edge: Framebuffer = painted(view_of(0, -ONE_G_MG, 0));
        assert_ne!(back.pixels(), upright.pixels());
        assert_ne!(upright.pixels(), edge.pixels());
        assert_ne!(back.pixels(), edge.pixels());
    }

    /// The readout is live to the smallest movement the domain reports: one milli-g on one
    /// axis reaches the glass. A screen that rounded this away would look calm and lie.
    #[test]
    fn a_single_milli_g_of_movement_reaches_the_glass() {
        assert_ne!(
            painted(view_of(0, 0, ONE_G_MG)).pixels(),
            painted(view_of(1, 0, ONE_G_MG)).pixels()
        );
    }

    /// The sign is drawn as direction: equal and opposite readings must not paint the same
    /// picture. A bar that plotted magnitude only would pass every ink-count test and be
    /// useless for telling which way up the board is.
    #[test]
    fn opposite_readings_paint_different_pictures() {
        assert_ne!(
            painted(view_of(0, 0, ONE_G_MG)).pixels(),
            painted(view_of(0, 0, -ONE_G_MG)).pixels()
        );
    }

    /// Nothing escapes the canvas in any state — including the widest face label, the widest
    /// angles, and a reading well past the bars' full scale.
    #[test]
    fn no_state_escapes_the_canvas() {
        // The widest label (RIGHT EDGE) and a full-scale reading on its axis.
        assert_eq!(painted(view_of(0, ONE_G_MG, 0)).escaped(), 0);
        // The widest angles: a half turn of roll reads R-180.
        assert_eq!(painted(view_of(0, 0, -ONE_G_MG)).escaped(), 0);
        // Far past full scale, on every axis at once, in both signs.
        assert_eq!(painted(view_of(8_000, -8_000, 8_000)).escaped(), 0);
        assert_eq!(painted(view_of(-8_000, 8_000, -8_000)).escaped(), 0);
    }

    /// A reading past the bars' full scale is clamped rather than refused — the bar runs out
    /// of room, but the render still succeeds and the number beside it still prints.
    #[test]
    fn an_over_range_reading_still_renders() {
        let hard_shake: OrientationView = view_of(0, 0, 8 * ONE_G_MG);
        assert!(painted(hard_shake).lit_pixels() > 0);
        assert_eq!(painted(hard_shake).escaped(), 0);
    }

    /// The erase-in-place guarantee, end to end: painting a large reading and then a small one
    /// over it must leave the small one's picture, not a smear of both. This is what lets the
    /// readout repaint at the loop's full cadence without a clear, and therefore without a
    /// flash.
    #[test]
    fn a_smaller_reading_fully_replaces_a_larger_one() {
        let small: OrientationView = view_of(0, 0, ONE_G_MG);

        let mut reused: Framebuffer = Framebuffer::new();
        render(&mut reused, view_of(-950, 900, -900), 0).expect("the large reading");
        render(&mut reused, small, 0).expect("the small reading over it");

        assert_eq!(
            reused.pixels(),
            painted(small).pixels(),
            "the previous reading was not fully erased — the readout would smear as it moves"
        );
    }

    /// A settled face and an unsettled one are drawn differently, so "I know" and "I cannot
    /// say" never look alike.
    #[test]
    fn a_settled_face_is_drawn_differently_from_an_unsettled_one() {
        let settled: Framebuffer = painted(view_of(0, 0, ONE_G_MG));
        let moving: Framebuffer = painted(view_of(0, 0, 3 * ONE_G_MG));
        assert_ne!(settled.pixels(), moving.pixels());
    }

    /// The honesty gap this screen exists to close: identical numbers, one still being
    /// confirmed and one not, must not paint the same picture. A screen that failed this
    /// would show a dead sensor as a perfectly calm readout.
    #[test]
    fn a_lost_signal_does_not_look_like_a_live_one() {
        assert_ne!(
            painted(view_of(0, 0, ONE_G_MG)).pixels(),
            painted(stale_view_of(0, 0, ONE_G_MG)).pixels()
        );
    }

    /// The three steps of the label vocabulary are three distinct pictures: a named face, a
    /// face it cannot name, and no signal at all.
    #[test]
    fn the_three_degrees_of_confidence_are_three_distinct_pictures() {
        let settled: Framebuffer = painted(view_of(0, 0, ONE_G_MG));
        let unsettled: Framebuffer = painted(view_of(0, 707, 707));
        let lost: Framebuffer = painted(stale_view_of(0, 0, ONE_G_MG));
        assert_ne!(settled.pixels(), unsettled.pixels());
        assert_ne!(unsettled.pixels(), lost.pixels());
        assert_ne!(settled.pixels(), lost.pixels());
    }

    /// A lost signal still paints — it dims and relabels the readout rather than blanking it,
    /// because the last thing the sensor said remains worth seeing.
    #[test]
    fn a_lost_signal_still_paints_the_last_reading() {
        assert!(painted(stale_view_of(0, 0, ONE_G_MG)).lit_pixels() > 0);
    }

    /// Nothing escapes the canvas with the `NO SIGNAL` label up either — it is the widest
    /// thing the field ever holds after `RIGHT EDGE`.
    #[test]
    fn a_lost_signal_does_not_escape_the_canvas() {
        assert_eq!(stale_view_of(0, ONE_G_MG, 0).label(), "NO SIGNAL");
        assert_eq!(painted(stale_view_of(0, ONE_G_MG, 0)).escaped(), 0);
        assert_eq!(painted(stale_view_of(-8_000, 8_000, -8_000)).escaped(), 0);
    }

    /// Erase-in-place holds across a signal change too: a readout that goes stale and
    /// recovers must land back on exactly the live picture, not a smear of the dimmed one.
    #[test]
    fn recovering_the_signal_fully_replaces_the_stale_picture() {
        let live: OrientationView = view_of(0, 0, ONE_G_MG);

        let mut reused: Framebuffer = Framebuffer::new();
        render(&mut reused, stale_view_of(0, 0, ONE_G_MG), 0).expect("the stale reading");
        render(&mut reused, live, 0).expect("the live reading over it");

        assert_eq!(
            reused.pixels(),
            painted(live).pixels(),
            "a recovered signal left the stale picture smeared underneath"
        );
    }

    /// Dimming darkens without erasing: a stale colour is neither the live one nor black,
    /// which is what keeps the numbers legible while plainly not current.
    #[test]
    fn dimming_darkens_without_erasing() {
        let dim: Rgb565 = dimmed(Rgb565::WHITE);
        assert_ne!(dim, Rgb565::WHITE);
        assert_ne!(dim, Rgb565::BLACK);
        assert_eq!(dimmed(Rgb565::BLACK), Rgb565::BLACK);
    }
}
