//! The device state: everything the stick knows, and the two ways it changes.
//!
//! A line arrives ([`apply`](DeviceState::apply)) or a button is pressed
//! ([`press`](DeviceState::press)); time passes ([`tick`](DeviceState::tick)); and
//! [`view`](DeviceState::view) is what the glass shows for all of it. No thread, no socket, no
//! pin — the composition root supplies those, so the whole control is exercised on the host.
//!
//! ## The fail-safe, restated on this side of the seam
//!
//! `buddy_wire::parse_inbound` makes it *structurally* impossible for anything but a snapshot to
//! reach the prompt state: a keepalive, a time sync, a transcript event and a command are each
//! their own variant, and only [`Inbound::Snapshot`] carries a packet that
//! [`SnapshotState::merge`] can touch. This module has to honour that rather than undo it, so
//! [`apply`](DeviceState::apply) matches exhaustively and the three non-snapshot arms do not go
//! near the prompt. An absent `prompt` field on a *real* snapshot legitimately means "gone", and
//! that is the only thing that clears one.
//!
//! ## The two buttons have a claim order
//!
//! 1. an **unanswered prompt** owns both buttons — A allows, B denies;
//! 2. the **reset confirmation** owns both — A confirms, B cancels;
//! 3. a **settings page** takes B to close;
//! 4. the **menu** takes what `buddy_core::Menu` dispatches;
//! 5. otherwise **navigation** — A turns the page, B walks the screens.
//!
//! The prompt is first because it is the only thing on the device that is blocking someone. A
//! press that arrived while the owner was in a menu still answers the prompt, which is what a
//! person pressing A while looking at an approval screen means.

use buddy_core::{
    charging_mood, Approval, Buddy, MenuEntry, MenuOutcome, MenuState, NapTransition, Outcome,
    PersonaState, SpeciesIndex, StepInput,
};
use buddy_display::{BuddyView, ClockView, Hint, Overlay, PromptView, Screen, Tool, Transcript};
use buddy_wire::{Command, Decision, Inbound, PermissionResponse, Prompt, SnapshotState};
use platform_core::{Acceleration, Tick};
use platform_input::{ButtonEvent, ButtonId, Gesture};

use crate::identity::Identity;
use crate::nav::Nav;
use crate::tally::Tally;
use crate::wall::WallClock;

/// How long after the last inbound line the link is still considered live.
///
/// The bridge heartbeats every ten seconds, so thirty is three missed beats — long enough that a
/// single dropped packet is not reported as a dead daemon, short enough that a bridge killed at
/// the terminal shows as `NO LINK` before the owner has finished wondering.
pub const LINK_WINDOW_MS: Tick = 30_000;

/// What a button press asks the composition root to do, beyond changing the picture.
///
/// Returned rather than performed, so this module names no BLE stack and no NVS: the effects are
/// data, and the root is the only thing that can carry them out.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Effect {
    /// Nothing to do outside the state.
    None,
    /// Send this permission response to the host — the headline effect.
    Answer(PermissionResponse),
    /// Forget the BLE bond and start over.
    ForgetBond,
    /// The selected species changed; persist it.
    PersistSpecies(SpeciesIndex),
}

/// Everything the stick knows.
#[derive(Clone, Debug)]
pub struct DeviceState {
    identity: Identity,
    snapshot: SnapshotState,
    buddy: Buddy,
    tally: Tally,
    nav: Nav,
    wall: WallClock,
    /// When the prompt currently on the glass arrived — the approval screen's counter.
    prompt_since: Tick,
    /// The prompt id already answered, so a second press does not answer it twice and the
    /// approval screen comes down as soon as the owner has decided.
    answered: Option<String>,
    /// A settings entry opened from the menu.
    settings: Option<MenuEntry>,
    /// Whether the destructive confirmation is up.
    reset_pending: bool,
    /// A pairing passkey being shown — takes over the whole glass.
    passkey: Option<u32>,
    /// When the last inbound line arrived, and whether any ever has.
    last_inbound: Option<Tick>,
    /// Whether a central is bonded, as the composition root last reported it.
    bonded: bool,
    /// The last sensor reading, so a button press can be folded through `step` without waiting
    /// for the next tick to resample.
    accel: Acceleration,
    /// Whether the glass is lit.
    screen_on: bool,
    /// Whether the board is on external power.
    charging: bool,
    /// Battery charge in percent, when a gauge is wired. This board's PMIC adapter reports only
    /// whether USB is present, so today it is `None` and the clock says so.
    battery_pct: Option<u8>,
    /// The tick of the last [`tick`](DeviceState::tick) — the clock every view is built at.
    now: Tick,
    /// The persona the last `step` resolved.
    persona: PersonaState,
}

