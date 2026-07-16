//! Pulse — one hostpulse frame: the grid, and every host's CPU + memory series.
//!
//! The whole of what one `GET /pulse` yields, as a pure value: the sampling grid
//! (`step_s`, `window_s`, read from the payload, never hard-coded) and a bounded, ordered
//! list of [`HostSeries`]. Because the endpoint has already done the PromQL `rate()`, a
//! frame is *ready to plot* — the on-device rate arithmetic and Prometheus-text parsing
//! the old node_exporter path needed are gone; the domain now only holds N hosts × two
//! `%`-series and hands them to the display.
//!
//! ## Building a frame is the transform
//!
//! [`PulseBuilder`] is the pure JSON→model transform, minus the JSON: an adapter parses
//! the wire (that is *its* framework's job) and pushes each host's raw values here, where
//! the domain owns the two invariants that make a frame trustworthy —
//!
//! - **clamping**: each present value is coerced into `0..=100` ([`Percent::clamped`]), so
//!   a glitched `150` becomes `100` and never a bar taller than the plot;
//! - **gaps**: a `null` stays a gap ([`None`]), never a `0` — a missing scrape is not an
//!   idle one.
//!
//! and the frame is bounded to [`MAX_HOSTS`]. Keeping this in the domain — not the
//! adapter — is what lets every case (zero / one / many hosts, gaps, a down host, an empty
//! payload) be proven on the host with plain values.
//!
//! ## Fixed capacity, on purpose
//!
//! A frame is the render loop's `Copy + Eq` state, so it is a fixed `[HostSeries;
//! MAX_HOSTS]` plus a count, not a `Vec` — see [`Series`](crate::series). The homelab's
//! contract is three hosts; [`MAX_HOSTS`] carries one of headroom, and a payload with more
//! is bounded rather than allowed to grow the frame.

use crate::hostseries::HostSeries;
use crate::name::HostName;
use crate::percent::Percent;
use crate::series::Series;

/// The most hosts a frame retains.
///
/// The homelab's hostpulse returns exactly three (`fedora`, `oracle-arm`, `oracle-amd`);
/// this carries one of headroom. A payload with more hosts is bounded here — the extras
/// are dropped rather than overflowing the fixed frame — because the panel fits only so
/// many rows anyway (see `host_display`).
pub const MAX_HOSTS: usize = 4;

/// One hostpulse frame: the sampling grid and every host's two series.
///
/// `Copy + Eq`, so the display can wrap it and the render loop can compare it tick-to-tick
/// for change suppression. Hosts live in `hosts[..count]`, in the order the endpoint sent
/// them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pulse {
    step_s: u32,
    window_s: u32,
    hosts: [HostSeries; MAX_HOSTS],
    count: usize,
}

impl Pulse {
    /// The seconds between two adjacent samples on the grid, as the payload declared it.
    pub const fn step_s(&self) -> u32 {
        self.step_s
    }

    /// The width of the window the frame covers, in seconds, as the payload declared it.
    pub const fn window_s(&self) -> u32 {
        self.window_s
    }

    /// The hosts in this frame, in wire order.
    pub fn hosts(&self) -> &[HostSeries] {
        &self.hosts[..self.count]
    }

    /// How many hosts the frame holds, `0..=`[`MAX_HOSTS`].
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether the frame holds no hosts (an empty payload).
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The highest *latest* CPU reading across all hosts, `0` if none is present.
    ///
    /// A single scalar the display can band the whole frame by (the busiest host drives an
    /// aggregate glyph), without the display reaching into every series itself.
    pub fn peak_cpu(&self) -> u8 {
        self.hosts()
            .iter()
            .filter_map(|host: &HostSeries| host.cpu().latest())
            .map(Percent::value)
            .max()
            .unwrap_or(0)
    }
}

/// The pure builder for a [`Pulse`] — the domain half of the JSON→model transform.
///
/// An adapter deserializes the wire and drives this: [`new`](Self::new) with the grid,
/// [`push`](Self::push) once per host with its raw values, then [`build`](Self::build).
/// The clamping and gap policy live here, so the adapter stays a thin translation and the
/// interesting behaviour is host-tested.
pub struct PulseBuilder {
    step_s: u32,
    window_s: u32,
    hosts: [HostSeries; MAX_HOSTS],
    count: usize,
}

impl PulseBuilder {
    /// Start a frame on the grid the payload declared.
    pub fn new(step_s: u32, window_s: u32) -> Self {
        Self {
            step_s,
            window_s,
            hosts: [HostSeries::EMPTY; MAX_HOSTS],
            count: 0,
        }
    }

    /// Add one host, clamping its values into `0..=100` and keeping `null`s as gaps.
    ///
    /// `cpu` and `mem` are the raw wire arrays (a present integer, or [`None`] for a gap),
    /// oldest-first. Hosts past [`MAX_HOSTS`] are dropped — the frame is bounded — so this
    /// is a no-op once the frame is full.
    pub fn push(&mut self, name: &str, cpu: &[Option<i32>], mem: &[Option<i32>]) {
        if self.count >= MAX_HOSTS {
            return;
        }
        self.hosts[self.count] = HostSeries::new(
            HostName::new(name),
            Series::from_wire(cpu),
            Series::from_wire(mem),
        );
        self.count += 1;
    }

