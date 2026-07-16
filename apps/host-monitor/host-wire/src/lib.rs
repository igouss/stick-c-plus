#![forbid(unsafe_code)]
//! # host-wire
//!
//! The hostpulse wire codec — one `GET /pulse` JSON body turned into a domain [`Pulse`].
//!
//! The firmware adapter performs the network round-trip; deciding *what the bytes mean* is
//! this crate's job, and it stays device-independent so the wire contract is host-tested
//! rather than only exercised on the metal. [`parse_pulse`] deserializes the small fixed
//! JSON with `serde_json` and folds each host through the pure
//! [`PulseBuilder`](host_core::PulseBuilder), which owns the clamping (`0..=100`) and gap
//! handling. Keeping the codec here — not in the framework-free domain, and not duplicated
//! in the firmware adapter — gives the contract a single, tested source of truth.
//!
//! ## The contract
//!
//! ```json
//! {
//!   "step_s": 30,
//!   "window_s": 900,
//!   "hosts": [
//!     { "name": "fedora", "cpu": [11, 13, null, 10], "mem": [41, 42, 43, 44] }
//!   ]
//! }
//! ```
//!
//! `step_s` / `window_s` are read from the payload, never assumed. `cpu` / `mem` are
//! integer percents, oldest-first, with a `null` for a gap (a missing scrape, *not* `0`).
//! `hosts` is every host in order; a down host arrives with all-`null` arrays and is kept,
//! not dropped. Unknown fields are ignored, so the endpoint can grow the payload without
//! breaking the device.

use host_core::{Pulse, PulseBuilder};
use serde::Deserialize;

/// The wire shape of one `GET /pulse` response — the DTO `serde_json` deserializes into.
///
/// A faithful mirror of the contract, and nothing more: it is translated into the domain
/// [`Pulse`] immediately by [`parse_pulse`], never held or exposed. Unknown fields are
/// ignored (no `deny_unknown_fields`) so a future payload extension does not fail the
/// device.
#[derive(Debug, Deserialize)]
struct WirePulse {
    /// Seconds between adjacent samples on the grid.
    step_s: u32,
    /// Width of the window the frame covers, in seconds.
    window_s: u32,
    /// Every host, in order; a down host has all-`null` arrays.
    hosts: Vec<WireHost>,
}

/// The wire shape of one host inside a `/pulse` response.
///
/// `cpu` / `mem` are integer percents oldest-first, `null` for a gap. They are read as
/// `Option<i32>` — wide enough to accept an out-of-range integer, which the domain builder
/// then clamps into `0..=100` rather than rejecting the whole frame.
#[derive(Debug, Deserialize)]
struct WireHost {
    /// The host's name, e.g. `oracle-arm`.
    name: String,
    /// CPU busy-percent series, oldest-first, `null` for a gap.
    cpu: Vec<Option<i32>>,
    /// Memory used-percent series, oldest-first, `null` for a gap.
    mem: Vec<Option<i32>>,
}

/// Why a `/pulse` body could not be turned into a [`Pulse`].
///
/// A single case today — the body was not JSON matching the contract (wrong service, an
/// error page, a truncated read, or a missing required field). It is [`Display`] so the
/// firmware adapter can log the detail, and it classifies to
/// [`HostFault::Malformed`](host_core::HostFault::Malformed): the endpoint answered, but
/// not usefully.
///
/// [`Display`]: core::fmt::Display
#[derive(Debug)]
pub enum WireError {
    /// The body was not a `/pulse` frame — see [`serde_json::Error`] for where it diverged.
    Json(serde_json::Error),
}

impl core::fmt::Display for WireError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WireError::Json(err) => write!(f, "pulse body was not a usable frame: {err}"),
        }
    }
}

impl std::error::Error for WireError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WireError::Json(err) => Some(err),
        }
    }
}

impl From<serde_json::Error> for WireError {
    fn from(err: serde_json::Error) -> Self {
        WireError::Json(err)
    }
}

