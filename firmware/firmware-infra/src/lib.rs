#![forbid(unsafe_code)]
//! # firmware-infra
//!
//! Reusable firmware infrastructure for all three projects: WiFi STA bring-up,
//! the mDNS `_esphomelib._tcp` advertiser, the on-device native-API socket
//! server host (the blocking accept loop), and OTA.
//!
//! Skeleton only (qhw.2 workspace carve): WiFi lands in qhw.7, mDNS in qhw.8,
//! the server host in qhw.27 and OTA in qhw.12. This crate exists now so the
//! workspace seam and its hex-arch `infra` role are in place for them to fill.
