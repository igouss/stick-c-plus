#![forbid(unsafe_code)]
//! # net
//!
//! Shared M5StickC Plus firmware networking, on `std`/ESP-IDF. The board-generic
//! infrastructure every networked app reuses:
//!
//!   - [`wifi`] — 2.4 GHz station bring-up + keep-alive, plus a DNS [`resolve`](wifi::resolve)
//!     helper. Credentials are baked in at build time from the git-ignored
//!     `firmware/secrets.toml` (see `build.rs`).
//!
//! This is pure infrastructure (role=infra): there is no inward domain port for "WiFi".
//! The apps' TCP consumers sit on top of the working stack it brings up — the plant
//! monitor's native-API server, the host monitor's HTTP metrics scraper — so this crate
//! is shared by both rather than living inside either app.
//!
//! App-specific network services do **not** belong here: the plant monitor's ESPHome
//! mDNS advertiser stays in its own `firmware-infra`, because it pulls the
//! `espressif/mdns` managed component that a non-ESPHome app has no use for.

pub mod wifi;
