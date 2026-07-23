#![forbid(unsafe_code)]
//! # buddy-daemon-core
//!
//! The pure **coordinator** sequencing the permission loop between the per-tool-call PreToolUse
//! hooks and the single bonded BLE link — an ECB **Control**. It owns **no** effects: no unix
//! socket, no BLE stack, no clock, no thread, no `std::process`. It folds inputs (a link coming
//! up or down, a hook arriving, a device line, a lifecycle event, a keepalive tick) into an
//! ordered [`Vec<Effect>`](Effect) the async daemon — a separate hand phase — actually performs.
//!
//! This is the host-testable heart of the daemon IPC. The async adapter (a `UnixListener` plus the
//! live link) runs THIS coordinator against the real world; because the coordinator returns effects
//! rather than performing them, the whole permission loop is exercised on the host with no I/O.
//!
//! ## The two fail-open hazards it must not reopen
//!
//! 1. **HOST fail-open** — Claude Code proceeds on a hook timeout. When nothing is bonded the
//!    coordinator answers a hook with [`DaemonReply::Unbonded`](buddy_permission::DaemonReply) and
//!    raises no prompt, so [`buddy_permission::decide`] degrades to the normal terminal prompt. The
//!    coordinator never fabricates an approval: an unknown or unparseable device line yields no
//!    reply at all (an empty effect list), never a guessed allow.
//! 2. **DEVICE fail-open** — an absent `prompt` field in a snapshot legitimately means "the prompt
//!    is gone", so a bare keepalive emitted while a prompt is outstanding would clear a live
//!    approval off the glass. Every snapshot this crate emits is built by the single private
//!    [`Coordinator`] helper that synthesizes the ledger and passes the packet through
//!    [`GatedSnapshot::gate`](buddy_permission::GatedSnapshot) before
//!    [`serialize_snapshot`](buddy_wire::serialize_snapshot) — so a bare keepalive that clears a
//!    live prompt is unrepresentable from the bridge side.
//!
//! ## Hexagon
//!
//! - **role** = `port-and-adapter`, **context** = `buddy`. An ECB Control that binds to the
//!   bridge-side policy (`buddy_permission`: [`Ledger`](buddy_permission::Ledger),
//!   [`GatedSnapshot`](buddy_permission::GatedSnapshot),
//!   [`HookRequest`](buddy_permission::HookRequest),
//!   [`DaemonReply`](buddy_permission::DaemonReply)) and the wire contract (`buddy_wire`:
//!   [`PermissionResponse`](buddy_wire::PermissionResponse),
//!   [`serialize_snapshot`](buddy_wire::serialize_snapshot),
//!   [`serialize_time`](buddy_wire::serialize_time),
//!   [`serialize_owner`](buddy_wire::serialize_owner)). A `domain` role would be an ILLEGAL outward
//!   edge to those crates.
//! - Dependencies point **inward** and are exactly `buddy-permission` and `buddy-wire`. No tokio,
//!   no `std::net`, no `std::process`, no `std::os`, no clock — time is INJECTED into
//!   [`Coordinator::on_link_up`].
//!
//! ## The modules
//!
//! - [`effect`] — the output alphabet: [`HookId`](effect::HookId) (the opaque handle the async
//!   layer assigns to a waiting hook) and [`Effect`](effect::Effect) (a serialized line to send
//!   down the link, or a reply to a specific waiting hook). Order within the returned `Vec` is
//!   significant.
//! - [`coordinator`] — the [`Coordinator`](coordinator::Coordinator) itself: the fold from each
//!   input to an ordered `Vec<Effect>`, and the single gated-snapshot emission helper.

pub mod coordinator;
pub mod effect;

pub use coordinator::Coordinator;
pub use effect::{Effect, HookId};
