#![forbid(unsafe_code)]
//! # buddy-wire
//!
//! The desk pet's NUS JSON wire contract — one line in becomes a typed message, and the
//! device's replies become bytes. Shared by **both** the firmware and the Linux bridge, so the
//! contract has a single, host-tested source of truth (mirroring how `host-wire` owns its
//! protocol).
//!
//! `std` but device-independent: it names no BLE stack and no socket. The firmware and the
//! bridge do the transport; deciding *what the bytes mean* is this crate's job, and it stays
//! host-testable.
//!
//! ## The layers
//! - [`framing`] — split the byte stream into lines, with truncation surfaced as a typed
//!   [`FrameError`] instead of a silent drop (defect fix c).
//! - [`inbound`] — classify a line into an exhaustive [`Inbound`], where only the snapshot may
//!   touch prompt state (defect fix b/D1), and merge snapshots asymmetrically on absent fields.
//! - [`command`] — the command vocabulary and its exhaustive dispatch to an [`Ack`], with an
//!   explicit nack for unknown commands (defect fix a/D3).
//! - [`permission`] — the device↔central [`PermissionResponse`]: the device serializes it with
//!   [`PermissionResponse::to_json`], the central reads it back with [`PermissionResponse::parse`].
//! - [`outbound`] — the central→device serializers (snapshot, time sync, connect messages): the
//!   daemon's half of the contract, the faithful mirror of [`parse_inbound`].
//!
//! ## NUS UUIDs (reference)
//!
//! The Nordic UART Service the transport uses — documented here, not spoken by this crate:
//! - Service `6e400001-b5a3-f393-e0a9-e50e24dcca9e`
//! - RX (central→device, write) `6e400002-b5a3-f393-e0a9-e50e24dcca9e`
//! - TX (device→central, notify) `6e400003-b5a3-f393-e0a9-e50e24dcca9e`

pub mod command;
pub mod framing;
pub mod inbound;
pub mod outbound;
pub mod permission;

pub use command::{dispatch, Ack, Command};
pub use framing::{FrameError, Framer, Ring, LINE_CAPACITY, RING_CAPACITY};
pub use inbound::{
    parse_inbound, BuddyEvent, Inbound, InboundError, Prompt, SnapshotPacket, SnapshotState,
};
pub use outbound::{serialize_name, serialize_owner, serialize_snapshot, serialize_time};
pub use permission::{Decision, PermissionParseError, PermissionResponse};