/// Deserialize a `/pulse` body and fold it into a domain [`Pulse`] frame.
///
/// The whole codec: `serde_json` parses the small fixed JSON, then each host is pushed
/// through the pure [`PulseBuilder`](host_core::PulseBuilder), which clamps every present
/// value into `0..=100` and keeps `null`s as gaps. Fails only when the body is not a frame
/// matching the contract — a well-formed frame with out-of-range numbers is clamped, not
/// rejected.
pub fn parse_pulse(body: &[u8]) -> Result<Pulse, WireError> {
    let wire: WirePulse = serde_json::from_slice(body)?;
    let mut builder: PulseBuilder = PulseBuilder::new(wire.step_s, wire.window_s);
    for host in &wire.hosts {
        builder.push(&host.name, &host.cpu, &host.mem);
    }
    Ok(builder.build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_core::{HostSeries, Percent};

    /// The three-host homelab frame from the contract.
    const CONTRACT: &str = r#"{
        "step_s": 30,
        "window_s": 900,
        "hosts": [
            { "name": "fedora",     "cpu": [11,13,9,12,null,10], "mem": [41,42,42,43,43,44] },
            { "name": "oracle-arm", "cpu": [3,4,3,5,4,4],        "mem": [58,58,59,59,60,60] },
            { "name": "oracle-amd", "cpu": [1,2,1,1,2,1],        "mem": [22,22,23,23,23,24] }
        ]
    }"#;

    #[test]
    fn the_contract_payload_parses_to_three_named_hosts_on_the_declared_grid() {
        let pulse: Pulse = parse_pulse(CONTRACT.as_bytes()).expect("the contract must parse");
        assert_eq!(pulse.step_s(), 30);
        assert_eq!(pulse.window_s(), 900);
        let names: Vec<&str> = pulse.hosts().iter().map(HostSeries::name).collect();
        assert_eq!(names, vec!["fedora", "oracle-arm", "oracle-amd"]);
    }

    #[test]
    fn a_null_is_read_as_a_gap_not_a_zero() {
        let pulse: Pulse = parse_pulse(CONTRACT.as_bytes()).expect("parse");
        let fedora_cpu: Vec<Option<u8>> = pulse.hosts()[0]
            .cpu()
            .samples()
            .iter()
            .map(|s: &Option<Percent>| s.map(Percent::value))
            .collect();
        assert_eq!(
            fedora_cpu,
            vec![Some(11), Some(13), Some(9), Some(12), None, Some(10)]
        );
        assert_eq!(
            pulse.hosts()[0].cpu().latest().map(Percent::value),
            Some(10),
            "latest skips no trailing gap here — the last sample is present"
        );
    }

    #[test]
    fn an_empty_hosts_array_is_an_empty_frame() {
        let pulse: Pulse =
            parse_pulse(br#"{"step_s":30,"window_s":900,"hosts":[]}"#).expect("parse");
        assert!(pulse.is_empty());
    }

    #[test]
    fn a_down_host_with_all_null_arrays_is_kept_as_no_data() {
        let body: &str = r#"{"step_s":30,"window_s":900,"hosts":[
            {"name":"oracle-amd","cpu":[null,null,null],"mem":[null,null,null]}
        ]}"#;
        let pulse: Pulse = parse_pulse(body.as_bytes()).expect("parse");
        assert_eq!(pulse.len(), 1);
        assert!(pulse.hosts()[0].is_down());
    }

    #[test]
    fn out_of_range_values_are_clamped_not_rejected() {
        let body: &str = r#"{"step_s":30,"window_s":900,"hosts":[
            {"name":"fedora","cpu":[150],"mem":[-4]}
        ]}"#;
        let pulse: Pulse = parse_pulse(body.as_bytes()).expect("a valid frame, clamped");
        assert_eq!(
            pulse.hosts()[0].cpu().latest().map(Percent::value),
            Some(100)
        );
        assert_eq!(pulse.hosts()[0].mem().latest().map(Percent::value), Some(0));
    }

    #[test]
    fn a_non_json_body_is_an_error() {
        assert!(parse_pulse(b"not json at all").is_err());
    }

    #[test]
    fn a_502_error_body_is_not_a_frame() {
        // The 502 status is handled by the adapter; if such a body ever reached the codec it
        // is not a frame, so it errors rather than parsing to something bogus.
        assert!(parse_pulse(br#"{"error":"prometheus_unavailable"}"#).is_err());
    }

    #[test]
    fn a_missing_required_field_is_an_error() {
        // No `window_s`: the contract requires it, so this is malformed.
        assert!(parse_pulse(br#"{"step_s":30,"hosts":[]}"#).is_err());
    }

    #[test]
    fn unknown_fields_are_ignored_for_forward_compatibility() {
        let body: &str = r#"{"step_s":30,"window_s":900,"generated_at":"2026",
            "hosts":[{"name":"fedora","cpu":[10],"mem":[40],"extra":true}]}"#;
        let pulse: Pulse = parse_pulse(body.as_bytes()).expect("unknown fields are tolerated");
        assert_eq!(pulse.len(), 1);
    }
}
