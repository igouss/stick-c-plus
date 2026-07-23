//! The status field: the mood in a word, and what the buddy is doing about it.
//!
//! Two rows beside the creature (below it, held upright). The creature already says the mood in
//! pictures; this says it in a word, because a pixel-art wink and a pixel-art surprise are not
//! reliably distinguishable across a room, and because "3 RUNNING" is a fact no sprite carries.

use buddy_core::PersonaState;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use platform_display::{text_field, RenderError};

use crate::layout::Layout;
use crate::palette;
use crate::view::BuddyView;

/// The word for a mood, chosen to fit the narrow canvas' thirteen columns.
pub const fn persona_word(persona: PersonaState) -> &'static str {
    match persona {
        PersonaState::Sleep => "SLEEPING",
        PersonaState::Idle => "IDLE",
        PersonaState::Busy => "BUSY",
        PersonaState::Attention => "WAITING",
        PersonaState::Celebrate => "DONE!",
        PersonaState::Dizzy => "DIZZY",
        PersonaState::Heart => "THANKS",
    }
}

/// The colour a mood is named in: the two that want the owner are loud, the rest are calm.
const fn persona_colour(persona: PersonaState) -> Rgb565 {
    match persona {
        PersonaState::Attention => palette::PROMPT_WARM,
        PersonaState::Celebrate | PersonaState::Heart => palette::APPROVE,
        PersonaState::Sleep => palette::DIM,
        PersonaState::Idle | PersonaState::Busy | PersonaState::Dizzy => palette::PRIMARY,
    }
}

/// Draw the two status rows for `view`.
pub fn render<D>(
    target: &mut D,
    layout: &Layout,
    view: &BuddyView,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    text_field(
        target,
        layout.status_row(0),
        persona_colour(view.persona),
        layout.status_cols,
        layout.align,
        format_args!("{}", persona_word(view.persona)),
    )?;
    sessions(target, layout, view)
}

/// The second row: what the host is doing, or that there is no host.
///
/// The unlinked case is the one that matters. A buddy with no bridge derives
/// [`PersonaState::Idle`] — the same mood as a linked-but-quiet one — so without this row the
/// glass could not tell a calm desk from a dead daemon.
fn sessions<D>(
    target: &mut D,
    layout: &Layout,
    view: &BuddyView,
) -> Result<(), RenderError<D::Error>>
where
    D: DrawTarget<Color = Rgb565>,
{
    if !view.device.linked {
        return text_field(
            target,
            layout.status_row(1),
            palette::UNLINKED,
            layout.status_cols,
            layout.align,
            format_args!("NO LINK"),
        );
    }
    let colour: Rgb565 = if view.sessions_waiting > 0 {
        palette::PROMPT_WARM
    } else {
        palette::LABEL
    };
    text_field(
        target,
        layout.status_row(1),
        colour,
        layout.status_cols,
        layout.align,
        format_args!(
            "{}R {}W",
            capped(view.sessions_running),
            capped(view.sessions_waiting)
        ),
    )
}

/// The largest session count the two-digit field can show.
const COUNT_CAP: u32 = 99;

/// A count as the status row shows it, saturating at [`COUNT_CAP`].
///
/// The field is two digits wide because thirteen columns is what the narrow canvas has, and a
/// count that grew a third digit would push the row past the edge — or, with the fixed-width
/// text primitive, refuse to draw at all. Nobody has a hundred concurrent Claude sessions; if
/// they do, "99" is a better answer on a desk pet than a blank row.
const fn capped(count: u32) -> u32 {
    if count > COUNT_CAP {
        COUNT_CAP
    } else {
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::LANDSCAPE;
    use buddy_core::SpeciesIndex;
    use platform_display::testing::Framebuffer;

    fn linked() -> BuddyView {
        let mut view: BuddyView = BuddyView::resting(SpeciesIndex::new(0));
        view.device.linked = true;
        view
    }

    fn painted(view: &BuddyView) -> Framebuffer {
        let mut fb: Framebuffer = Framebuffer::new();
        render(&mut fb, &LANDSCAPE, view).expect("a framebuffer render cannot fail");
        fb
    }

    /// Every mood has a word, and every word fits the narrow canvas — the shape that decides
    /// whether a mood has to be abbreviated, checked against the real column count.
    #[test]
    fn every_moods_word_fits_the_narrow_canvas() {
        let longest: usize = [
            PersonaState::Sleep,
            PersonaState::Idle,
            PersonaState::Busy,
            PersonaState::Attention,
            PersonaState::Celebrate,
            PersonaState::Dizzy,
            PersonaState::Heart,
        ]
        .iter()
        .map(|persona: &PersonaState| persona_word(*persona).len())
        .max()
        .expect("the mood table is not empty");
        assert!(longest <= crate::layout::PORTRAIT.status_cols);
    }

    /// One: a status reaches the glass.
    #[test]
    fn a_status_paints_pixels() {
        assert!(painted(&linked()).lit_pixels() > 0);
    }

    /// Many: two moods are two different pictures — the word actually tracks the persona.
    #[test]
    fn two_moods_paint_differently() {
        let mut busy: BuddyView = linked();
        busy.persona = PersonaState::Busy;
        let mut waiting: BuddyView = linked();
        waiting.persona = PersonaState::Attention;
        assert_ne!(painted(&busy).pixels(), painted(&waiting).pixels());
    }

    /// THE ONE THAT MATTERS: an unlinked buddy is idle, and so is a linked-but-quiet one. The
    /// second row is the only thing on the glass that tells a calm desk from a dead daemon.
    #[test]
    fn an_unlinked_buddy_looks_different_from_a_quiet_one() {
        let mut unlinked: BuddyView = linked();
        unlinked.device.linked = false;
        assert_eq!(unlinked.persona, linked().persona);
        assert_ne!(painted(&unlinked).pixels(), painted(&linked()).pixels());
    }

    /// The session counts reach the glass rather than a label that ignores them.
    #[test]
    fn a_changed_session_count_paints_differently() {
        let mut one: BuddyView = linked();
        one.sessions_running = 1;
        let mut three: BuddyView = linked();
        three.sessions_running = 3;
        assert_ne!(painted(&one).pixels(), painted(&three).pixels());
    }

    /// Nothing escapes the canvas, at the widest word and the largest counts.
    #[test]
    fn the_widest_status_stays_on_the_glass() {
        let mut wide: BuddyView = linked();
        wide.persona = PersonaState::Sleep;
        wide.sessions_running = u32::MAX;
        wide.sessions_waiting = u32::MAX;
        assert_eq!(painted(&wide).escaped(), 0);
    }
}
