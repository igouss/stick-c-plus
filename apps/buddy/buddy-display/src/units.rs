//! The two readings that will not fit as digits: a token count, and a span of nap time.
//!
//! [`Display`](core::fmt::Display) adapters rather than formatting helpers returning strings,
//! because the render path builds its lines with `format_args!` straight into a stack buffer —
//! a helper that returned a `String` would put an allocation on the one path that must not have
//! one.

use core::fmt;

/// A count as a person reads it at a glance: `847`, `128k`, `3M`.
///
/// Tokens run to millions and the field is a handful of characters. The exact number is not the
/// question a desk pet answers — "roughly how much have I fed it today" is — so the reading is
/// truncated toward zero rather than rounded: it never claims a milestone that has not been
/// reached.
pub struct Compact(pub u32);

/// The largest millions reading the field can hold. A `u32` of tokens runs to 4294 million,
/// which is five characters — one more than a thirteen-column row has to spare once it has a
/// label on it, and four thousand million tokens is not a number anyone reads off a desk pet.
const MILLION_CAP: u32 = 999;

impl fmt::Display for Compact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            count if count < 1_000 => write!(f, "{count}"),
            count if count < 1_000_000 => write!(f, "{}k", count / 1_000),
            count => write!(f, "{}M", (count / 1_000_000).min(MILLION_CAP)),
        }
    }
}

/// A span of whole minutes as `14m`, `2h14m`, or `999h+` once it stops being a reading.
///
/// The hours are **not** rolled over at a day: a buddy left face-down over a weekend has
/// genuinely napped for sixty hours, and showing that as `12h` would be a lie about the one stat
/// the pet screen exists to show. They are *capped*, though, because a thirteen-column canvas
/// cannot hold a five-digit hour count — and past a month the number has stopped being a nap and
/// become a pet on a shelf.
pub struct Span(pub u32);

/// The largest hour count the field can hold; past it the reading says so with a `+`.
const HOUR_CAP: u32 = 999;

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.0 / 60, self.0 % 60) {
            (hours, _) if hours > HOUR_CAP => write!(f, "{HOUR_CAP}h+"),
            (0, minutes) => write!(f, "{minutes}m"),
            (hours, minutes) => write!(f, "{hours}h{minutes:02}m"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// Zero, one, many — across both of the thresholds a count crosses.
    #[test]
    fn a_count_reads_plainly_then_in_thousands_then_in_millions() {
        assert_eq!(format!("{}", Compact(0)), "0");
        assert_eq!(format!("{}", Compact(847)), "847");
        assert_eq!(format!("{}", Compact(999)), "999");
        assert_eq!(format!("{}", Compact(1_000)), "1k");
        assert_eq!(format!("{}", Compact(128_400)), "128k");
        assert_eq!(format!("{}", Compact(1_000_000)), "1M");
        assert_eq!(format!("{}", Compact(u32::MAX)), "999M");
    }

    /// The truncation is toward zero: 999 999 tokens is not yet a million.
    #[test]
    fn a_count_never_claims_a_milestone_it_has_not_reached() {
        assert_eq!(format!("{}", Compact(999_999)), "999k");
    }

    /// A span reads in minutes below the hour and in hours above it, with the minutes padded so
    /// two spans line up in a fixed-width field.
    #[test]
    fn a_span_reads_in_minutes_then_in_hours() {
        assert_eq!(format!("{}", Span(0)), "0m");
        assert_eq!(format!("{}", Span(14)), "14m");
        assert_eq!(format!("{}", Span(59)), "59m");
        assert_eq!(format!("{}", Span(60)), "1h00m");
        assert_eq!(format!("{}", Span(134)), "2h14m");
    }

    /// A long nap really is long: the hours do not roll over at a day.
    #[test]
    fn a_span_of_days_keeps_counting_hours() {
        assert_eq!(format!("{}", Span(60 * 60)), "60h00m");
    }

    /// Past the field's width the reading says it has run out of room, rather than running off
    /// the edge of the glass.
    #[test]
    fn a_span_past_the_field_says_so() {
        assert_eq!(format!("{}", Span(HOUR_CAP * 60)), "999h00m");
        assert_eq!(format!("{}", Span((HOUR_CAP + 1) * 60)), "999h+");
        assert_eq!(format!("{}", Span(u32::MAX)), "999h+");
    }
}
