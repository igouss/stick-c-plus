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

use async_trait::async_trait;
use bluer::gatt::remote::{Characteristic, CharacteristicWriteRequest, Service};
use bluer::gatt::WriteOp;
use bluer::{Adapter, Address, Device, Uuid};
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

/// A BLE central bound to one peripheral by address, driving it over bluer.
pub struct BluerCentral {
    adapter: Adapter,
    address: Address,
    device: Device,
    rx: Option<Characteristic>,
    tx: Option<Characteristic>,
    mtu: u16,
    /// Whether BlueZ held a bond when the current connect began — the stale-LTK discriminator.
    was_paired_at_connect: bool,
}

impl BluerCentral {
    /// Bind to the device at `address` on `adapter` (found by a prior scan).
    pub fn new(adapter: Adapter, address: Address) -> Result<Self, CentralError> {
        let device: Device = adapter
            .device(address)
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
        Ok(BluerCentral {
            adapter,
            address,
            device,
            rx: None,
            tx: None,
            mtu: DEFAULT_MTU,
            was_paired_at_connect: false,
        })
    }

    /// The address this central is bound to.
    pub fn address(&self) -> Address {
        self.address
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
            .device
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

    async fn connect(&mut self) -> Result<Connected, CentralError> {
        // Read the bond state BEFORE connecting — it is the discriminator for the stale-LTK
        // trap, and it must be sampled while BlueZ still reports the pre-connect view.
        self.was_paired_at_connect = self
            .device
            .is_paired()
            .await
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
        self.device
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
            .device
            .is_paired()
            .await
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
        if already {
            return Err(CentralError::AlreadyPaired);
        }
        self.device
            .pair()
            .await
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
        Ok(())
    }

    async fn remove_and_reacquire(&mut self) -> Result<(), CentralError> {
        // Clear BlueZ's stale bond. remove_device INVALIDATES the Device handle, so re-acquire
        // a fresh one and drop the resolved characteristics — the next connect re-resolves.
        self.adapter
            .remove_device(self.address)
            .await
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
        self.device = self
            .adapter
            .device(self.address)
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
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
        let stream = tx
            .notify()
            .await
            .map_err(|err: bluer::Error| self.stale_or_bluer(err))?;
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
/// `name_prefix`, returning its address. The discovery stream is dropped on return, which stops
/// scanning.
pub async fn discover(adapter: &Adapter, name_prefix: &str) -> Result<Address, CentralError> {
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
        .discover_devices()
        .await
        .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
    while let Some(event) = events.next().await {
        let AdapterEvent::DeviceAdded(address) = event else {
            continue;
        };
        // The name rides in the scan response, so it may not be present on the first event;
        // a device whose name is not yet known is simply skipped and re-seen on a later event.
        let device: Device = adapter
            .device(address)
            .map_err(|err: bluer::Error| CentralError::Bluer(err.to_string()))?;
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
    Err(CentralError::Bluer(format!(
        "discovery ended before a '{name_prefix}' device appeared"
    )))
}
