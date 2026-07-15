//! `mdns` — advertise the device as an ESPHome native-API device (qhw.8).
//!
//! Home Assistant auto-discovers ESPHome devices by browsing mDNS for
//! `_esphomelib._tcp`. This module publishes that service on port 6053 with the
//! TXT records HA reads to raise its discovery notification. Like [`wifi`], it is
//! pure infrastructure — no inward domain port — so it lives in `firmware-infra`;
//! the composition root assembles the facts (from the WiFi station's MAC, the
//! board identity, the firmware version) and hands them here to advertise.
//!
//! The advert is *live*: the returned [`EspMdns`] handle owns the mDNS service
//! task, so it must be kept alive for the life of the app — dropping it retracts
//! the record (which is exactly how powering the device off makes it disappear
//! within the TTL, qhw.8 acceptance).
//!
//! Backed by the ESP-IDF `espressif/mdns` managed component (wired in via
//! esp-idf-sys; see this crate's Cargo.toml). Noise's `api_encryption` TXT is
//! deliberately absent — it is added only once the native API speaks Noise
//! (qhw.10); its presence is what tells HA a device is encrypted.

use esp_idf_svc::mdns::EspMdns;
use esp_idf_sys::EspError;

/// The ESPHome native-API TCP port every `_esphomelib._tcp` device advertises.
pub const ESPHOME_API_PORT: u16 = 6053;

/// The facts advertised for one ESPHome device: the mDNS identity plus the TXT
/// records Home Assistant reads on discovery.
///
/// Borrowed strings — the composition root owns the backing storage for the life
/// of the advert. `network` is fixed to `wifi` (the only transport here); the
/// Noise `api_encryption` key is intentionally not modelled until qhw.10.
pub struct EsphomeService<'a> {
    /// mDNS hostname, sans `.local` — the device answers `<hostname>.local`.
    pub hostname: &'a str,
    /// Human-readable service instance name (shown by browsers / HA).
    pub instance_name: &'a str,
    /// `friendly_name` TXT — HA's display name for the device.
    pub friendly_name: &'a str,
    /// `version` TXT — the firmware version string.
    pub version: &'a str,
    /// `mac` TXT — lowercase hex, no separators (see [`esphome_mac`]).
    pub mac: &'a str,
    /// `platform` TXT — the chip family, e.g. `ESP32`.
    pub platform: &'a str,
    /// `board` TXT — the board identifier, e.g. `m5stick-c`.
    pub board: &'a str,
}

/// Publish `service` as a live `_esphomelib._tcp` advert on the LAN.
///
/// Must run after the WiFi netif is up (mDNS binds to it). Returns the owning
/// [`EspMdns`] handle: keep it alive to keep advertising.
pub fn advertise(service: &EsphomeService) -> Result<EspMdns, EspError> {
    let mut mdns: EspMdns = EspMdns::take()?;
    mdns.set_hostname(service.hostname)?;
    mdns.set_instance_name(service.instance_name)?;

    let txt: [(&str, &str); 6] = [
        ("friendly_name", service.friendly_name),
        ("version", service.version),
        ("mac", service.mac),
        ("platform", service.platform),
        ("board", service.board),
        ("network", "wifi"),
    ];
    mdns.add_service(
        Some(service.instance_name),
        "_esphomelib",
        "_tcp",
        ESPHOME_API_PORT,
        &txt,
    )?;
    Ok(mdns)
}

/// Format a 6-byte MAC as the ESPHome `mac` TXT value: lowercase hex, no
/// separators (e.g. `[0xC8, 0x85, 0x41, 0x4E, 0x02, 0x90]` -> `c885414e0290`).
///
/// This is the identifier HA keys ESPHome devices on, so the format is exact.
pub fn esphome_mac(mac: &[u8; 6]) -> String {
    mac.iter().map(|byte| format!("{byte:02x}")).collect()
}
