//! The clock: the time, and how the board is powered.
//!
//! A screen the owner walks to, not one a charger imposes: a stick that hid its creature behind a
//! clock the moment it was plugged in spent the glass on a fact the owner already had. The
//! persona still runs underneath while docked — `buddy_core::charging_mood` writes the mood
//! directly — but the *picture* here is the time, because that is what an idle desk object is
//! asked for. The one thing that does put this screen up unasked is a battery about to die
//! (`buddy_core::CRITICAL_BATTERY_PCT`), which is the fact the owner cannot see coming.
//!
//! Both ways up, deliberately. A charging stick is as often stood on its USB-C port as laid flat,
//! and this is the one screen whose whole job is to be read from across the desk.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_display::{text_field, FieldAlign, RenderError};

use crate::backdrop;
use crate::layout::Layout;
use crate::palette;
use crate::view::ClockView;

/// Battery percentage at or below which the reading turns hot.
const LOW_BATTERY_PCT: u8 = 20;

/// Draw the charging clock.
pub fn render<D>(
    target: &mut D,
    layout: &Layout,
    clock: &ClockView,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let time_row: usize = layout.rows / 2 - 1;

    // The background everywhere the two rows are not, and then the rows — each pixel painted
    // exactly once. A full-screen clear first would blank the time before rewriting it, which on
    // a panel with no framebuffer is a visible blink of the whole glass on every repaint.
    backdrop::behind(
        target,
        layout.canvas_rect(),
        [layout.full_row(time_row), layout.full_row(time_row + 1)],
        palette::BACKGROUND,
    )?;
    text_field(
        target,
        layout.row_origin(time_row),
        time_colour(clock),
        layout.cols,
        FieldAlign::Centred,
        format_args!("{}", Time(clock)),
    )?;
    text_field(
        target,
        layout.row_origin(time_row + 1),
        battery_colour(clock),
        layout.cols,
        FieldAlign::Centred,
        format_args!("{}", Power(clock)),
    )
}

/// The time itself, or `--:--` on a stick nobody has told the time to.
///
/// The board has no RTC; it learns the hour from the host over the link. Until then the honest
/// answer is dashes — the same shape the glass already uses for a battery it cannot measure. A
/// confident `00:00` is not a blank, it is a wrong reading the owner would act on.
struct Time<'a>(&'a ClockView);

impl core::fmt::Display for Time<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0.time {
            Some((hour, minute)) => write!(f, "{:02}:{:02}", hour.min(23), minute.min(59)),
            None => f.write_str("--:--"),
        }
    }
}

/// The time is dim until it is real, so an unsynced clock does not read as an authoritative one.
const fn time_colour(clock: &ClockView) -> Rgb565 {
    match clock.time {
        Some(_) => palette::PRIMARY,
        None => palette::DIM,
    }
}

/// The power line under the time: the charge when the board can measure it, and whether it is
/// filling. A board with no gauge says only what it does know.
struct Power<'a>(&'a ClockView);

impl core::fmt::Display for Power<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match (self.0.battery_pct, self.0.charging) {
            (Some(pct), true) => write!(f, "{}% CHG", pct.min(100)),
            (Some(pct), false) => write!(f, "{}%", pct.min(100)),
            (None, true) => f.write_str("CHARGING"),
            (None, false) => f.write_str("ON BATTERY"),
        }
    }
}

