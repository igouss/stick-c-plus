//! [`BuddyView`] — everything on the glass at one instant, as a plain value.
//!
//! The generic render loop remembers the last state it painted and compares it to the next, so
//! a view is `Copy + Eq + Send + 'static`. That shapes every type here: the text is
//! [`FixedStr`], never a `String` or a borrow, and the transcript is a fixed number of slots
//! rather than a growable list. The firmware fills a view from the merged wire snapshot each
//! frame; nothing here reads a clock, a socket, or a pin.
//!
//! ## The anchor is the persona, and only the persona
//!
//! [`Animated::anchor`] is what restarts the creature's animation clock, and the approval
//! screen's elapsed-seconds counter ticks once a second while a prompt is pending. If the
//! counter were part of the anchor, the creature would restart its animation every second and
//! never finish a loop. So the anchor is `(persona, species)` — it changes on a real persona
//! transition and on nothing else — while [`Eq`] still compares *everything drawn*, which is
//! what makes the ticking counter reach the glass at all.

use buddy_core::{MenuEntry, PersonaState, SpeciesIndex};
use platform_core::{Animated, FixedStr, Tick};

/// Characters of a tool name the glass can show.
pub const TOOL_CAP: usize = 24;
/// Characters of a prompt hint the glass can show — two full landscape lines.
pub const HINT_CAP: usize = 48;
/// Characters of one transcript entry the glass can show.
pub const ENTRY_CAP: usize = 64;
/// Characters of a short device field — a name, a firmware version.
pub const FIELD_CAP: usize = 24;

/// Transcript entries a view carries. The band shows at most three wrapped lines in landscape,
/// so four entries is more than can ever be on the glass at once — enough that a short entry
/// never leaves the band half empty, and bounded so the view stays a small value.
pub const TRANSCRIPT_SLOTS: usize = 4;

/// A tool name, as the glass can hold it.
pub type Tool = FixedStr<TOOL_CAP>;
/// A prompt hint, as the glass can hold it.
pub type Hint = FixedStr<HINT_CAP>;
/// One transcript entry, as the glass can hold it.
pub type Entry = FixedStr<ENTRY_CAP>;
/// A short device field, as the glass can hold it.
pub type Field = FixedStr<FIELD_CAP>;

/// The seconds a prompt may wait before the approval screen turns hot.
///
/// Not a timeout — nothing expires here. It is the point at which the colour stops being
/// informational and starts asking, which on a desk pet is the whole user interface.
pub const HOT_AFTER_S: u32 = 10;

/// A permission prompt waiting for the owner's answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PromptView {
    /// The tool the prompt is about.
    pub tool: Tool,
    /// A short human hint — the command, the path, the URL.
    pub hint: Hint,
    /// Whole seconds since the prompt arrived. Injected, like every other time here.
    pub waiting_s: u32,
}

impl PromptView {
    /// Whether the prompt has waited long enough to be drawn hot.
    pub const fn is_hot(&self) -> bool {
        self.waiting_s >= HOT_AFTER_S
    }
}

/// The transcript, newest first, with the count the host actually holds.
///
/// `total` is *not* `held`: the band shows the newest few and the scroll indicator shows how
/// much more there is behind them, so the two numbers are different questions.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Transcript {
    entries: [Entry; TRANSCRIPT_SLOTS],
    held: usize,
    total: usize,
}

impl Transcript {
    /// The transcript for a wire entry list, which arrives **oldest first**.
    ///
    /// Keeps the newest [`TRANSCRIPT_SLOTS`] and stores them newest first, so the renderer
    /// draws index 0 as the bright line without knowing anything about wire order. `total`
    /// records the whole list's length, which is what the scroll indicator is drawn from.
    pub fn oldest_first(entries: &[&str]) -> Self {
        let mut held: [Entry; TRANSCRIPT_SLOTS] = [Entry::empty(); TRANSCRIPT_SLOTS];
        let newest: &[&str] = &entries[entries.len().saturating_sub(TRANSCRIPT_SLOTS)..];
        newest
            .iter()
            .rev()
            .enumerate()
            .for_each(|(slot, text): (usize, &&str)| held[slot] = Entry::new(text));
        Transcript {
            entries: held,
            held: newest.len(),
            total: entries.len(),
        }
    }

    /// The entries the view carries, newest first.
    pub fn newest_first(&self) -> &[Entry] {
        &self.entries[..self.held]
    }

