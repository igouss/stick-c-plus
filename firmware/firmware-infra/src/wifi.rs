//! `wifi` — 2.4 GHz station bring-up and keep-alive (qhw.7).
//!
//! Joins the configured WPA2 network as a station, waits for a DHCP lease, and
//! keeps the link up across AP restarts. This is pure infrastructure: there is no
//! inward domain port for "WiFi" — the native-API server host (qhw.27) and OTA
//! (qhw.23) sit on top of the working TCP stack this brings up, so it lives here
//! in `firmware-infra`, not among the driven `adapters`.
//!
//! Credentials are never in the source tree: [`build.rs`] reads the git-ignored
//! `firmware/secrets.toml` and bakes the SSID/password in as the `WIFI_SSID` /
//! `WIFI_PASSWORD` env read below (qhw.7). The future Noise PSK (qhw.10) follows
//! the same path.
//!
//! Auto-reconnect is a supervisory loop, not an event callback: ESP-IDF does not
//! reconnect on its own, and the raw `esp_wifi_connect()` re-arm is an `unsafe`
//! FFI call this project forbids. [`WifiStation::ensure_connected`] re-joins from
//! safe Rust instead; the caller ticks it from its own loop, so a router reboot
//! is ridden out without a power cycle.

use esp_idf_hal::modem::Modem;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::ipv4::Ipv4Addr;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use esp_idf_sys::EspError;
use log::{info, warn};

/// The 2.4 GHz network, baked in at build time from `firmware/secrets.toml`.
///
/// These are compile-time constants, not tracked source: `build.rs` fails the
/// build if the secrets file is missing, so an image is never produced without
/// them (the second `env!` argument is the message shown if that guarantee ever
/// breaks).
const SSID: &str = env!(
    "WIFI_SSID",
    "WIFI_SSID not set — firmware-infra/build.rs must emit it"
);
const PASSWORD: &str = env!(
    "WIFI_PASSWORD",
    "WIFI_PASSWORD not set — firmware-infra/build.rs must emit it"
);

/// A WiFi bring-up failure: bad credentials, or an underlying ESP-IDF error.
#[derive(Debug)]
pub enum WifiError {
    /// The SSID (>32 B) or password (>64 B) overruns the 802.11 field. This is a
    /// mistyped `secrets.toml`, caught the first time the credentials are used.
    CredentialTooLong,
    /// An ESP-IDF call (driver init, association, DHCP) failed.
    Esp(EspError),
}

impl core::fmt::Display for WifiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CredentialTooLong => f.write_str(
                "wifi credential too long: SSID must be <=32 bytes, password <=64 (check secrets.toml)",
            ),
            Self::Esp(err) => write!(f, "wifi esp-idf error: {err}"),
        }
    }
}

impl std::error::Error for WifiError {}

impl From<EspError> for WifiError {
    fn from(err: EspError) -> Self {
        Self::Esp(err)
    }
}

/// A connected 2.4 GHz station, owning the WiFi driver for the life of the app.
///
/// Built by [`connect`](Self::connect), kept alive by
/// [`ensure_connected`](Self::ensure_connected). It owns the modem, so it — and
/// the netif behind [`ip`](Self::ip) — must outlive every consumer of the link
/// (the native-API server, OTA); the composition root holds it accordingly.
pub struct WifiStation<'d> {
    wifi: BlockingWifi<EspWifi<'d>>,
}

impl<'d> WifiStation<'d> {
    /// Join the configured network and block until a DHCP lease lands, logging
    /// the acquired IP.
    ///
    /// `modem`, `sysloop` and `nvs` come from the composition root: the modem is
    /// the board's single radio peripheral, the system event loop carries the
    /// WiFi/IP events the driver waits on, and NVS backs the radio's stored
    /// calibration (passing it avoids a re-calibrate every boot).
    pub fn connect(
        modem: Modem<'d>,
        sysloop: EspSystemEventLoop,
        nvs: EspDefaultNvsPartition,
    ) -> Result<Self, WifiError> {
        let mut wifi: BlockingWifi<EspWifi<'d>> =
            BlockingWifi::wrap(EspWifi::new(modem, sysloop.clone(), Some(nvs))?, sysloop)?;

        wifi.set_configuration(&client_configuration()?)?;
        wifi.start()?;
        info!("wifi: joining SSID {SSID:?}");
        wifi.connect()?;
        wifi.wait_netif_up()?;

        let station: Self = Self { wifi };
        info!("wifi: acquired IP {}", station.ip()?);
        Ok(station)
    }

    /// The current DHCP-assigned IPv4 of the station netif.
    pub fn ip(&self) -> Result<Ipv4Addr, EspError> {
        Ok(self.wifi.wifi().sta_netif().get_ip_info()?.ip)
    }

    /// Re-join if the link has dropped; a cheap no-op while it is up.
    ///
    /// The plant monitor must survive a router reboot without a power cycle
    /// (qhw.7). ESP-IDF does not reconnect on its own, so the caller ticks this
    /// from its loop: while associated and holding an IP it returns immediately;
    /// once the AP comes back it re-associates and waits for a fresh lease.
    pub fn ensure_connected(&mut self) -> Result<(), WifiError> {
        if self.wifi.is_connected()? && self.wifi.is_up()? {
            return Ok(());
        }
        warn!("wifi: link down, reconnecting");
        self.wifi.connect()?;
        self.wifi.wait_netif_up()?;
        info!("wifi: reconnected, IP {}", self.ip()?);
        Ok(())
    }
}

/// Resolve `host:port` through the DHCP-provided DNS servers.
///
/// Holding an IP proves the link and DHCP; resolving a name additionally proves
/// the resolver path — the DNS servers the lease handed us actually answer
/// (qhw.7). Kept infrastructure-side because it exercises the ESP netif's socket
/// stack, the same stack the native-API server (qhw.27) will bind.
pub fn resolve(host: &str, port: u16) -> std::io::Result<Vec<std::net::SocketAddr>> {
    use std::net::ToSocketAddrs;
    (host, port).to_socket_addrs().map(Iterator::collect)
}

/// The station configuration from the baked-in credentials.
///
/// Auth method follows the password: a non-empty password is WPA2-Personal, an
/// empty one an open network ([`AuthMethod::None`]) — pairing WPA2 with a blank
/// password would refuse to associate.
fn client_configuration() -> Result<Configuration, WifiError> {
    let auth_method: AuthMethod = if PASSWORD.is_empty() {
        AuthMethod::None
    } else {
        AuthMethod::WPA2Personal
    };
    Ok(Configuration::Client(ClientConfiguration {
        ssid: SSID.try_into().map_err(|_| WifiError::CredentialTooLong)?,
        password: PASSWORD
            .try_into()
            .map_err(|_| WifiError::CredentialTooLong)?,
        auth_method,
        ..Default::default()
    }))
}
