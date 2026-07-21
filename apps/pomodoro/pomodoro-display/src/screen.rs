//! The pomodoro screen: a phase label, a `MM:SS` clock, and the creature that stands for it.
//!
//! Device-independent by construction — it draws into any [`DrawTarget`], so the on-target
//! panel and a host framebuffer render *the same code*.

use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_core::ScreenRotation;
use platform_display::{sprite, text_line, RenderError};
use pomodoro_core::{Phase, Status};

use crate::layout::{
    CLOCK_ORIGIN, CLOCK_WIDTH, LABEL_ORIGIN, LABEL_WIDTH, SPRITE_ORIGIN, SPRITE_SCALE,
};
use crate::scene;
use crate::view::PomodoroView;

/// The word shown for a pomodoro state, chosen for a 240-pixel panel: what you are doing now,
/// or `DONE` at a boundary waiting for the next tap.
pub const fn label_text(view: PomodoroView) -> &'static str {
    match view.status {
        Status::Idle => "READY",
        Status::Paused => "PAUSED",
        Status::Finished => "DONE",
        Status::Running => match view.phase {
            Phase::Focus => "FOCUS",
            Phase::ShortBreak => "BREAK",
            Phase::LongBreak => "LONG BREAK",
        },
    }
}

/// The colour of the phase label: focus is red, a short break green, a long break cyan — so a
/// glance names the phase by colour before the eye reads the word.
const fn label_colour(view: PomodoroView) -> Rgb565 {
    match view.phase {
        Phase::Focus => Rgb565::RED,
        Phase::ShortBreak => Rgb565::GREEN,
        Phase::LongBreak => Rgb565::CYAN,
    }
}

/// Render the pomodoro screen: the phase label, the `MM:SS` clock, and the creature.
///
/// `elapsed_ms` is how long this state has been current — the creature's animation clock. A
/// still state (idle, paused) ignores it and shows frame 0, so the render loop repaints
/// nothing between seconds; a running or finished state animates.
pub fn render<D>(
    target: &mut D,
    view: PomodoroView,
    elapsed_ms: u64,
    // This screen is landscape-only for now: it takes the rotation the platform supplies
    // and does not yet honour it. A portrait layout is its own bead.
    _rotation: ScreenRotation,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    label(target, view)?;
    clock(target, view)?;
    creature(target, view, elapsed_ms)
}

/// Paint the phase label row.
fn label<D>(target: &mut D, view: PomodoroView) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_line(
        target,
        LABEL_ORIGIN,
        label_colour(view),
        LABEL_WIDTH,
        format_args!("{}", label_text(view)),
    )
}

/// Paint the `MM:SS` clock row.
fn clock<D>(target: &mut D, view: PomodoroView) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let (minutes, seconds): (u32, u32) = view.minutes_seconds();
    text_line(
        target,
        CLOCK_ORIGIN,
        Rgb565::WHITE,
        CLOCK_WIDTH,
        format_args!("{minutes:02}:{seconds:02}"),
    )
}

/// Paint the creature for `view` in the panel's right-hand region.
///
/// Drawn **opaque** (see [`sprite::draw_onto`]): each frame overwrites its own box against the
/// black background, so an animating creature never smears the frame before it.
fn creature<D>(
    target: &mut D,
    view: PomodoroView,
    elapsed_ms: u64,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    let sprite: &sprite::Sprite = scene::creature(view.phase, view.status);
    let index: usize = scene::frame_index(view.phase, view.status, elapsed_ms);
    sprite::draw_onto(
        target,
        sprite,
        &sprite.frames()[index],
        SPRITE_ORIGIN,
        SPRITE_SCALE,
        Rgb565::BLACK,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_display::testing::Framebuffer;

    fn view(phase: Phase, status: Status, remaining_secs: u32) -> PomodoroView {
        PomodoroView {
            phase,
            status,
            remaining_secs,
        }
    }

    /// Paint `view` at the instant it appeared.
    fn painted(view: PomodoroView) -> Framebuffer {
        painted_at(view, 0)
    }

    fn painted_at(view: PomodoroView, elapsed_ms: u64) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        render(&mut fb, view, elapsed_ms, ScreenRotation::Deg0)
            .expect("a framebuffer render cannot fail");
        fb
    }

    /// Zero: an unrendered canvas is blank.
    #[test]
    fn an_unrendered_canvas_is_blank() {
        assert_eq!(Framebuffer::new().lit_pixels(), 0);
    }

    /// One: a running focus puts ink on the glass.
    #[test]
    fn a_running_focus_paints_pixels() {
        assert!(painted(view(Phase::Focus, Status::Running, 1_500)).lit_pixels() > 0);
    }

    /// Many: a focus, a break, and a finished state paint three distinct pictures — a glance
    /// tells them apart.
    #[test]
    fn focus_break_and_finished_paint_distinct_pictures() {
        let focus: Framebuffer = painted(view(Phase::Focus, Status::Running, 1_500));
        let brk: Framebuffer = painted(view(Phase::ShortBreak, Status::Running, 300));
        let done: Framebuffer = painted(view(Phase::Focus, Status::Finished, 0));
        assert_ne!(focus.pixels(), brk.pixels());
        assert_ne!(brk.pixels(), done.pixels());
        assert_ne!(focus.pixels(), done.pixels());
    }

    /// A changed clock value repaints differently — the seconds actually reach the glass,
    /// rather than a label that ignores them.
    #[test]
    fn a_different_clock_value_paints_differently() {
        assert_ne!(
            painted(view(Phase::Focus, Status::Running, 1_500)).pixels(),
            painted(view(Phase::Focus, Status::Running, 1_499)).pixels()
        );
    }

    /// A running creature animates: the same state, one frame-hold later, is a different
    /// picture.
    #[test]
    fn a_running_creature_animates() {
        let running: PomodoroView = view(Phase::Focus, Status::Running, 1_500);
        let hold: u64 =
            u64::from(scene::creature(running.phase, running.status).frames()[0].hold_ms());
        assert_ne!(
            painted_at(running, 0).pixels(),
            painted_at(running, hold).pixels(),
            "the coding creature never moved"
        );
    }

    /// A paused creature is still: however long it has been up, the picture is identical, so
    /// the render loop finds nothing to repaint.
    #[test]
    fn a_paused_screen_never_changes_however_long_it_has_been_up() {
        let paused: PomodoroView = view(Phase::Focus, Status::Paused, 750);
        assert_eq!(
            painted_at(paused, 0).pixels(),
            painted_at(paused, u64::MAX).pixels()
        );
    }

    /// Nothing escapes the canvas in any state, including the widest label and the largest
    /// clock value.
    #[test]
    fn no_state_escapes_the_canvas() {
        assert_eq!(
            painted(view(Phase::LongBreak, Status::Running, 15 * 60)).escaped(),
            0
        );
        assert_eq!(
            painted(view(Phase::Focus, Status::Idle, 25 * 60)).escaped(),
            0
        );
        assert_eq!(
            painted(view(Phase::Focus, Status::Finished, 0)).escaped(),
            0
        );
    }
}
