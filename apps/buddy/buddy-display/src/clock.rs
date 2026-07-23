//! The charging clock: what the glass shows when the buddy is docked and nothing is happening.
//!
//! A stick on a charger is furniture, and furniture may as well be a clock. The persona still
//! runs underneath — `buddy_core::charging_mood` writes the mood directly while docked — but the
//! *picture* is the time, because that is the thing an idle desk object is asked for.
//!
//! Both ways up, deliberately. A charging stick is as often stood on its USB-C port as laid flat,
//! and this is the one screen whose whole job is to be read from across the desk.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_display::{text_field, FieldAlign, RenderError};

use crate::layout::Layout;
use crate::page;
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

    page::blank(target, layout, 0, layout.rows)?;
    text_field(
        target,
        layout.row_origin(time_row),
        palette::PRIMARY,
        layout.cols,
        FieldAlign::Centred,
        format_args!("{:02}:{:02}", clock.hour.min(23), clock.minute.min(59)),
    )?;
    text_field(
        target,
        layout.row_origin(time_row + 1),
        battery_colour(clock),
        layout.cols,
        FieldAlign::Centred,
        format_args!(
            "{}%{}",
            clock.battery_pct.min(100),
            if clock.charging { " CHG" } else { "" }
        ),
    )
}

/// The colour of the battery reading: charging is calm, a low battery off the charger is not.
///
/// Charging outranks low, because a low battery that is *filling* is a non-event and colouring
/// it red would teach the owner to ignore the colour.
const fn battery_colour(clock: &ClockView) -> Rgb565 {
    if clock.charging {
        palette::APPROVE
    } else if clock.battery_pct <= LOW_BATTERY_PCT {
        palette::PROMPT_HOT
    } else {
        palette::DIM
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{LANDSCAPE, PORTRAIT};
    use platform_display::testing::Framebuffer;

    fn clock() -> ClockView {
        ClockView {
            hour: 14,
            minute: 37,
            battery_pct: 82,
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
        later.minute = 38;
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
        low_charging.battery_pct = 5;
        let mut low_unplugged: ClockView = low_charging;
        low_unplugged.charging = false;
        assert_ne!(battery_colour(&low_charging), palette::PROMPT_HOT);
        assert_eq!(battery_colour(&low_unplugged), palette::PROMPT_HOT);
    }

    /// Zero, and out of range: a nonsense reading is clamped rather than drawn off the glass.
    #[test]
    fn a_nonsense_reading_stays_on_both_canvases() {
        let absurd: ClockView = ClockView {
            hour: u8::MAX,
            minute: u8::MAX,
            battery_pct: u8::MAX,
            charging: false,
        };
        assert_eq!(painted(&LANDSCAPE, &absurd).escaped(), 0);
        assert_eq!(painted(&PORTRAIT, &absurd).escaped(), 0);
        assert_eq!(painted(&LANDSCAPE, &ClockView::default()).escaped(), 0);
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
