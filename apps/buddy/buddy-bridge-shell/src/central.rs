//! The `Central` port — the seam between the pure decision and the real BLE stack.
//!
//! The FSM in `buddy-bridge-core` decides *what* to do; this trait is *how*, expressed as the
//! handful of operations a BLE central performs against the stick. The concrete `BluerCentral`
//! implements it over bluer; tests implement a fake, so the whole
//! [`DriveLoop`](crate::drive_loop::DriveLoop) runs on the host with no device. The adapter's
//! contract is to map raw stack errors into the typed
//! [`CentralError`] the loop can decide on — the same discipline `host-core` uses for its
//! `PulseFault`.

use async_trait::async_trait;
use futures::Stream;

/// The outcome of a successful connect: whether BlueZ already holds a bond for the device.
///
/// This is what tells the loop to pair (fresh) or go straight to subscribing (already paired,
/// so `pair()` would be a no-op re-key — see the stale-LTK trap).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Connected {
    /// True if the device was already paired when the link came up.
    pub already_paired: bool,
}

/// Why a `Central` operation could not complete — the raw stack error, classified so the FSM
/// can decide on typed values rather than stringly-matching bluer's `Error`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CentralError {
    /// An operation was attempted with no live connection.
    NotConnected,
    /// The link failed to encrypt — the stale-LTK trap. The adapter raises this when the
    /// device was already paired yet the link dropped before it could be used (BlueZ offered a
    /// stale LTK the device rejected), since a specific bluer `ErrorKind` is not reliable.
    EncryptionFailed,
    /// `pair()` was refused because BlueZ still believes it is paired — the no-op re-key that
    /// cannot re-establish encryption; recovery is remove + re-pair.
    AlreadyPaired,
    /// No pairing agent is registered (the dropped-`AgentHandle` trap) — fatal, never retried.
    AgentMissing,
    /// A GATT-level failure (service/characteristic resolution, subscribe, write).
    Gatt(String),
    /// Any other bluer/BlueZ error, carried verbatim for the log.
    Bluer(String),
}

impl core::fmt::Display for CentralError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CentralError::NotConnected => write!(f, "not connected"),
            CentralError::EncryptionFailed => write!(f, "link encryption failed (stale bond?)"),
            CentralError::AlreadyPaired => write!(f, "already paired — re-key is a no-op"),
            CentralError::AgentMissing => write!(f, "no pairing agent registered"),
            CentralError::Gatt(why) => write!(f, "gatt error: {why}"),
            CentralError::Bluer(why) => write!(f, "bluer error: {why}"),
        }
    }
}

impl std::error::Error for CentralError {}

/// A BLE central against one peripheral: connect, bond, subscribe, write, and recover a stale
/// bond. Every method maps its stack error into a [`CentralError`].
#[async_trait]
pub trait Central: Send {
    /// The reassembly-free stream of raw TX notification payloads (each item is one ATT
    /// notification; fragmentation is the reason the caller reassembles).
    type Tx: Stream<Item = Vec<u8>> + Unpin + Send;

    /// Connect to the device (resolving GATT), and report whether it was already paired.
    async fn connect(&mut self) -> Result<Connected, CentralError>;

    /// Bond as initiator — the agent prompts for the passkey the device displays.
    async fn pair(&mut self) -> Result<(), CentralError>;

    /// Recover from a stale bond: `remove_device` then re-acquire the (now invalidated) handle.
    /// Leaves the device unpaired and ready for a fresh `connect`.
    async fn remove_and_reacquire(&mut self) -> Result<(), CentralError>;

    /// The negotiated ATT MTU (payload sizing is `mtu - 3`).
    async fn mtu(&self) -> Result<u16, CentralError>;

    /// Subscribe to TX notifications, returning the raw payload stream.
    async fn subscribe_tx(&mut self) -> Result<Self::Tx, CentralError>;

    /// Write one already-sized chunk to RX (write-without-response).
    async fn write_rx(&self, chunk: &[u8]) -> Result<(), CentralError>;
}