/// The colour of the battery reading: charging is calm, a low battery off the charger is not.
///
/// Charging outranks low, because a low battery that is *filling* is a non-event and colouring
/// it red would teach the owner to ignore the colour.
const fn battery_colour(clock: &ClockView) -> Rgb565 {
    match (clock.charging, clock.battery_pct) {
        (true, _) => palette::APPROVE,
        (false, Some(pct)) if pct <= LOW_BATTERY_PCT => palette::PROMPT_HOT,
        (false, _) => palette::DIM,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{LANDSCAPE, PORTRAIT};
    use platform_display::testing::Framebuffer;

    fn clock() -> ClockView {
        ClockView {
            time: Some((14, 37)),
            battery_pct: Some(82),
            charging: true,
        }
    }

    fn painted(layout: &Layout, clock: &ClockView) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::sized(layout.canvas);
        render(&mut fb, layout, clock).expect("a framebuffer render cannot fail");
        fb
    }

    /// One: the clock paints.
    #[test]
    fn the_clock_paints_pixels() {
        assert!(painted(&LANDSCAPE, &clock()).lit_pixels() > 0);
    }

    /// Many: each minute is a different picture — the time actually reaches the glass.
    #[test]
    fn each_minute_paints_differently() {
        let mut later: ClockView = clock();
        later.time = Some((14, 38));
        assert_ne!(
            painted(&LANDSCAPE, &clock()).pixels(),
            painted(&LANDSCAPE, &later).pixels()
        );
    }

    /// A charging battery reads differently from the same charge off the charger — the state the
    /// screen is named for.
    #[test]
    fn charging_reads_differently_from_running_on_battery() {
        let mut unplugged: ClockView = clock();
        unplugged.charging = false;
        assert_ne!(
            painted(&LANDSCAPE, &clock()).pixels(),
            painted(&LANDSCAPE, &unplugged).pixels()
        );
    }

    /// A low battery that is filling is a non-event: charging outranks low, so the reading does
    /// not go red on the charger and teach the owner to ignore the colour.
    #[test]
    fn a_low_battery_on_the_charger_is_not_hot() {
        let mut low_charging: ClockView = clock();
        low_charging.battery_pct = Some(5);
        let mut low_unplugged: ClockView = low_charging;
        low_unplugged.charging = false;
        assert_ne!(battery_colour(&low_charging), palette::PROMPT_HOT);
        assert_eq!(battery_colour(&low_unplugged), palette::PROMPT_HOT);
    }

    /// Zero, and out of range: a nonsense reading is clamped rather than drawn off the glass.
    #[test]
    fn a_nonsense_reading_stays_on_both_canvases() {
        let absurd: ClockView = ClockView {
            time: Some((u8::MAX, u8::MAX)),
            battery_pct: Some(u8::MAX),
            charging: false,
        };
        assert_eq!(painted(&LANDSCAPE, &absurd).escaped(), 0);
        assert_eq!(painted(&PORTRAIT, &absurd).escaped(), 0);
        assert_eq!(painted(&LANDSCAPE, &ClockView::default()).escaped(), 0);
    }

    /// A stick nobody has told the time to says so, rather than painting a confident midnight.
    ///
    /// The board has no RTC, so this is the state it boots into and stays in until the host
    /// syncs — the first thing an owner sees on a charger. `00:00` there is not a blank, it is a
    /// wrong reading, and the whole point is that it must not be mistakable for a real one.
    #[test]
    fn an_unsynced_clock_does_not_paint_a_confident_midnight() {
        let unsynced: ClockView = ClockView {
            time: None,
            ..clock()
        };
        let midnight: ClockView = ClockView {
            time: Some((0, 0)),
            ..clock()
        };
        assert_ne!(
            painted(&LANDSCAPE, &unsynced).pixels(),
            painted(&LANDSCAPE, &midnight).pixels(),
            "an unsynced clock is indistinguishable from a real midnight"
        );
        assert_ne!(time_colour(&unsynced), time_colour(&midnight));
        assert_eq!(painted(&PORTRAIT, &unsynced).escaped(), 0);
    }

    /// A board with no gauge says what it does know rather than painting a plausible number —
    /// and the two states are still told apart.
    #[test]
    fn a_board_with_no_gauge_says_only_what_it_knows() {
        let mut charging: ClockView = clock();
        charging.battery_pct = None;
        let mut on_battery: ClockView = charging;
        on_battery.charging = false;
        assert!(painted(&LANDSCAPE, &charging).lit_pixels() > 0);
        assert_ne!(
            painted(&LANDSCAPE, &charging).pixels(),
            painted(&LANDSCAPE, &on_battery).pixels()
        );
        assert_eq!(painted(&PORTRAIT, &on_battery).escaped(), 0);
    }

    /// Both ways up, because a charging stick is as often stood upright as laid flat.
    #[test]
    fn the_clock_draws_on_both_canvases() {
        let flat: Framebuffer = painted(&LANDSCAPE, &clock());
        let turned: Framebuffer = painted(&PORTRAIT, &clock());
        assert_ne!(flat.size(), turned.size());
        assert!(turned.lit_pixels() > 0);
    }
}