impl DeviceState {
    /// A freshly booted stick: awake, at home, unlinked, nothing pending.
    pub fn new(identity: Identity, species: SpeciesIndex) -> Self {
        DeviceState {
            identity,
            snapshot: SnapshotState::default(),
            buddy: Buddy::new(species),
            tally: Tally::new(),
            nav: Nav::new(),
            wall: WallClock::new(),
            prompt_since: 0,
            answered: None,
            settings: None,
            reset_pending: false,
            passkey: None,
            last_inbound: None,
            bonded: false,
            accel: Acceleration::new(0, 0, 1_000),
            screen_on: true,
            charging: false,
            battery_pct: None,
            now: 0,
            persona: PersonaState::Idle,
        }
    }

    /// Fold one inbound wire message in at `now`.
    ///
    /// Exhaustive over the four shapes, and only the snapshot arm goes near the prompt — see the
    /// module docs. Every shape refreshes the link liveness, because any line at all is proof
    /// the bridge is there.
    pub fn apply(&mut self, inbound: Inbound, now: Tick) {
        self.last_inbound = Some(now);
        match inbound {
            Inbound::Snapshot(packet) => {
                let before: Option<String> = self.prompt_id();
                if let Some(total) = self.snapshot.merge(&packet) {
                    self.credit(total, now);
                }
                let after: Option<String> = self.prompt_id();
                if after != before {
                    // A NEW prompt (or a cleared one): restart the counter and forget that the
                    // previous one was answered, so the next prompt is answerable.
                    self.prompt_since = now;
                    self.answered = None;
                }
            }
            Inbound::Time { epoch, tz_offset_s } => self.wall.sync(epoch, tz_offset_s, now),
            Inbound::Command(command) => self.command(command),
            // A transcript event is a heartbeat with a name on it. It must not disturb a pending
            // decision — this empty arm is the point of the exhaustive match.
            Inbound::Event(_) => {}
        }
    }

    /// Act on a host command. Only the two that change what is on the glass are handled here;
    /// the rest are the transport's business and are acked there.
    fn command(&mut self, command: Command) {
        match command {
            Command::Name(name) => self.identity.name = name,
            Command::Owner(owner) => self.identity.owner = owner,
            Command::Species(index) => self.buddy.species = SpeciesIndex::new(index),
            _ => {}
        }
    }

    /// Credit a desktop token total through the domain's latch, arming the celebrate one-shot if
    /// it crosses a level.
    fn credit(&mut self, total: u32, now: Tick) {
        let earned: u32 = self.buddy.tokens.observe(total);
        let milestone: bool = self.tally.would_level_up(earned);
        self.tally.credit(earned);
        self.tally.tokens_today = self.snapshot.tokens_today;
        if milestone {
            self.step(now, None, None, true);
        }
    }

    /// Advance time: resample the sensors, fold a frame through the domain, and let the charging
    /// clock take or release the glass.
    pub fn tick(&mut self, now: Tick, accel: Acceleration, screen_on: bool) {
        self.accel = accel;
        self.screen_on = screen_on;
        let outcome: Outcome = self.step(now, None, None, false);
        match outcome.nap {
            NapTransition::Entered => self.tally.nap_began(now),
            NapTransition::Left => self.tally.nap_ended(now),
            NapTransition::None => {}
        }
    }

