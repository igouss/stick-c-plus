//! The wall clock: a time sync from the host, held against the monotonic tick.
//!
//! The board has no RTC worth trusting across a reset, so the only wall time it has is what the
//! bridge sends — `{"time":[epoch, tz_offset_seconds]}`. This holds that reading *and the tick
//! it arrived at*, so the clock keeps running between syncs off the monotonic clock rather than
//! freezing at whatever second the last packet named.
//!
//! Unsynced is a real state, not a zero: a stick that has never heard from a host shows no time
//! at all rather than midnight on the first of January, which would look like a working clock.

use platform_core::Tick;

/// Seconds in a day, for the wrap.
const DAY_S: i64 = 24 * 60 * 60;

/// The host's wall time, anchored to the tick it arrived at.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct WallClock {
    /// The local (offset-applied) epoch seconds of the last sync, and the tick it landed on.
    synced: Option<(i64, Tick)>,
}

impl WallClock {
    /// An unsynced clock — no host has said what time it is.
    pub const fn new() -> Self {
        WallClock { synced: None }
    }

    /// Take a `{"time":[epoch, tz_offset_seconds]}` sync at `now`.
    ///
    /// The offset is folded into the epoch here, once, so everything downstream is local time
    /// and no later reader has to remember to apply it.
    pub fn sync(&mut self, epoch: i64, tz_offset_s: i32, now: Tick) {
        self.synced = Some((epoch + i64::from(tz_offset_s), now));
    }

    /// Whether a host has ever said what time it is.
    pub const fn is_synced(&self) -> bool {
        self.synced.is_some()
    }

    /// The local hour and minute at `now`, or `None` while unsynced.
    ///
    /// The elapsed monotonic milliseconds since the sync are added to it, so the clock advances
    /// between syncs. `now` before the sync tick cannot happen on a monotonic clock, and is
    /// saturated rather than wrapped if it somehow does.
    pub fn hour_minute(&self, now: Tick) -> Option<(u8, u8)> {
        let local: i64 = self.local_seconds(now)?.rem_euclid(DAY_S);
        Some(((local / 3_600) as u8, ((local % 3_600) / 60) as u8))
    }

    /// The local hour and day of week at `now` — `0` is Sunday — or `None` while unsynced.
    ///
    /// The charging clock's mood schedule is a function of both (a Friday afternoon is not a
    /// Sunday afternoon), so the two are answered together: reading them from two calls would
    /// let a midnight rollover land between them and pair an hour with the wrong day.
    pub fn hour_and_day(&self, now: Tick) -> Option<(u8, u8)> {
        let local: i64 = self.local_seconds(now)?;
        let hour: u8 = (local.rem_euclid(DAY_S) / 3_600) as u8;
        // 1 January 1970 was a Thursday, and Sunday is 0 — hence the +4.
        let day: u8 = ((local.div_euclid(DAY_S) + 4).rem_euclid(7)) as u8;
        Some((hour, day))
    }

    /// Local epoch seconds at `now`, or `None` while unsynced.
    fn local_seconds(&self, now: Tick) -> Option<i64> {
        let (epoch, at): (i64, Tick) = self.synced?;
        Some(epoch + (now.saturating_sub(at) / 1_000) as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 14:37:00 UTC on an arbitrary day, as epoch seconds.
    const AT_1437_UTC: i64 = 1_700_000_000 - 1_700_000_000 % DAY_S + 14 * 3_600 + 37 * 60;

    /// Zero: an unsynced clock has no time to show — not midnight, which would look like a
    /// working clock that had lost a day.
    #[test]
    fn an_unsynced_clock_shows_no_time() {
        let clock: WallClock = WallClock::new();
        assert!(!clock.is_synced());
        assert_eq!(clock.hour_minute(10_000), None);
    }

    /// One: a sync at UTC reads back the hour and minute it named.
    #[test]
    fn a_sync_reads_back_the_time_it_named() {
        let mut clock: WallClock = WallClock::new();
        clock.sync(AT_1437_UTC, 0, 5_000);
        assert_eq!(clock.hour_minute(5_000), Some((14, 37)));
    }

    /// The offset is folded in once: the same epoch an hour east reads an hour later.
    #[test]
    fn the_timezone_offset_is_applied() {
        let mut clock: WallClock = WallClock::new();
        clock.sync(AT_1437_UTC, 3_600, 5_000);
        assert_eq!(clock.hour_minute(5_000), Some((15, 37)));
    }

    /// Many: the clock keeps running between syncs, off the monotonic tick — it does not freeze
    /// at the second the last packet named.
    #[test]
    fn the_clock_runs_between_syncs() {
        let mut clock: WallClock = WallClock::new();
        clock.sync(AT_1437_UTC, 0, 5_000);
        assert_eq!(clock.hour_minute(5_000 + 90 * 60 * 1_000), Some((16, 7)));
    }

    /// It wraps at midnight rather than counting to a twenty-fifth hour.
    #[test]
    fn the_clock_wraps_at_midnight() {
        let mut clock: WallClock = WallClock::new();
        clock.sync(AT_1437_UTC, 0, 0);
        let ten_hours_later: Tick = 10 * 60 * 60 * 1_000;
        assert_eq!(clock.hour_minute(ten_hours_later), Some((0, 37)));
    }

    /// A negative offset that crosses back over midnight still lands on a real time of day.
    #[test]
    fn a_negative_offset_across_midnight_stays_a_real_time() {
        let mut clock: WallClock = WallClock::new();
        clock.sync(AT_1437_UTC - 14 * 3_600, -3_600, 0);
        assert_eq!(clock.hour_minute(0), Some((23, 37)));
    }
}
