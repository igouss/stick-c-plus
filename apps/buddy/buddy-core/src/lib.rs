#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! # buddy-core
//!
//! Framework-free desk-pet domain for the M5StickC Plus — the portable heart of the buddy.
//!
//! Pure `no_std`: the persona state machine, the two self-sensors, the stats math, the
//! charging-clock schedule, the menu tree, and the selected species index — all deterministic
//! and host-testable, with the Xtensa side supplying only a clock, the IMU, the buttons, and
//! the merged wire snapshot. Time is a parameter ([`platform_core::Tick`]), never a clock read.
//!
//! ## Hexagon
//! - **Entities**: [`PersonaState`] and [`Snapshot`] — the moods and the input they derive
//!   from; [`OneShot`] — the timed override layer; [`ShakeDetector`], [`NapCounter`],
//!   [`VelocityRing`], [`TokenLatch`] — the stateful sensor and stats aggregates;
//!   [`SpeciesIndex`] — the chosen creature; [`Menu`] — the settings tree.
//! - **Control**: [`step`] — the one use case, folding a loop's inputs into the next
//!   [`Buddy`] and its outcome, in the ordering that matters.
//! - The domain names no framework and no hardware; it owns *which persona is active* and
//!   *which species is selected*, never a sprite or a pin.
//!
//! ## The one loop order
//!
//! [`step`] resolves the persona in exactly this order, because upstream does and the order
//! is observable:
//!
//! 1. **level-up** — arm the celebrate one-shot;
//! 2. **derive** — the base persona from the snapshot;
//! 3. **wake-window rewrite** — base `Idle → Sleep` while the wake window is armed;
//! 4. **one-shot resolution** — a live override outranks the base;
//! 5. **shake** — sample the IMU (only when awake and out of the menu), arming dizzy;
//! 6. **buttons / heart** — dispatch the button event; a fast approval arms heart.
//!
//! Two writers reach [`Sleep`](PersonaState::Sleep): the wake window rewrites the *base*
//! (step 3, Idle only), and the charging clock writes the *active* state **directly** (a final
//! override that bypasses the base and the one-shot), modelled as
//! [`StepInput::charging_mood`].

mod clock;
mod menu;
mod oneshot;
mod persona;
mod sensors;
mod species;
mod stats;

pub use clock::charging_mood;
pub use menu::{Menu, MenuEntry, MenuOutcome, MenuState, MENU_ENTRIES};
pub use oneshot::{OneShot, APPROVAL_HEART_WINDOW_S, CELEBRATE_MS, DIZZY_MS, HEART_MS};
pub use persona::{derive, PersonaState, Snapshot, WAKE_WINDOW_MS};
pub use sensors::{
    is_face_down, NapCounter, NapTransition, ShakeDetector, NAP_COUNTER_MAX, NAP_COUNTER_MIN,
    NAP_ENTER, NAP_LEAVE, SHAKE_BASELINE_SEED_G, SHAKE_THRESHOLD_G,
};
pub use species::{SpeciesIndex, GIF_SENTINEL, SPECIES_NVS_KEY};
pub use stats::{
    energy_tier, fed_progress, hours_since, level, mood_tier, TokenLatch, VelocityRing,
    ENERGY_AT_NAP_FULL, TOKENS_PER_LEVEL,
};

use platform_core::{Acceleration, Tick};
use platform_input::ButtonEvent;

/// An approval decision the owner just made, for the fast-approval heart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Approval {
    /// Whether it was an approve (`true`) or a deny (`false`); a deny fires no heart.
    pub approved: bool,
    /// How long the owner took to answer, in whole seconds.
    pub took_s: u32,
}

