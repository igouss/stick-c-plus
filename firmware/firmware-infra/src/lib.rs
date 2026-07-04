#![forbid(unsafe_code)]
//! # firmware-infra
//!
//! Reusable firmware infrastructure for all three projects: WiFi STA bring-up,
//! the mDNS `_esphomelib._tcp` advertiser, and OTA — the ESP-IDF-backed services
//! that need `esp-idf-svc`.
//!
//!   - [`wifi`] — 2.4 GHz station bring-up + keep-alive (qhw.7) ✅
//!   - [`mdns`] — `_esphomelib._tcp` advertiser for HA discovery (qhw.8) ✅
//!
//! The native-API **server host** (qhw.27, the blocking accept loop) is *not*
//! here: it needs only portable `std` (`TcpListener`, threads), so it lives in the
//! host crate `esphome-server` — verified on the host, cross-compiled to esp-idf
//! `std` unchanged — and the composition root pulls it by path. OTA (qhw.12) fills
//! this crate's remaining `infra` seam in its own bead.

pub mod mdns;
pub mod wifi;
