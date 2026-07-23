//! The concrete [`Central`] over bluer 0.17.4 — the one place the real BLE stack is named.
//!
//! Everything the FSM decides, this executes against a live NUS peripheral: connect and
//! resolve the RX/TX characteristics, bond, subscribe, write chunked, and recover a stale
//! bond. Its one non-obvious job is **classifying** bluer's errors into the typed
//! [`CentralError`] the loop decides on — in particular, detecting the stale-LTK trap without
//! relying on a specific `bluer::ErrorKind` (which is not dependable): when the device was
//! already paired at connect time yet anything fails before the subscription is live, that is
//! treated as [`CentralError::EncryptionFailed`], routing the FSM to remove + re-pair.
//!
//! This adapter is device-tested only — a green host build proves it compiles, never that it
//! bonds. See `tests/device_bridge.rs` (`#[ignore]`d) and `just bridge-device`.

use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use bluer::gatt::remote::{Characteristic, CharacteristicWriteRequest, Service};
use bluer::gatt::WriteOp;
use bluer::{Adapter, Address, Device, DeviceEvent, DeviceProperty, Uuid};
use futures::{Stream, StreamExt};

use crate::central::{Central, CentralError, Connected};

/// The Nordic UART Service the stick advertises (byte-identical to the firmware + upstream).
pub const NUS_SERVICE: Uuid = Uuid::from_u128(0x6e40_0001_b5a3_f393_e0a9_e50e_24dc_ca9e);
/// RX: central → device, write (+ write-without-response).
pub const NUS_RX: Uuid = Uuid::from_u128(0x6e40_0002_b5a3_f393_e0a9_e50e_24dc_ca9e);
/// TX: device → central, notify.
pub const NUS_TX: Uuid = Uuid::from_u128(0x6e40_0003_b5a3_f393_e0a9_e50e_24dc_ca9e);

/// The default ATT payload before an MTU is negotiated (23-byte MTU minus 3).
const DEFAULT_MTU: u16 = 23;

/// How long one [`Central::locate`] scan runs before reporting [`CentralError::NotFound`].
///
/// The scan returns the instant the stick is seen, so this is only the cost of a *failed* look —
/// it bounds the wait so an absent device produces a steady, observable retry rhythm instead of
/// a daemon parked forever in a scan (the six-minute silence that opened this bug).
const SCAN_WINDOW: Duration = Duration::from_secs(20);

/// A BLE central bound to the stick *by advertised name*, driving it over bluer.
///
/// The binding is by name rather than address because the address is not knowable before the
/// first scan, and the device handle does not survive [`Central::remove_and_reacquire`]. Both
/// are re-established by [`Central::locate`], which is the only thing that sets `address` and
/// `device`.
pub struct BluerCentral {
    adapter: Adapter,
    name_prefix: String,
    address: Option<Address>,
    device: Option<Device>,
    rx: Option<Characteristic>,
    tx: Option<Characteristic>,
    mtu: u16,
    /// Whether BlueZ held a bond when the current connect began — the stale-LTK discriminator.
    was_paired_at_connect: bool,
}

impl BluerCentral {
    /// A central that will look for a peripheral whose advertised name starts with `name_prefix`.
    /// Nothing is resolved until [`Central::locate`] runs, so this cannot fail.
    pub fn new(adapter: Adapter, name_prefix: &str) -> Self {
        BluerCentral {
            adapter,
            name_prefix: name_prefix.to_string(),
            address: None,
            device: None,
            rx: None,
            tx: None,
            mtu: DEFAULT_MTU,
            was_paired_at_connect: false,
        }
    }

    /// The address this central last located, or `None` before the first successful scan.
    pub fn address(&self) -> Option<Address> {
        self.address
    }

    /// The located device handle, or [`CentralError::NotConnected`] when nothing has been
    /// located yet — the type-level statement that `locate` precedes every other operation.
    fn device(&self) -> Result<&Device, CentralError> {
        self.device.as_ref().ok_or(CentralError::NotConnected)
    }

    /// Classify an error that occurred while the device was (or was not) already paired: a
    /// failure on an already-paired link, before the subscription is live, is the stale-LTK
    /// trap; otherwise it is carried verbatim. This is the heuristic the plan mandates — robust
    /// to bluer not exposing a dependable encryption-failure `ErrorKind`.
    fn stale_or_bluer(&self, err: bluer::Error) -> CentralError {
        if self.was_paired_at_connect {
            CentralError::EncryptionFailed
        } else {
            CentralError::Bluer(err.to_string())
        }
    }