/// One loop iteration's inputs to [`step`] — everything the buddy reads that frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StepInput {
    /// The merged session snapshot the base persona derives from.
    pub snapshot: Snapshot,
    /// The monotonic time for this iteration.
    pub now: Tick,
    /// The current acceleration, for the shake and nap sensors.
    pub accel: Acceleration,
    /// Whether the glass is currently lit (a false→true edge arms the wake window).
    pub screen_on: bool,
    /// Whether the settings menu is open (gates the shake sampling).
    pub menu_open: bool,
    /// Whether a permission prompt is up **and unanswered** (freezes the nap counter).
    pub prompt_unanswered: bool,
    /// A level-up milestone this frame — arms the celebrate one-shot.
    pub level_up: bool,
    /// An approval the owner just answered — may arm the heart one-shot.
    pub approval: Option<Approval>,
    /// A button event this frame — dispatched to the menu.
    pub button: Option<ButtonEvent>,
    /// The charging clock's direct persona override, when docked and idle (see
    /// [`charging_mood`]). `Some` wins over the derived/one-shot persona for this frame,
    /// bypassing the base and the one-shot resolution — the second, direct writer into sleep.
    pub charging_mood: Option<PersonaState>,
}

/// What one [`step`] produced besides the next buddy: the persona to render, and any menu or
/// nap transition the shell must act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// The active persona to render this frame.
    pub persona: PersonaState,
    /// What the button event did to the menu.
    pub menu: MenuOutcome,
    /// Any nap transition (drives stats-on-nap-end / wake in the shell).
    pub nap: NapTransition,
}

/// The whole desk-pet state: the persona layers, the sensors, the stats, the menu, the
/// selected species. [`Copy`], so [`step`] takes and returns it by value.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Buddy {
    /// The timed override layer.
    pub one_shot: OneShot,
    /// The face-down nap hysteresis.
    pub nap: NapCounter,
    /// The shake EMA detector.
    pub shake: ShakeDetector,
    /// The velocity history feeding the mood tier.
    pub velocity: VelocityRing,
    /// The RAM-only token-delta latch.
    pub tokens: TokenLatch,
    /// The settings menu.
    pub menu: Menu,
    /// The selected creature.
    pub species: SpeciesIndex,
    /// The wake window deadline; base `Idle` is rewritten to `Sleep` while `now < wake_until`.
    wake_until: Tick,
    /// The previous frame's `screen_on`, to detect the off→on edge that arms the wake window.
    prev_screen_on: bool,
}

impl Buddy {
    /// A fresh buddy on the given species: awake, idle, nothing armed.
    pub const fn new(species: SpeciesIndex) -> Self {
        Buddy {
            one_shot: OneShot::idle(),
            nap: NapCounter::new(),
            shake: ShakeDetector::new(),
            velocity: VelocityRing::new(),
            tokens: TokenLatch::new(),
            menu: Menu::new(),
            species,
            wake_until: 0,
            prev_screen_on: false,
        }
    }
}