    /// Finish the frame.
    pub fn build(self) -> Pulse {
        Pulse {
            step_s: self.step_s,
            window_s: self.window_s,
            hosts: self.hosts,
            count: self.count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three-host homelab frame from the contract, built through the builder.
    fn homelab() -> Pulse {
        let mut b: PulseBuilder = PulseBuilder::new(30, 900);
        b.push(
            "fedora",
            &[Some(11), Some(13), None, Some(10)],
            &[Some(41), Some(44)],
        );
        b.push("oracle-arm", &[Some(3), Some(4)], &[Some(58), Some(60)]);
        b.push("oracle-amd", &[Some(1), Some(2)], &[Some(22), Some(24)]);
        b.build()
    }

    #[test]
    fn an_empty_payload_is_an_empty_frame() {
        let pulse: Pulse = PulseBuilder::new(30, 900).build();
        assert!(pulse.is_empty());
        assert_eq!(pulse.len(), 0);
        assert_eq!(pulse.hosts().len(), 0);
        assert_eq!(pulse.peak_cpu(), 0, "no hosts, no peak");
    }

    #[test]
    fn the_grid_is_read_from_the_payload_not_hard_coded() {
        let pulse: Pulse = PulseBuilder::new(15, 600).build();
        assert_eq!(pulse.step_s(), 15);
        assert_eq!(pulse.window_s(), 600);
    }

    #[test]
    fn one_host_is_carried_with_its_latest_values() {
        let mut b: PulseBuilder = PulseBuilder::new(30, 900);
        b.push("fedora", &[Some(11), Some(13)], &[Some(41), Some(44)]);
        let pulse: Pulse = b.build();
        assert_eq!(pulse.len(), 1);
        assert_eq!(pulse.hosts()[0].name(), "fedora");
        assert_eq!(pulse.hosts()[0].cpu().latest().unwrap().value(), 13);
        assert_eq!(pulse.hosts()[0].mem().latest().unwrap().value(), 44);
    }

    #[test]
    fn many_hosts_keep_their_wire_order() {
        let pulse: Pulse = homelab();
        assert_eq!(pulse.len(), 3);
        let names: Vec<&str> = pulse.hosts().iter().map(HostSeries::name).collect();
        assert_eq!(names, vec!["fedora", "oracle-arm", "oracle-amd"]);
    }

    #[test]
    fn a_gap_survives_the_transform() {
        let pulse: Pulse = homelab();
        let fedora_cpu: Vec<Option<u8>> = pulse.hosts()[0]
            .cpu()
            .samples()
            .iter()
            .map(|s: &Option<Percent>| s.map(Percent::value))
            .collect();
        assert_eq!(
            fedora_cpu,
            vec![Some(11), Some(13), None, Some(10)],
            "the null is a gap, not a zero"
        );
    }

    #[test]
    fn a_down_host_is_kept_in_order_as_no_data() {
        // The middle host is down (all-null): it stays in place, the others do not shift.
        let mut b: PulseBuilder = PulseBuilder::new(30, 900);
        b.push("fedora", &[Some(11)], &[Some(41)]);
        b.push("oracle-arm", &[None, None], &[None, None]);
        b.push("oracle-amd", &[Some(1)], &[Some(22)]);
        let pulse: Pulse = b.build();
        assert_eq!(pulse.len(), 3);
        assert!(!pulse.hosts()[0].is_down());
        assert!(
            pulse.hosts()[1].is_down(),
            "the down host is kept, not dropped"
        );
        assert!(!pulse.hosts()[2].is_down());
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let mut b: PulseBuilder = PulseBuilder::new(30, 900);
        b.push("fedora", &[Some(150)], &[Some(-3)]);
        let pulse: Pulse = b.build();
        assert_eq!(pulse.hosts()[0].cpu().latest().unwrap().value(), 100);
        assert_eq!(pulse.hosts()[0].mem().latest().unwrap().value(), 0);
    }

    #[test]
    fn the_frame_is_bounded_to_max_hosts() {
        let mut b: PulseBuilder = PulseBuilder::new(30, 900);
        for i in 0..(MAX_HOSTS + 2) {
            // Distinct one-sample series per host so the drop is observable by count.
            let _ = i;
            b.push("host", &[Some(50)], &[Some(50)]);
        }
        let pulse: Pulse = b.build();
        assert_eq!(pulse.len(), MAX_HOSTS, "hosts past the bound are dropped");
    }

    #[test]
    fn peak_cpu_is_the_busiest_host_latest() {
        // fedora latest cpu = 10, oracle-arm = 4, oracle-amd = 2 → peak 10.
        assert_eq!(homelab().peak_cpu(), 10);
    }
}