    /// Report the power state. A board put on the charger shows the clock; taken off, it goes
    /// back home — but only from the clock, so a plug-in does not yank the owner off a page they
    /// were reading.
    pub fn power(&mut self, charging: bool, battery_pct: Option<u8>) {
        let docked: bool = charging && !self.charging;
        let undocked: bool = !charging && self.charging;
        self.charging = charging;
        self.battery_pct = battery_pct;
        if docked {
            self.nav.show(Screen::Clock);
        } else if undocked && self.nav.screen() == Screen::Clock {
            self.nav.show(Screen::Home);
        }
    }

    /// Show (or clear) a pairing passkey. `Some` takes over the whole glass.
    pub fn show_passkey(&mut self, passkey: Option<u32>) {
        self.passkey = passkey;
    }

    /// Report whether a central is bonded.
    pub fn set_bonded(&mut self, bonded: bool) {
        self.bonded = bonded;
    }

    /// Whether the bridge link is live — an inbound line inside [`LINK_WINDOW_MS`].
    pub fn is_linked(&self) -> bool {
        self.last_inbound
            .is_some_and(|at: Tick| self.now.saturating_sub(at) < LINK_WINDOW_MS)
    }

    /// Fold one button press in at `now`, returning what the composition root must do about it.
    ///
    /// The claim order is the module docs'. Nothing here performs an effect.
    pub fn press(&mut self, event: ButtonEvent, now: Tick) -> Effect {
        self.now = now;

        // 1. An unanswered prompt owns both buttons — it is the only thing blocking someone.
        if let Some(id) = self.unanswered_prompt_id() {
            match (event.button, event.gesture) {
                (ButtonId::Front, Gesture::Click) => return self.answer(id, Decision::Once, now),
                (ButtonId::Side, Gesture::Click) => return self.answer(id, Decision::Deny, now),
                _ => {}
            }
        }

        // 2. The destructive confirmation owns both. A cancel is the default for everything that
        //    is not an explicit A click, because the cost of the two mistakes is not symmetric.
        if self.reset_pending {
            self.reset_pending = false;
            self.settings = None;
            return match (event.button, event.gesture) {
                (ButtonId::Front, Gesture::Click) => {
                    self.close_menu();
                    Effect::ForgetBond
                }
                _ => Effect::None,
            };
        }

        // 3. A settings page: B closes it back to the menu. A hold falls through, so the menu's
        //    own close still works from inside a page.
        if self.settings.is_some()
            && (event.button, event.gesture) == (ButtonId::Side, Gesture::Click)
        {
            self.settings = None;
            return Effect::None;
        }

        // 4. The menu, while it is open — or a hold, which is what opens it.
        if self.buddy.menu.is_open() || event.gesture == Gesture::LongHold {
            let outcome: Outcome = self.step(now, Some(event), None, false);
            return self.act_on(outcome.menu);
        }

        // 5. Navigation.
        self.nav.press(event);
        Effect::None
    }

    /// Answer the prompt on the glass, and remember that it has been answered.
    fn answer(&mut self, id: String, decision: Decision, now: Tick) -> Effect {
        let took_s: u32 =
            (now.saturating_sub(self.prompt_since) / 1_000).min(u64::from(u32::MAX)) as u32;
        let approved: bool = decision == Decision::Once;
        self.answered = Some(id.clone());
        self.tally
            .answered(&mut self.buddy.velocity, approved, took_s);
        // Through the domain, so a fast approval arms the heart one-shot on the same frame.
        self.step(now, None, Some(Approval { approved, took_s }), false);
        Effect::Answer(PermissionResponse { id, decision })
    }