/// Fold one loop iteration into the next buddy and its outcome — the whole use case.
///
/// The resolution order is the contract; see the crate-level "one loop order". `now` comes
/// from the injected clock, never read here.
pub fn step(buddy: Buddy, input: &StepInput) -> (Buddy, Outcome) {
    let mut buddy: Buddy = buddy;

    // 1. level-up — arm the celebrate one-shot (it preempts any live override).
    if input.level_up {
        buddy.one_shot.level_up(input.now);
    }

    // 2. derive — the base persona from the snapshot.
    let mut base: PersonaState = derive(&input.snapshot);

    // 3. wake-window rewrite — arm on a screen off→on edge, then rewrite base Idle→Sleep while
    //    the window is open. Only Idle is rewritten; attention/celebrate/busy pass through.
    if input.screen_on && !buddy.prev_screen_on {
        buddy.wake_until = input.now + WAKE_WINDOW_MS;
    }
    buddy.prev_screen_on = input.screen_on;
    if base == PersonaState::Idle && input.now < buddy.wake_until {
        base = PersonaState::Sleep;
    }

    // 4. one-shot resolution happens through `active` at the end; the shake below self-guards
    //    against preempting a live one-shot.

    // 5. shake — sample the IMU only while awake and out of the menu (the `&&` short-circuit is
    //    the load-bearing stale-baseline quirk: the EMA does not advance otherwise).
    if input.screen_on && !input.menu_open && buddy.shake.sample(input.accel) {
        buddy.one_shot.shake(input.now);
    }

    // 6. buttons / heart — dispatch the button event, then let a fast approval arm the heart.
    let menu: MenuOutcome = match input.button {
        Some(event) => buddy.menu.dispatch(event),
        None => MenuOutcome::None,
    };
    if let Some(approval) = input.approval {
        buddy
            .one_shot
            .approval_answered(approval.approved, approval.took_s, input.now);
    }

    // The nap counter always advances (frozen internally while a prompt is unanswered).
    let nap: NapTransition = buddy.nap.update(input.accel, input.prompt_unanswered);

    // Resolve the persona: the one-shot override while live, else the base — unless the charging
    // clock is the final, direct writer this frame, bypassing base and one-shot entirely.
    let persona: PersonaState = match input.charging_mood {
        Some(mood) => mood,
        None => buddy.one_shot.active(input.now, base),
    };

    (buddy, Outcome { persona, menu, nap })
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::Acceleration;
    use platform_input::{ButtonId, Gesture};

    /// A resting acceleration under gravity — no shake, not face-down.
    const REST: Acceleration = Acceleration::new(0, 0, 1_000);

    /// A base input template: connected, quiet, screen on, nothing armed.
    fn base_input(now: Tick) -> StepInput {
        StepInput {
            snapshot: Snapshot {
                connected: true,
                sessions_waiting: 0,
                sessions_running: 0,
                recently_completed: false,
            },
            now,
            accel: REST,
            screen_on: true,
            menu_open: false,
            prompt_unanswered: false,
            level_up: false,
            approval: None,
            button: None,
            charging_mood: None,
        }
    }

    /// A quiet connected buddy that has been awake past the wake window renders idle.
    #[test]
    fn a_quiet_buddy_is_idle() {
        let buddy: Buddy = Buddy::new(SpeciesIndex::new(0));
        // First step arms the wake window (screen off→on edge from the fresh `false`).
        let (buddy, _): (Buddy, Outcome) = step(buddy, &base_input(0));
        // Past the window, idle is idle.
        let (_, outcome): (Buddy, Outcome) = step(buddy, &base_input(WAKE_WINDOW_MS + 1));
        assert_eq!(outcome.persona, PersonaState::Idle);
    }

    /// The wake window rewrites a base Idle to Sleep on the screen-on edge.
    #[test]
    fn the_wake_window_rewrites_idle_to_sleep() {
        let buddy: Buddy = Buddy::new(SpeciesIndex::new(0));
        let (_, outcome): (Buddy, Outcome) = step(buddy, &base_input(0));
        assert_eq!(outcome.persona, PersonaState::Sleep);
    }

    /// The wake window does NOT rewrite attention — only idle passes into sleep.
    #[test]
    fn the_wake_window_lets_attention_pass() {
        let buddy: Buddy = Buddy::new(SpeciesIndex::new(0));
        let mut input: StepInput = base_input(0);
        input.snapshot.sessions_waiting = 1;
        let (_, outcome): (Buddy, Outcome) = step(buddy, &input);
        assert_eq!(outcome.persona, PersonaState::Attention);
    }

    /// A level-up celebrates, overriding the derived base.
    #[test]
    fn a_level_up_celebrates_over_the_base() {
        let buddy: Buddy = Buddy::new(SpeciesIndex::new(0));
        // Get past the wake window first so the base would otherwise be idle.
        let (buddy, _): (Buddy, Outcome) = step(buddy, &base_input(0));
        let mut input: StepInput = base_input(WAKE_WINDOW_MS + 1);
        input.level_up = true;
        let (_, outcome): (Buddy, Outcome) = step(buddy, &input);
        assert_eq!(outcome.persona, PersonaState::Celebrate);
    }

    /// The charging clock is the final direct writer, suppressing the derived persona.
    #[test]
    fn the_charging_clock_writes_directly() {
        let buddy: Buddy = Buddy::new(SpeciesIndex::new(0));
        let mut input: StepInput = base_input(0);
        input.snapshot.sessions_waiting = 1; // base would be attention
        input.charging_mood = Some(PersonaState::Sleep);
        let (_, outcome): (Buddy, Outcome) = step(buddy, &input);
        assert_eq!(outcome.persona, PersonaState::Sleep);
    }

    /// PRESERVED QUIRK: with the menu open the shake detector is never sampled, so its EMA
    /// baseline goes stale. This test pins that the first sample after closing the menu differs
    /// from what it would have been had the menu-open frames advanced the baseline.
    #[test]
    fn the_shake_baseline_goes_stale_while_the_menu_is_open() {
        // A steady high-magnitude acceleration for several frames.
        let high: Acceleration = Acceleration::new(0, 0, 1_600);

        // Buddy A holds the menu open across two frames: the baseline stays at the seed.
        let mut a_frame_0: StepInput = base_input(0);
        a_frame_0.menu_open = true;
        a_frame_0.accel = high;
        let mut a_frame_1: StepInput = a_frame_0;
        a_frame_1.now = 50;
        let (a1, _): (Buddy, Outcome) = step(Buddy::new(SpeciesIndex::new(0)), &a_frame_0);
        let (with_menu, _): (Buddy, Outcome) = step(a1, &a_frame_1);

        // Buddy B has the menu closed across the same frames: its baseline tracks toward 1.6 g.
        let mut b_frame_0: StepInput = base_input(0);
        b_frame_0.accel = high;
        let mut b_frame_1: StepInput = b_frame_0;
        b_frame_1.now = 50;
        let (b1, _): (Buddy, Outcome) = step(Buddy::new(SpeciesIndex::new(0)), &b_frame_0);
        let (without_menu, _): (Buddy, Outcome) = step(b1, &b_frame_1);

        // The two shake detectors now hold different baselines.
        assert_ne!(with_menu.shake, without_menu.shake);
    }

    /// PRESERVED QUIRK (the screen-off half): the shake detector is not sampled while the screen
    /// is off either, so the baseline stays stale. Same shape as the menu-open pin, on the other
    /// arm of the `screen_on && !menu_open` guard.
    #[test]
    fn the_shake_baseline_goes_stale_while_the_screen_is_off() {
        let high: Acceleration = Acceleration::new(0, 0, 1_600);

        // Buddy A has the screen off across two frames: the baseline stays at the seed.
        let mut a_frame_0: StepInput = base_input(0);
        a_frame_0.screen_on = false;
        a_frame_0.accel = high;
        let mut a_frame_1: StepInput = a_frame_0;
        a_frame_1.now = 50;
        let (a1, _): (Buddy, Outcome) = step(Buddy::new(SpeciesIndex::new(0)), &a_frame_0);
        let (screen_off, _): (Buddy, Outcome) = step(a1, &a_frame_1);

        // Buddy B has the screen on across the same frames: its baseline tracks toward 1.6 g.
        let mut b_frame_0: StepInput = base_input(0);
        b_frame_0.accel = high;
        let mut b_frame_1: StepInput = b_frame_0;
        b_frame_1.now = 50;
        let (b1, _): (Buddy, Outcome) = step(Buddy::new(SpeciesIndex::new(0)), &b_frame_0);
        let (screen_on, _): (Buddy, Outcome) = step(b1, &b_frame_1);

        assert_ne!(screen_off.shake, screen_on.shake);
    }

    /// A front long-hold opens the menu through `step`, surfacing the outcome.
    #[test]
    fn a_button_event_drives_the_menu_through_step() {
        let buddy: Buddy = Buddy::new(SpeciesIndex::new(0));
        let mut input: StepInput = base_input(0);
        input.button = Some(ButtonEvent::new(ButtonId::Front, Gesture::LongHold));
        let (buddy, outcome): (Buddy, Outcome) = step(buddy, &input);
        assert_eq!(outcome.menu, MenuOutcome::Opened);
        assert!(buddy.menu.is_open());
    }
}