    /// Resolve the NUS RX/TX characteristics and the negotiated MTU on the live link.
    async fn resolve_gatt(&mut self) -> Result<(), CentralError> {
        let services: Vec<Service> = self
            .device()?
            .services()
            .await
            .map_err(|err: bluer::Error| self.stale_or_bluer(err))?;
        self.rx = None;
        self.tx = None;
        for service in services {
            let is_nus: bool = service
                .uuid()
                .await
                .map_err(|err: bluer::Error| self.stale_or_bluer(err))?
                == NUS_SERVICE;
            if !is_nus {
                continue;
            }
            let characteristics: Vec<Characteristic> = service
                .characteristics()
                .await
                .map_err(|err: bluer::Error| self.stale_or_bluer(err))?;
            for characteristic in characteristics {
                let uuid: Uuid = characteristic
                    .uuid()
                    .await
                    .map_err(|err: bluer::Error| self.stale_or_bluer(err))?;
                if uuid == NUS_RX {
                    self.rx = Some(characteristic);
                } else if uuid == NUS_TX {
                    self.tx = Some(characteristic);
                }
            }
        }
        if self.rx.is_none() || self.tx.is_none() {
            return Err(CentralError::Gatt(
                "NUS RX/TX characteristics not found".to_string(),
            ));
        }
        self.mtu = self.negotiated_mtu().await?;
        Ok(())
    }

    /// Read the negotiated ATT MTU off a write socket on RX (the only place bluer exposes it),
    /// then drop the socket — the actual writes go through `write_ext`.
    async fn negotiated_mtu(&self) -> Result<u16, CentralError> {
        let rx: &Characteristic = self.rx.as_ref().ok_or(CentralError::NotConnected)?;
        let writer = rx
            .write_io()
            .await
            .map_err(|err: bluer::Error| self.stale_or_bluer(err))?;
        let mtu: u16 = u16::try_from(writer.mtu()).unwrap_or(DEFAULT_MTU);
        Ok(mtu)
    }
}

#[async_trait]
impl Central for BluerCentral {
    type Tx = Pin<Box<dyn Stream<Item = Vec<u8>> + Send>>;

    async fn locate(&mut self) -> Result<(), CentralError> {
        let address: Address = discover(&self.adapter, &self.name_prefix, SCAN_WINDOW).await?;
        let device: Device = self
            .adapter
            .device(address)
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
        // A located device is a fresh handle: any characteristics resolved against the previous
        // one belong to a connection that no longer exists.
        self.address = Some(address);
        self.device = Some(device);
        self.rx = None;
        self.tx = None;
        Ok(())
    }

    async fn connect(&mut self) -> Result<Connected, CentralError> {
        // Read the bond state BEFORE connecting — it is the discriminator for the stale-LTK
        // trap, and it must be sampled while BlueZ still reports the pre-connect view.
        self.was_paired_at_connect = self
            .device()?
            .is_paired()
            .await
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
        self.device()?
            .connect()
            .await
            .map_err(|err: bluer::Error| self.stale_or_bluer(err))?;
        self.resolve_gatt().await?;
        Ok(Connected {
            already_paired: self.was_paired_at_connect,
        })
    }

    async fn pair(&mut self) -> Result<(), CentralError> {
        // pair() is a no-op re-key when BlueZ already holds a bond — surface it so the FSM can
        // recover rather than silently doing nothing.
        let already: bool = self
            .device()?
            .is_paired()
            .await
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
        if already {
            return Err(CentralError::AlreadyPaired);
        }
        self.device()?
            .pair()
            .await
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
        Ok(())
    }

    async fn remove_and_reacquire(&mut self) -> Result<(), CentralError> {
        let address: Address = self.address.ok_or(CentralError::NotConnected)?;
        // This is the expensive act: it destroys the bond, and the owner must type a passkey at
        // the glass to get one back. The FSM only reaches here on conclusive evidence, so say so
        // loudly rather than letting a re-pairing prompt appear from nowhere.
        log::warn!(
            "giving up the bond with {address} after repeated encryption failures — \
             the device will have to be paired again"
        );
        self.adapter
            .remove_device(address)
            .await
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
        // remove_device INVALIDATES the handle *and evicts the device from BlueZ entirely*, so
        // there is nothing to re-acquire here: only a fresh scan can produce a connectable
        // device. Drop everything and let the FSM's `Locate` rebuild it.
        self.device = None;
        self.rx = None;
        self.tx = None;
        Ok(())
    }

    async fn mtu(&self) -> Result<u16, CentralError> {
        Ok(self.mtu)
    }

