//! HostSeries — one host's name and its CPU + memory `%` series.
//!
//! The unit the pulse frame is a list of: a named host with two [`Series`], one for CPU
//! busy-percent and one for memory-used-percent, both on the same grid the endpoint
//! sent. It carries only what a host row draws — a label and two sparklines' worth of
//! data — and nothing about *how* it is drawn.
//!
//! ## A down host is not dropped
//!
//! The endpoint returns *every* host every time, in order; a host that is down arrives
//! with all-`null` arrays rather than vanishing. [`is_down`](HostSeries::is_down) reports
//! that — both series are entirely gaps — so the display can render the host's row as
//! "no data" and keep the layout stable, instead of the row silently disappearing and the
//! others sliding up.
//!
//! Pure and `no_std`, `Copy + Eq`.

use crate::name::HostName;
use crate::series::Series;

/// One host: its name, its CPU `%` series, and its memory `%` series.
///
/// `Copy + Eq`, so it rides inside the frame the render loop compares by value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HostSeries {
    name: HostName,
    cpu: Series,
    mem: Series,
}

impl HostSeries {
    /// The empty host — the fill for an unused slot in the frame.
    pub const EMPTY: HostSeries = HostSeries {
        name: HostName::EMPTY,
        cpu: Series::EMPTY,
        mem: Series::EMPTY,
    };

    /// A host from its name and its two series.
    pub const fn new(name: HostName, cpu: Series, mem: Series) -> Self {
        Self { name, cpu, mem }
    }

    /// The host's name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// The host's CPU busy-percent series.
    pub fn cpu(&self) -> &Series {
        &self.cpu
    }

    /// The host's memory used-percent series.
    pub fn mem(&self) -> &Series {
        &self.mem
    }

    /// Whether the host reported no data at all — both series are entirely gaps.
    ///
    /// This is the "down host" the contract sends as all-`null` arrays: the row is drawn as
    /// "no data", never dropped.
    pub fn is_down(&self) -> bool {
        self.cpu.all_gaps() && self.mem.all_gaps()
    }
}

impl Default for HostSeries {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_carries_its_name_and_series() {
        let host: HostSeries = HostSeries::new(
            HostName::new("fedora"),
            Series::from_wire(&[Some(11), Some(13)]),
            Series::from_wire(&[Some(41), Some(42)]),
        );
        assert_eq!(host.name(), "fedora");
        assert_eq!(host.cpu().len(), 2);
        assert_eq!(host.mem().len(), 2);
        assert!(!host.is_down());
    }

    #[test]
    fn a_host_with_all_null_series_is_down() {
        let host: HostSeries = HostSeries::new(
            HostName::new("oracle-amd"),
            Series::from_wire(&[None, None]),
            Series::from_wire(&[None, None]),
        );
        assert!(host.is_down(), "all-null arrays mean the host is down");
    }

    #[test]
    fn a_host_with_one_live_axis_is_not_down() {
        // CPU all gaps but memory has data: not "down", just a partial reading.
        let host: HostSeries = HostSeries::new(
            HostName::new("oracle-arm"),
            Series::from_wire(&[None, None]),
            Series::from_wire(&[Some(58), Some(59)]),
        );
        assert!(!host.is_down());
    }
}