    /// How many entries the host holds in all — the scroll indicator's denominator.
    pub const fn total(&self) -> usize {
        self.total
    }

    /// Whether there is nothing to show.
    pub const fn is_empty(&self) -> bool {
        self.held == 0
    }
}

/// The stats the pet screen reads, already reduced by `buddy_core::stats`.
///
/// Tiers rather than raw sensor values: the domain owns the arithmetic, and the glass owns only
/// how many cells of a meter are lit.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StatsView {
    /// `buddy_core::mood_tier`, `0..=4`.
    pub mood: u8,
    /// `buddy_core::fed_progress`, `0..=9`.
    pub fed: u8,
    /// `buddy_core::energy_tier`, `0..=5`.
    pub energy: u8,
    /// `buddy_core::level`.
    pub level: u32,
    /// Approvals the owner has granted.
    pub approvals: u32,
    /// Denials the owner has issued.
    pub denials: u32,
    /// Naps taken.
    pub naps: u32,
    /// Whole minutes spent napping.
    pub nap_minutes: u32,
    /// Today's token count.
    pub tokens_today: u32,
}

/// The wall clock, as the charging screen shows it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ClockView {
    /// Hour of day, `0..=23`.
    pub hour: u8,
    /// Minute, `0..=59`.
    pub minute: u8,
    /// Battery charge in percent, when the board can actually measure it.
    ///
    /// `Option`, not a `u8` with a sentinel: this board's PMIC adapter reports whether USB is
    /// present and nothing else, so a firmware that has no gauge must be able to say *nothing*
    /// rather than paint a plausible number. A fabricated `82%` on the glass is worse than a
    /// blank — it is a reading the owner would believe.
    pub battery_pct: Option<u8>,
    /// Whether the board is on external power.
    pub charging: bool,
}

/// What the info screen's device and bluetooth pages report.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DeviceView {
    /// The advertised name, e.g. `Claude-4F2A`.
    pub name: Field,
    /// The firmware version.
    pub firmware: Field,
    /// The Bluetooth address.
    pub address: Field,
    /// The owner label, as set from the menu.
    pub owner: Field,
    /// Whether a central is bonded.
    pub bonded: bool,
    /// Whether the bridge link is live.
    pub linked: bool,
}

/// Which screen the buddy is showing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Screen {
    /// The creature and its transcript HUD — the resting screen.
    #[default]
    Home,
    /// The pet screen, at one of its two pages.
    Pet(PetPage),
    /// The info screen, at one of its six pages.
    Info(InfoPage),
    /// The charging clock.
    Clock,
}

/// The pet screen's two pages.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PetPage {
    /// Mood, fed, energy, level, counts, nap time, tokens.
    #[default]
    Stats,
    /// How the pet works — what feeds it, what tires it, what makes it happy.
    HowTo,
}

/// The info screen's six pages, in cursor order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InfoPage {
    /// What this thing is.
    #[default]
    About,
    /// What the two buttons do.
    Buttons,
    /// What it does with Claude Code.
    Claude,
    /// This board: firmware, name, owner.
    Device,
    /// The BLE link: address, bond, bridge.
    Bluetooth,
    /// Whose art and whose code.
    Credits,
}

impl InfoPage {
    /// The pages in cursor order — the table the screen and its tests both walk.
    pub const ALL: [InfoPage; 6] = [
        InfoPage::About,
        InfoPage::Buttons,
        InfoPage::Claude,
        InfoPage::Device,
        InfoPage::Bluetooth,
        InfoPage::Credits,
    ];
}

/// An overlay drawn on top of whatever screen is underneath.
///
/// At most one is on the glass, because they *nest*: the reset confirmation is reached from
/// settings, and settings from the menu. So the innermost open thing wins — see
/// [`Overlay::priority`], which is what the compositor picks by.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Overlay {
    /// Nothing on top; the screen underneath is the whole picture.
    #[default]
    None,
    /// The settings menu, with the cursor resting on `MENU_ENTRIES[cursor]`.
    Menu {
        /// The highlighted entry index into `buddy_core::MENU_ENTRIES`.
        cursor: u8,
    },
    /// A settings entry opened for reading or editing.
    Settings {
        /// The entry being shown.
        entry: MenuEntry,
    },
    /// The destructive confirmation: forget the bond and start over.
    Reset,
}

