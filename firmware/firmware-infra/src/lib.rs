#![forbid(unsafe_code)]
//! # firmware-infra
//!
//! Reusable firmware infrastructure for all three projects: WiFi STA bring-up,
//! the mDNS `_esphomelib._tcp` advertiser, the on-device native-API socket
//! server host (the blocking accept loop), and OTA.
//!
//!   - [`wifi`] — 2.4 GHz station bring-up + keep-alive (qhw.7) ✅
//!   - [`mdns`] — `_esphomelib._tcp` advertiser for HA discovery (qhw.8) ✅
//!
//! The server host lands in qhw.27 and OTA in qhw.12, each filling this crate's
//! `infra` seam in its own bead.

pub mod mdns;
pub mod wifi;
