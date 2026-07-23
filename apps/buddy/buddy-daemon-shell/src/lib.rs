#![forbid(unsafe_code)]
//! # buddy-daemon-shell
//!
//! The async imperative shell of the permission daemon. It runs the pure
//! [`Coordinator`](buddy_daemon_core::Coordinator) — an ECB Control that folds each input into an
//! ordered `Vec<Effect>` — against the real world, and performs those effects. The Control stays the
//! **single owner** of all permission state behind one actor task, so the whole loop is free of
//! locks and races by construction.
//!
//! ## The edges this shell wires
//!
//! - [`Daemon`] — the cheap-clone handle onto the actor. [`Daemon::ask`] forwards a hook and blocks
//!   on the device's answer (failing safe to [`DaemonReply`](buddy_permission::DaemonReply)`::Unbonded`
//!   if the actor is gone); [`Daemon::link_up`] / [`Daemon::link_down`] / [`Daemon::device_line`] /
//!   [`Daemon::keepalive`] feed the link and the clock.
//! - [`socket::serve`] — the unix-socket boundary: each per-tool-call hook connection becomes one
//!   `ask`. A malformed or early-closed connection is dropped, so the hook degrades to the normal
//!   terminal prompt — the safe outcome.
//! - [`keepalive::run`] — the interval that keeps the glass warm; the Control decides what a tick
//!   means (a gated snapshot iff bonded, never clearing a live prompt).
//!
//! ## The two fail-open hazards, preserved across the async edge
//!
//! Both hazards are closed in the pure Control; this shell must not reopen them:
//! 1. **HOST fail-open.** Every way [`Daemon::ask`] can lose the actor resolves to `Unbonded`, which
//!    the hook renders as silence — never a guessed allow. The socket never invents a reply either:
//!    the only line written back is a real [`DaemonReply`] the actor produced.
//! 2. **DEVICE fail-open.** Every outbound line originates from a Control `SendLink` effect, and the
//!    Control builds every snapshot through its single emission gate — so no line this shell writes
//!    can clear a live prompt.
//!
//! ## Host-testable against a fake link
//!
//! [`Daemon::link_up`] returns the outbound channel's receiver: that receiver **is** the fake link.
//! The whole permission loop — handshake, raise a prompt, device answer, keepalive, link-down drain
//! — is exercised over real async channels with no BLE and no device (`tests/loop.rs`), and the
//! socket boundary against a real [`UnixListener`](tokio::net::UnixListener) (`tests/socket.rs`).
//!
//! ## Hexagon
//!
//! - **role** = `driving-adapter`, **context** = `buddy`. A primary adapter: it turns outside events
//!   into folds of the Control and performs the returned effects. It owns effects (the actor task,
//!   the unix socket, the channels, the interval) but drives inward through the Control.
//! - Dependencies point **inward**: `buddy-daemon-core` (the Control) and `buddy-permission` (the
//!   IPC DTOs), plus tokio / serde_json / log as infrastructure. No BLE stack lives here — the BLE
//!   link is fed through [`Daemon`] by the composition root, so this crate stays device-free and
//!   host-tested.

pub mod actor;
pub mod keepalive;
pub mod socket;

pub use actor::Daemon;
