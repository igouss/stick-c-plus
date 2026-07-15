//! The tunable lengths of a pomodoro cycle — one constant, easy to change.

/// The lengths of the three phases and how often a long break is earned.
///
/// A value object with no invariant beyond being [`Copy`], so the composition root can hold
/// one and pass it into every [`step`](crate::step) and [`remaining`](crate::Timer::remaining)
/// call. Durations are milliseconds so they share the [`Tick`](platform_core::Tick) unit the
/// countdown measures in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Durations {
    /// A focus pomodoro's length, in milliseconds.
    pub focus_ms: u64,
    /// A short break's length, in milliseconds.
    pub short_break_ms: u64,
    /// A long break's length, in milliseconds.
    pub long_break_ms: u64,
    /// A long break replaces the short one after this many *completed* focus pomodoros. Zero
    /// means "never a long break".
    pub long_break_every: u32,
}

/// The classic pomodoro: 25 min focus, 5 min short break, 15 min long break every 4th focus.
///
/// Change this one constant (or build a [`Durations`] in the composition root) to retune the
/// whole cycle — the FSM reads its lengths from here and nowhere else.
pub const CLASSIC: Durations = Durations {
    focus_ms: 25 * 60_000,
    short_break_ms: 5 * 60_000,
    long_break_ms: 15 * 60_000,
    long_break_every: 4,
};
