#![forbid(unsafe_code)]
//! # esphome-server
//!
//! The reusable **native-API server host**: the imperative shell that turns a TCP
//! socket into served ESPHome entities, so Home Assistant can adopt a device and
//! read (or command) it. Both firmware binaries — the plant monitor and the LED
//! driver — build on this one accept loop; it holds no project-specific logic.
//!
//! ## Hexagon
//! A **driving (primary) adapter**. It owns the effects the pure core must not —
//! a [`std::net::TcpListener`], a sized [`std::thread`] per connection, blocking
//! reads with timeouts — and drives the api core with them:
//!
//! ```text
//!   TCP bytes ──▶ FrameReader ──▶ Inbound ──▶ esphome_api::handle ──▶ Outbound ──▶ FrameWriter ──▶ TCP bytes
//!                 (esphome-api, generic over Read/Write — the real TcpStream is injected here)
//! ```
//!
//! All protocol logic lives in [`esphome_api`] (role `domain`, a pure functional
//! core: the frame codec, the entity model, the connection FSM); this crate only
//! moves bytes and owns the concurrency. Because it names no hardware and no
//! concrete entity type — it serves any [`Device`] — every rule is exercised on
//! the host with a loopback client and the real aioesphomeapi client, and the same
//! code cross-compiles to esp-idf std unchanged (the on-target socket comes from
//! lwIP under the same [`std::net`]). This is the sibling of `plant-shell`: one
//! turns time into domain calls, the other turns network events into them.
//!
//! ## Using it
//! The composition root implements [`Device`] over its entity registry, binds a
//! [`Server`], and serves:
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use esphome_server::{Device, Server, ServerConfig};
//! # use esphome_api::proto::DeviceInfoResponse;
//! # use esphome_api::{ListMessage, StateMessage};
//! # struct PlantDevice;
//! # impl Device for PlantDevice {
//! #     fn device_info(&self) -> DeviceInfoResponse { DeviceInfoResponse::default() }
//! #     fn list_messages(&self) -> Vec<ListMessage> { Vec::new() }
//! #     fn state_messages(&self) -> Vec<StateMessage> { Vec::new() }
//! # }
//! let server: Server = Server::bind(ServerConfig::default())?;
//! server.serve(Arc::new(PlantDevice))?; // blocks; run on a dedicated thread on-device
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! ## Scope
//! Plaintext framing only. Noise transport encryption is the follow-on
//! (`qhw.10`), which re-sizes the per-connection stack ([`PLAINTEXT_STACK_SIZE`])
//! and wraps the same pumps. Command routing (HA → controllable entity) arrives
//! with the LED driver (`fmz.1`), which first extends the FSM's `Inbound` set;
//! this host serves the read path — describe, list, subscribe, stream — that a
//! sensor device needs today.

mod device;
mod server;

pub use device::Device;
pub use server::{Server, ServerConfig, ServerHandle, NATIVE_API_PORT, PLAINTEXT_STACK_SIZE};