impl Overlay {
    /// How far in this overlay is — higher wins when more than one could be drawn.
    ///
    /// The order is the *nesting*, not a taste: reset is opened from settings and settings from
    /// the menu, so the innermost is the one the owner is actually looking at. A compositor
    /// that drew the menu over its own confirmation dialog would hide the question it was
    /// asking.
    pub const fn priority(&self) -> u8 {
        match self {
            Overlay::None => 0,
            Overlay::Menu { .. } => 1,
            Overlay::Settings { .. } => 2,
            Overlay::Reset => 3,
        }
    }

    /// Whether anything is drawn on top.
    pub const fn is_open(&self) -> bool {
        !matches!(self, Overlay::None)
    }
}

/// Everything on the buddy's glass at one instant.
///
/// Built by the firmware from the domain and the merged wire snapshot each frame, then handed
/// to the render loop and to [`render`](crate::render).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BuddyView {
    /// The mood the creature wears.
    pub persona: PersonaState,
    /// The chosen creature.
    pub species: SpeciesIndex,
    /// Which screen is showing.
    pub screen: Screen,
    /// What is drawn on top of it.
    pub overlay: Overlay,
    /// A pairing passkey being displayed. `Some` **takes over the whole glass**: the number has
    /// to be typed on the peer within seconds, and nothing else on the device matters until it
    /// has been.
    pub passkey: Option<u32>,
    /// A permission prompt waiting for an answer, which replaces the transcript HUD.
    pub prompt: Option<PromptView>,
    /// The transcript behind the HUD.
    pub transcript: Transcript,
    /// The stats the pet screen reads.
    pub stats: StatsView,
    /// The wall clock the charging screen shows.
    pub clock: ClockView,
    /// What the info screen reports about this board.
    pub device: DeviceView,
    /// Sessions currently running, for the status field.
    pub sessions_running: u32,
    /// Sessions waiting on the owner, for the status field.
    pub sessions_waiting: u32,
}

impl BuddyView {
    /// A resting view of `species`: idle, at home, nothing pending.
    ///
    /// The starting point a firmware or a test fills in from, so adding a field to the view does
    /// not mean editing every construction of one.
    pub fn resting(species: SpeciesIndex) -> Self {
        BuddyView {
            persona: PersonaState::Idle,
            species,
            screen: Screen::Home,
            overlay: Overlay::None,
            passkey: None,
            prompt: None,
            transcript: Transcript::default(),
            stats: StatsView::default(),
            clock: ClockView::default(),
            device: DeviceView::default(),
            sessions_running: 0,
            sessions_waiting: 0,
        }
    }

    /// Whether the creature is on the glass at all.
    ///
    /// The passkey takeover and the pages that are pure text draw no creature, so nothing about
    /// them needs an animation cadence.
    pub const fn shows_creature(&self) -> bool {
        self.passkey.is_none() && matches!(self.screen, Screen::Home)
    }
}

impl Animated for BuddyView {
    /// The persona and the species — deliberately **not** the prompt's second counter, the
    /// transcript, or the screen's page. The creature animates on the mood's clock; a HUD value
    /// ticking underneath it must not restart that animation.
    type Anchor = (PersonaState, SpeciesIndex);

    fn anchor(&self) -> (PersonaState, SpeciesIndex) {
        (self.persona, self.species)
    }

    fn is_animated(&self) -> bool {
        self.shows_creature()
    }