    async fn subscribe_tx(&mut self) -> Result<Self::Tx, CentralError> {
        let tx: &Characteristic = self.tx.as_ref().ok_or(CentralError::NotConnected)?;
        // Subscribing writes the CCCD; on a stale bond this is exactly where encryption is
        // rejected, so route the failure through the same discriminator.
        let notifications = tx
            .notify()
            .await
            .map_err(|err: bluer::Error| self.stale_or_bluer(err))?;
        // The `Central` contract is that this stream ENDS when the link drops, so the loop sees
        // Disconnected and reconnects. bluer's notify stream only ends on the characteristic's
        // D-Bus `InterfacesRemoved` — but BlueZ CACHES GATT objects for a bonded device across a
        // disconnect, so a device reboot fires no removal and the notify stream would hang open
        // forever. Watch the device's `Connected` property and end the stream when it goes false.
        let device_events = self
            .device()?
            .events()
            .await
            .map_err(|err: bluer::Error| self.stale_or_bluer(err))?;
        let disconnected = async move {
            let mut device_events = Box::pin(device_events);
            while let Some(event) = device_events.next().await {
                if matches!(
                    event,
                    DeviceEvent::PropertyChanged(DeviceProperty::Connected(false))
                ) {
                    return;
                }
            }
        };
        let stream = notifications.take_until(Box::pin(disconnected));
        Ok(Box::pin(stream))
    }

    async fn write_rx(&self, chunk: &[u8]) -> Result<(), CentralError> {
        let rx: &Characteristic = self.rx.as_ref().ok_or(CentralError::NotConnected)?;
        // WriteOp::Command = write-without-response, matching the peripheral's RX flags.
        let request: CharacteristicWriteRequest = CharacteristicWriteRequest {
            op_type: WriteOp::Command,
            ..Default::default()
        };
        rx.write_ext(chunk, &request)
            .await
            .map_err(|err: bluer::Error| CentralError::Gatt(err.to_string()))
    }
}

/// Scan (filtered to the NUS service) for a peripheral whose advertised name starts with
/// `name_prefix`, returning its address, or [`CentralError::NotFound`] if `window` elapses
/// first. The discovery stream is dropped on return, which stops scanning.
///
/// ## Why this must be `discover_devices_with_changes`
///
/// A 128-bit service UUID and a name do not both fit in a 31-byte advertisement, so the firmware
/// puts the name in the **scan response** (see `bring_up_ble`) — it arrives strictly after the
/// advertisement that creates the device. bluer says so plainly: *"Device properties are queried
/// asynchronously and may not be available yet when a DeviceAdded event occurs. Use
/// discover_devices_with_changes when you want to be notified when the device properties
/// change."*
///
/// With plain `discover_devices` the name is `None` at `DeviceAdded`, the device is skipped, and
/// it is **never re-emitted** — so a cold daemon scans forever past a stick that is advertising
/// two feet away. It appeared to work only when something else (`bluetoothctl scan`) had already
/// cached the name, because `discover_devices` replays already-known addresses first and the
/// cached name resolves instantly. That is the whole bug: it could only find a device somebody
/// else had already found.
///
/// `discover_devices_with_changes` re-emits `DeviceAdded` on every property change, so the name
/// is re-checked when the scan response lands.
pub async fn discover(
    adapter: &Adapter,
    name_prefix: &str,
    window: Duration,
) -> Result<Address, CentralError> {
    tokio::time::timeout(window, matching_device(adapter, name_prefix))
        .await
        .unwrap_or(Err(CentralError::NotFound))
}

/// The unbounded scan [`discover`] puts a deadline on: resolve the first device whose name
/// matches, re-checking each time a device's properties change.
async fn matching_device(adapter: &Adapter, name_prefix: &str) -> Result<Address, CentralError> {
    use bluer::{AdapterEvent, DiscoveryFilter, DiscoveryTransport};
    use std::collections::HashSet;

    let filter: DiscoveryFilter = DiscoveryFilter {
        uuids: HashSet::from([NUS_SERVICE]),
        transport: DiscoveryTransport::Le,
        ..Default::default()
    };
    adapter
        .set_discovery_filter(filter)
        .await
        .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
    let mut events = adapter
        .discover_devices_with_changes()
        .await
        .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
    while let Some(event) = events.next().await {
        let AdapterEvent::DeviceAdded(address) = event else {
            continue;
        };
        let device: Device = adapter
            .device(address)
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
        // A device whose name is not resolved YET is skipped here and re-offered by the stream
        // as soon as the scan response lands — which is precisely what the `_with_changes`
        // variant guarantees and the plain one does not.
        let name: Option<String> = device
            .name()
            .await
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
        if name
            .as_deref()
            .is_some_and(|name: &str| name.starts_with(name_prefix))
        {
            return Ok(address);
        }
    }
    Err(CentralError::NotFound)
}
