#![forbid(unsafe_code)]
//! # firmware-infra
//!
//! The plant monitor's ESPHome-specific firmware infrastructure: the mDNS
//! advertiser that makes the device discoverable to Home Assistant.
//!
//!   - [`mdns`] — `_esphomelib._tcp` advertiser for HA discovery (qhw.8) ✅
//!
//! Board-generic networking — WiFi STA bring-up and the DNS resolve helper — is
//! **not** here: it is reused by every networked app, so it lives in the shared
//! `firmware/platform/net` crate. This crate keeps only what is ESPHome/HA-specific,
//! because the mDNS advertiser pulls the `espressif/mdns` managed component that a
//! non-ESPHome app (the host monitor) has no use for.
//!
//! The native-API **server host** (qhw.27, the blocking accept loop) is likewise not
//! here: it needs only portable `std` (`TcpListener`, threads), so it lives in the
//! host crate `esphome-server` — verified on the host, cross-compiled to esp-idf
//! `std` unchanged — and the composition root pulls it by path. OTA (qhw.12) fills
//! this crate's remaining `infra` seam in its own bead.

pub mod mdns;