    /// Turn a menu outcome into an overlay change and, where it has one, an effect.
    fn act_on(&mut self, outcome: MenuOutcome) -> Effect {
        match outcome {
            MenuOutcome::Selected(MenuEntry::Species) => {
                let next: SpeciesIndex = self.buddy.species.cycle(species_count());
                self.buddy.species = next;
                self.settings = Some(MenuEntry::Species);
                Effect::PersistSpecies(next)
            }
            MenuOutcome::Selected(MenuEntry::Unpair) => {
                self.reset_pending = true;
                Effect::None
            }
            MenuOutcome::Selected(entry) => {
                self.settings = Some(entry);
                Effect::None
            }
            MenuOutcome::Closed => {
                self.settings = None;
                Effect::None
            }
            MenuOutcome::Opened | MenuOutcome::Moved | MenuOutcome::None => {
                self.settings = None;
                Effect::None
            }
        }
    }

    /// Close the menu by dispatching the hold that toggles it — rather than reaching into the
    /// domain's private state, which would be a second copy of the menu's rules.
    fn close_menu(&mut self) {
        if self.buddy.menu.is_open() {
            self.buddy
                .menu
                .dispatch(ButtonEvent::new(ButtonId::Front, Gesture::LongHold));
        }
        self.settings = None;
    }

    /// One turn of the domain's crank, from the cached sensor readings.
    fn step(
        &mut self,
        now: Tick,
        button: Option<ButtonEvent>,
        approval: Option<Approval>,
        level_up: bool,
    ) -> Outcome {
        self.now = now;
        let input: StepInput = StepInput {
            snapshot: buddy_core::Snapshot {
                connected: self.is_linked(),
                sessions_waiting: u32::from(self.snapshot.waiting),
                sessions_running: u32::from(self.snapshot.running),
                recently_completed: self.snapshot.completed,
            },
            now,
            accel: self.accel,
            screen_on: self.screen_on,
            menu_open: self.buddy.menu.is_open(),
            prompt_unanswered: self.unanswered_prompt_id().is_some(),
            level_up,
            approval,
            button,
            charging_mood: self.charging_persona(now),
        };
        let (buddy, outcome): (Buddy, Outcome) = buddy_core::step(self.buddy, &input);
        self.buddy = buddy;
        self.persona = outcome.persona;
        outcome
    }

    /// The charging clock's direct persona override, when there is one.
    ///
    /// Only while docked *and* the host has said what time it is: the schedule is a function of
    /// the hour and the day, and guessing either would put the buddy to sleep at the wrong time
    /// on a stick that has simply never been synced.
    fn charging_persona(&self, now: Tick) -> Option<PersonaState> {
        if !self.charging {
            return None;
        }
        self.wall
            .hour_and_day(now)
            .map(|(hour, day): (u8, u8)| charging_mood(hour, day, now))
    }

    /// The id of the prompt the host currently has pending, answered or not.
    fn prompt_id(&self) -> Option<String> {
        self.snapshot
            .prompt
            .as_ref()
            .map(|prompt: &Prompt| prompt.id.clone())
    }

    /// The id of the prompt still waiting for an answer, or `None`.
    fn unanswered_prompt_id(&self) -> Option<String> {
        let id: String = self.prompt_id()?;
        (self.answered.as_deref() != Some(id.as_str())).then_some(id)
    }

    /// The whole picture at the last known tick.
    pub fn view(&self) -> BuddyView {
        let mut view: BuddyView = BuddyView::resting(self.buddy.species);
        let linked: bool = self.is_linked();

        view.persona = self.persona;
        view.screen = self.nav.screen();
        view.overlay = self.overlay();
        view.passkey = self.passkey;
        view.prompt = self.prompt_view();
        view.transcript = self.transcript();
        view.stats = self.tally.view(&self.buddy.velocity, self.now);
        view.device = self.identity.view(self.bonded, linked);
        view.sessions_running = u32::from(self.snapshot.running);
        view.sessions_waiting = u32::from(self.snapshot.waiting);
        view.clock = ClockView {
            // Read once, not twice: an unsynced wall clock reports `None` and the glass shows
            // dashes rather than a midnight nobody set.
            time: self.wall.hour_minute(self.now),
            battery_pct: self.battery_pct,
            charging: self.charging,
        };
        view
    }

