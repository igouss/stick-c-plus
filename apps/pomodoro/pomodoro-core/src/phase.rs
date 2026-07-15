//! The three kinds of pomodoro interval.

use crate::config::Durations;

/// A pomodoro phase: focused work, or one of two rests.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// A focus pomodoro — the work interval.
    Focus,
    /// The short rest after a focus pomodoro.
    ShortBreak,
    /// The longer rest earned after several focus pomodoros.
    LongBreak,
}

impl Phase {
    /// This phase's full length, in milliseconds, under `durations`.
    pub const fn duration_ms(self, durations: Durations) -> u64 {
        match self {
            Phase::Focus => durations.focus_ms,
            Phase::ShortBreak => durations.short_break_ms,
            Phase::LongBreak => durations.long_break_ms,
        }
    }

    /// Whether this phase is a rest (a break), as opposed to focused work.
    pub const fn is_break(self) -> bool {
        matches!(self, Phase::ShortBreak | Phase::LongBreak)
    }
}
