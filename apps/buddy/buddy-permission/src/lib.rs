#![forbid(unsafe_code)]
//! # buddy-permission
//!
//! The bridge-side permission + heartbeat **policy** for the desk pet — pure, time-injected, and
//! effect-free. It *decides*; the long-lived daemon (a later hand phase) *does*: the unix socket,
//! the bonded BLE link, the clock, and stdin/stdout all live in the daemon and the per-tool-call
//! hook binary, never here.
//!
//! This crate exists to close **two fail-open hazards**, both live:
//!
//! 1. **HOST fail-open** — Claude Code treats a PreToolUse hook timeout as a non-blocking error and
//!    PROCEEDS with the tool call. A stick that is asleep, out of range, or unbonded would silently
//!    wave everything through. [`hook::decide`] is the guard: only a real device decision emits an
//!    allow/deny; every other outcome ([`hook::AskOutcome::TimedOut`] / `DaemonDown` / `Unbonded`)
//!    is [`hook::HookOutput::Silent`] — print NOTHING, and Claude Code's normal terminal prompt
//!    takes over. Degrading to the ordinary prompt is safe; silently proceeding is not. There is
//!    NO path from a non-decided outcome to an emit, and no `"ask"` / `"defer"` anywhere (upstream
//!    #39344: an `"ask"` hook can silently disable `permissions.deny` rules).
//!
//! 2. **DEVICE fail-open** — an absent `prompt` field in a snapshot legitimately means "the prompt
//!    is gone", so a bare keepalive snapshot emitted while a prompt is outstanding would clear a
//!    live approval off the glass. [`gating::GatedSnapshot`] makes that unrepresentable: a
//!    synthesized snapshot must pass the gate, which re-attaches the live prompt whenever one is
//!    outstanding, before the daemon serializes it. Clearing the prompt is a DELIBERATE act (an
//!    explicit [`ledger::SessionEvent::Resolved`]), never a side effect of a heartbeat.
//!
//! ## Hexagon
//!
//! - **role** = `port-and-adapter`, **context** = `buddy`. The same role as `buddy-wire`: this
//!   crate binds bridge-side policy to the wire DTOs (`SnapshotPacket`, `Decision`,
//!   `PermissionResponse`, `serialize_snapshot`) and the domain (`buddy_core::TokenLatch`). A
//!   `domain` role would be an ILLEGAL outward edge to `buddy-wire`.
//! - Dependencies point **inward** and are exactly `buddy-core` and `buddy-wire` (plus serde for
//!   the IPC DTOs). No BLE stack, no tokio, no socket, no `std::process`, no clock.
//!
//! ## The modules
//!
//! - [`hook`] — the fail-safe PreToolUse decision: [`AskOutcome`](hook::AskOutcome) →
//!   [`HookOutput`](hook::HookOutput), and its exact stdout rendering.
//! - [`ledger`] — session → snapshot synthesis: fold already-typed session events into the
//!   heartbeat [`SnapshotPacket`](buddy_wire::SnapshotPacket) the device expects, crediting tokens
//!   through [`TokenLatch`](buddy_core::TokenLatch).
//! - [`gating`] — the device-side anti-hazard: [`GatedSnapshot`](gating::GatedSnapshot) keeps a
//!   live prompt on every emitted snapshot.
//! - [`ipc`] — the hook↔daemon message DTOs (ours to define): the [`HookRequest`](ipc::HookRequest)
//!   the hook forwards and the [`DaemonReply`](ipc::DaemonReply) that maps back to an
//!   [`AskOutcome`](hook::AskOutcome).

pub mod gating;
pub mod hook;
pub mod ipc;
pub mod ledger;

pub use gating::GatedSnapshot;
pub use hook::{decide, to_stdout, AskOutcome, HookOutput, Permission, PreToolUseDecision};
pub use ipc::{DaemonReply, HookRequest};
pub use ledger::{Ledger, SessionEvent, SessionId};