    /// Which overlay is on top: the innermost open thing.
    fn overlay(&self) -> Overlay {
        if self.reset_pending {
            return Overlay::Reset;
        }
        if let Some(entry) = self.settings {
            return Overlay::Settings { entry };
        }
        match self.buddy.menu.state() {
            MenuState::Open { cursor } => Overlay::Menu { cursor },
            MenuState::Closed => Overlay::None,
        }
    }

    /// The approval screen's view of the pending prompt — `None` once it has been answered, so
    /// the band goes back to the transcript the instant the owner has decided rather than
    /// waiting for the host to clear it.
    fn prompt_view(&self) -> Option<PromptView> {
        let id: String = self.unanswered_prompt_id()?;
        let prompt: &Prompt = self
            .snapshot
            .prompt
            .as_ref()
            .filter(|prompt: &&Prompt| prompt.id == id)?;
        Some(PromptView {
            tool: Tool::new(&prompt.tool),
            hint: Hint::new(&prompt.hint),
            waiting_s: (self.now.saturating_sub(self.prompt_since) / 1_000).min(u64::from(u32::MAX))
                as u32,
        })
    }

    /// The transcript, as the HUD reads it.
    fn transcript(&self) -> Transcript {
        let entries: Vec<&str> = self
            .snapshot
            .entries
            .iter()
            .map(|entry: &String| entry.as_str())
            .collect();
        Transcript::oldest_first(&entries)
    }
}

