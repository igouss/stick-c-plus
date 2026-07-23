//! The [`Species`] abstraction: a creature as DATA, not code.
//!
//! A species is a name plus a table of sprite SETS indexed by the [`PersonaState`] ordinal.
//! `states[state as usize]` is that state's set — a small, **non-empty** slice: one sprite for
//! a state with no variety (sleep, attention, dizzy, heart), several where the state offers
//! variety (idle has three, busy and celebrate two). A creature is a single `const` value;
//! adding a creature is adding a `const`, never a code path.
//!
//! The ordinal is the load-bearing contract (`Sleep = 0` .. `Heart = 6`, see
//! [`PersonaState`]); the table is laid out in that exact order and indexed by it.

use buddy_core::PersonaState;
use platform_display::sprite::Sprite;

/// The number of persona states — the width of every species' sprite table.
///
/// `Sleep = 0` through `Heart = 6`; the table has exactly one set per state.
pub const STATE_COUNT: usize = 7;

/// A creature as data: a name, and a set of sprites per persona state.
///
/// Constructed as a `const`/`static` value per creature (see [`crate::CLAUDEPIX`]). The
/// per-state sets are `&'static` slices into the vendored sprite library — the species holds
/// no art of its own, only the binding.
#[derive(Clone, Copy, Debug)]
pub struct Species {
    name: &'static str,
    states: [&'static [&'static Sprite]; STATE_COUNT],
}

impl Species {
    /// Bind a named creature to its per-state sprite sets, in persona-ordinal order
    /// (`[Sleep, Idle, Busy, Attention, Celebrate, Dizzy, Heart]`).
    ///
    /// Every set MUST be non-empty; an empty set would make [`current`](crate::current) yield
    /// no sprite for that state. The invariant is a data fact of each `const`, pinned by a
    /// property test rather than enforced in the type.
    pub const fn new(
        name: &'static str,
        states: [&'static [&'static Sprite]; STATE_COUNT],
    ) -> Self {
        Species { name, states }
    }

    /// The creature's stable name — lets the registry order be checked by identity.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The sprite set for a persona state: `states[state as usize]`, the load-bearing ordinal.
    ///
    /// Non-empty for all seven states (a data invariant of every well-formed [`Species`]).
    pub const fn sprites(&self, state: PersonaState) -> &'static [&'static Sprite] {
        self.states[state as usize]
    }
}