    fn frame_index(&self, elapsed_ms: Tick) -> usize {
        crate::creature::selected(self.species, self.persona, elapsed_ms)
            .map(|selected: buddy_creature::Selected| selected.frame_index)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one species the registry holds today.
    const CLAUDEPIX: SpeciesIndex = SpeciesIndex::new(0);

    fn resting() -> BuddyView {
        BuddyView::resting(CLAUDEPIX)
    }

    /// Zero: an empty transcript holds nothing and reports nothing behind it.
    #[test]
    fn an_empty_transcript_is_empty() {
        let transcript: Transcript = Transcript::oldest_first(&[]);
        assert!(transcript.is_empty());
        assert_eq!(transcript.total(), 0);
        assert!(transcript.newest_first().is_empty());
    }

    /// One: a single entry is the newest entry.
    #[test]
    fn one_entry_is_the_newest_entry() {
        let transcript: Transcript = Transcript::oldest_first(&["ran the tests"]);
        assert_eq!(transcript.newest_first()[0].as_str(), "ran the tests");
        assert_eq!(transcript.total(), 1);
    }

    /// Many: the wire's oldest-first order is reversed, so index 0 is the newest.
    #[test]
    fn the_wire_order_is_reversed_so_index_zero_is_newest() {
        let transcript: Transcript = Transcript::oldest_first(&["first", "second", "third"]);
        assert_eq!(transcript.newest_first()[0].as_str(), "third");
        assert_eq!(transcript.newest_first()[2].as_str(), "first");
    }

    /// More entries than there are slots: the newest are kept, the oldest dropped, and `total`
    /// still reports the whole list — which is exactly what the scroll indicator draws.
    #[test]
    fn a_long_transcript_keeps_the_newest_and_still_counts_the_rest() {
        let transcript: Transcript =
            Transcript::oldest_first(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        assert_eq!(transcript.newest_first().len(), TRANSCRIPT_SLOTS);
        assert_eq!(transcript.newest_first()[0].as_str(), "h");
        assert_eq!(transcript.total(), 8);
    }

    /// The anchor changes on a persona transition — the creature re-animates on a real mood
    /// change.
    #[test]
    fn a_persona_change_moves_the_anchor() {
        let mut busy: BuddyView = resting();
        busy.persona = PersonaState::Busy;
        assert_ne!(resting().anchor(), busy.anchor());
    }

    /// THE LOAD-BEARING ONE: a ticking prompt counter changes the VIEW (so the glass repaints)
    /// but not the ANCHOR (so the creature keeps animating). A view that folded the counter into
    /// its anchor would restart the animation every second and never finish a loop.
    #[test]
    fn a_ticking_prompt_counter_repaints_without_restarting_the_animation() {
        let prompt = |waiting_s: u32| PromptView {
            tool: Tool::new("Bash"),
            hint: Hint::new("cargo test --workspace"),
            waiting_s,
        };
        let mut at_three: BuddyView = resting();
        at_three.prompt = Some(prompt(3));
        let mut at_four: BuddyView = resting();
        at_four.prompt = Some(prompt(4));

        assert_ne!(at_three, at_four, "the new second must reach the glass");
        assert_eq!(
            at_three.anchor(),
            at_four.anchor(),
            "the new second must not restart the creature"
        );
    }

    /// The same for the transcript: a new entry repaints without re-anchoring.
    #[test]
    fn a_new_transcript_entry_repaints_without_restarting_the_animation() {
        let mut before: BuddyView = resting();
        before.transcript = Transcript::oldest_first(&["read the bead"]);
        let mut after: BuddyView = resting();
        after.transcript = Transcript::oldest_first(&["read the bead", "wrote the crate"]);

        assert_ne!(before, after);
        assert_eq!(before.anchor(), after.anchor());
    }

    /// A prompt turns hot at ten seconds and not before — the boundary, stated on both sides.
    #[test]
    fn a_prompt_turns_hot_at_ten_seconds() {
        let prompt = |waiting_s: u32| PromptView {
            tool: Tool::new("Bash"),
            hint: Hint::new("rm -rf build"),
            waiting_s,
        };
        assert!(!prompt(9).is_hot());
        assert!(prompt(10).is_hot());
    }

    /// The creature is on the glass at home and nowhere else — the passkey takes the whole
    /// panel, and the pages are text.
    #[test]
    fn the_creature_shows_at_home_and_not_under_a_takeover_or_a_page() {
        assert!(resting().shows_creature());

        let mut with_passkey: BuddyView = resting();
        with_passkey.passkey = Some(482_913);
        assert!(!with_passkey.shows_creature());

        let mut on_a_page: BuddyView = resting();
        on_a_page.screen = Screen::Info(InfoPage::About);
        assert!(!on_a_page.shows_creature());
    }

    /// The overlays nest, so the innermost wins. A menu drawn over its own reset confirmation
    /// would hide the question being asked.
    #[test]
    fn the_overlays_rank_by_how_far_in_they_are() {
        assert!(
            Overlay::Reset.priority()
                > Overlay::Settings {
                    entry: MenuEntry::Unpair
                }
                .priority()
        );
        assert!(
            Overlay::Settings {
                entry: MenuEntry::Unpair
            }
            .priority()
                > Overlay::Menu { cursor: 0 }.priority()
        );
        assert!(Overlay::Menu { cursor: 0 }.priority() > Overlay::None.priority());
        assert!(!Overlay::None.is_open());
        assert!(Overlay::Reset.is_open());
    }
}