/// How many species the registry holds, as a `u8` for the domain's cycle.
///
/// Read from the registry rather than written down, so adding a creature next door does not
/// leave the menu cycling a shorter list than exists. A registry longer than 255 would be a
/// different problem; saturating keeps the cycle total rather than panicking mid-press.
fn species_count() -> u8 {
    u8::try_from(buddy_creature::REGISTRY.len()).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use buddy_core::is_face_down;
    use buddy_wire::{BuddyEvent, SnapshotPacket};

    const A_CLICK: ButtonEvent = ButtonEvent::new(ButtonId::Front, Gesture::Click);
    const B_CLICK: ButtonEvent = ButtonEvent::new(ButtonId::Side, Gesture::Click);
    const A_HOLD: ButtonEvent = ButtonEvent::new(ButtonId::Front, Gesture::LongHold);
    const REST: Acceleration = Acceleration::new(0, 0, 1_000);

    fn state() -> DeviceState {
        DeviceState::new(
            Identity::new("Claude-4F2A", "0.1.0", "A0:B7:65:4F:2A:11"),
            SpeciesIndex::new(0),
        )
    }

    /// A snapshot carrying a prompt.
    fn with_prompt(id: &str) -> Inbound {
        Inbound::Snapshot(SnapshotPacket {
            running: Some(1),
            prompt: Some(Prompt {
                id: id.to_string(),
                tool: "Bash".to_string(),
                hint: "cargo test --workspace".to_string(),
            }),
            ..SnapshotPacket::default()
        })
    }

    /// Zero: a fresh stick is unlinked, at home, with nothing pending.
    #[test]
    fn a_fresh_stick_is_unlinked_and_quiet() {
        let state: DeviceState = state();
        assert!(!state.is_linked());
        assert_eq!(state.view().screen, Screen::Home);
        assert!(state.view().prompt.is_none());
    }

    /// One: a snapshot with a prompt puts it on the glass.
    #[test]
    fn a_prompt_reaches_the_glass() {
        let mut state: DeviceState = state();
        state.apply(with_prompt("p1"), 1_000);
        state.tick(1_000, REST, true);
        let view: BuddyView = state.view();
        assert_eq!(
            view.prompt.expect("a prompt is pending").tool.as_str(),
            "Bash"
        );
    }

    /// THE HEADLINE: A answers once, B answers deny, and each names the prompt it answered.
    #[test]
    fn a_allows_and_b_denies_the_pending_prompt() {
        let mut allow: DeviceState = state();
        allow.apply(with_prompt("p1"), 1_000);
        assert_eq!(
            allow.press(A_CLICK, 2_000),
            Effect::Answer(PermissionResponse {
                id: "p1".to_string(),
                decision: Decision::Once,
            })
        );

        let mut deny: DeviceState = state();
        deny.apply(with_prompt("p1"), 1_000);
        assert_eq!(
            deny.press(B_CLICK, 2_000),
            Effect::Answer(PermissionResponse {
                id: "p1".to_string(),
                decision: Decision::Deny,
            })
        );
    }

    /// A prompt is answered once. A second press does not send a second response — a duplicate
    /// answer to a hook that has already acted on the first is noise at best.
    #[test]
    fn a_prompt_is_answered_only_once() {
        let mut state: DeviceState = state();
        state.apply(with_prompt("p1"), 1_000);
        assert!(matches!(state.press(A_CLICK, 2_000), Effect::Answer(_)));
        assert_eq!(state.press(A_CLICK, 2_100), Effect::None);
        assert!(state.view().prompt.is_none(), "the band did not come back");
    }

    /// A NEW prompt is answerable again — the answered latch is per prompt, not for ever.
    #[test]
    fn a_new_prompt_is_answerable_again() {
        let mut state: DeviceState = state();
        state.apply(with_prompt("p1"), 1_000);
        state.press(A_CLICK, 2_000);
        state.apply(with_prompt("p2"), 3_000);
        assert!(matches!(state.press(A_CLICK, 4_000), Effect::Answer(_)));
    }

    /// THE FAIL-SAFE, DEVICE SIDE: keepalive traffic — a transcript event, a time sync, a
    /// command — leaves a pending prompt exactly where it was. Only a real snapshot may clear
    /// it, and this is the arm that has to stay empty for that to hold.
    #[test]
    fn keepalive_traffic_does_not_disturb_a_pending_prompt() {
        let mut state: DeviceState = state();
        state.apply(with_prompt("p1"), 1_000);
        state.apply(Inbound::Event(BuddyEvent::Turn), 2_000);
        state.apply(
            Inbound::Time {
                epoch: 1_700_000_000,
                tz_offset_s: 0,
            },
            3_000,
        );
        state.apply(Inbound::Command(Command::Status), 4_000);
        state.tick(5_000, REST, true);
        assert_eq!(
            state
                .view()
                .prompt
                .expect("the prompt survived")
                .tool
                .as_str(),
            "Bash"
        );
    }

    /// A real snapshot with no prompt field DOES clear it — the other half of the same rule,
    /// without which the fail-safe would just be a prompt that never goes away.
    #[test]
    fn a_snapshot_without_a_prompt_clears_it() {
        let mut state: DeviceState = state();
        state.apply(with_prompt("p1"), 1_000);
        state.apply(Inbound::Snapshot(SnapshotPacket::default()), 2_000);
        state.tick(2_000, REST, true);
        assert!(state.view().prompt.is_none());
    }

    /// The counter counts up from when the prompt arrived, not from boot.
    #[test]
    fn the_counter_measures_from_the_prompts_arrival() {
        let mut state: DeviceState = state();
        state.apply(with_prompt("p1"), 10_000);
        state.tick(14_000, REST, true);
        assert_eq!(state.view().prompt.expect("pending").waiting_s, 4);
    }

    /// Many: the link goes live on a line and lapses after the window, which is the only thing
    /// that tells a calm desk from a dead daemon.
    #[test]
    fn the_link_lapses_after_the_window() {
        let mut state: DeviceState = state();
        state.apply(Inbound::Event(BuddyEvent::Turn), 1_000);
        state.tick(1_000, REST, true);
        assert!(state.is_linked());
        state.tick(1_000 + LINK_WINDOW_MS, REST, true);
        assert!(!state.is_linked());
    }

    /// The prompt outranks the menu: a press made with the menu open still answers.
    #[test]
    fn a_prompt_outranks_an_open_menu() {
        let mut state: DeviceState = state();
        state.press(A_HOLD, 500);
        assert!(matches!(state.view().overlay, Overlay::Menu { .. }));
        state.apply(with_prompt("p1"), 1_000);
        assert!(matches!(state.press(A_CLICK, 2_000), Effect::Answer(_)));
    }

    /// The reset confirmation only forgets the bond on an explicit A — B, and anything else,
    /// cancels. The two mistakes do not cost the same.
    #[test]
    fn the_reset_confirmation_defaults_to_cancelling() {
        let mut confirm: DeviceState = state();
        open_reset(&mut confirm);
        assert_eq!(confirm.press(A_CLICK, 4_000), Effect::ForgetBond);

        let mut cancel: DeviceState = state();
        open_reset(&mut cancel);
        assert_eq!(cancel.press(B_CLICK, 4_000), Effect::None);
        assert!(!matches!(cancel.view().overlay, Overlay::Reset));
    }

    /// Walk the menu to Unpair and select it.
    fn open_reset(state: &mut DeviceState) {
        state.press(A_HOLD, 1_000);
        // MENU_ENTRIES: Species, Owner, Status, Unpair, Close — three clicks to reach Unpair.
        state.press(A_CLICK, 1_100);
        state.press(A_CLICK, 1_200);
        state.press(A_CLICK, 1_300);
        state.press(B_CLICK, 1_400);
        assert!(matches!(state.view().overlay, Overlay::Reset));
    }

    /// Selecting the creature entry cycles the species and asks for it to be persisted.
    #[test]
    fn selecting_the_creature_entry_cycles_and_persists() {
        let mut state: DeviceState = state();
        state.press(A_HOLD, 1_000);
        assert!(matches!(
            state.press(B_CLICK, 1_100),
            Effect::PersistSpecies(_)
        ));
    }

    /// A passkey takes over the glass whatever else is showing.
    #[test]
    fn a_passkey_takes_over_the_glass() {
        let mut state: DeviceState = state();
        state.apply(with_prompt("p1"), 1_000);
        state.show_passkey(Some(482_913));
        state.tick(1_000, REST, true);
        assert_eq!(state.view().passkey, Some(482_913));
    }

    /// Docking shows the clock and undocking gives the screen back.
    #[test]
    fn docking_shows_the_clock_and_undocking_gives_it_back() {
        let mut state: DeviceState = state();
        state.power(true, None);
        assert_eq!(state.view().screen, Screen::Clock);
        state.power(false, None);
        assert_eq!(state.view().screen, Screen::Home);
    }

    /// Undocking does NOT yank the owner off a page they walked to themselves.
    #[test]
    fn undocking_does_not_move_a_screen_the_owner_chose() {
        let mut state: DeviceState = state();
        state.power(true, None);
        state.press(B_CLICK, 1_000);
        let chosen: Screen = state.view().screen;
        state.power(false, None);
        assert_eq!(state.view().screen, chosen);
    }

    /// The transcript reaches the glass, newest first.
    #[test]
    fn the_transcript_reaches_the_glass_newest_first() {
        let mut state: DeviceState = state();
        state.apply(
            Inbound::Snapshot(SnapshotPacket {
                entries: Some(vec!["older".to_string(), "newer".to_string()]),
                ..SnapshotPacket::default()
            }),
            1_000,
        );
        state.tick(1_000, REST, true);
        let view: BuddyView = state.view();
        assert_eq!(view.transcript.newest_first()[0].as_str(), "newer");
    }

    /// A nap is counted when the buddy is left face-down long enough and then picked up.
    #[test]
    fn a_nap_is_counted_from_face_down_to_picked_up() {
        let face_down: Acceleration = Acceleration::new(0, 0, -1_000);
        assert!(is_face_down(face_down));
        let mut state: DeviceState = state();
        // The hysteresis counter needs several frames at each end.
        (0..40).for_each(|frame: u64| state.tick(1_000 + frame * 100, face_down, true));
        (0..40).for_each(|frame: u64| state.tick(9_000 + frame * 100, REST, true));
        assert_eq!(state.view().stats.naps, 1);
    }
}
