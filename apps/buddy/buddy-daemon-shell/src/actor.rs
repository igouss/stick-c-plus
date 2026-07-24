//! The single-owner **actor** that owns the pure [`Coordinator`] and performs its [`Effect`]s.
//!
//! The [`Coordinator`](buddy_daemon_core::Coordinator) is an ECB Control: it folds each input into
//! an ordered `Vec<Effect>` and owns nothing. This module gives it exactly one owner — a tokio task
//! reading a single [`Input`] channel — so every input (a hook arriving, a device line, a keepalive
//! tick, the link coming up or down) is serialized through one mailbox. There are **no locks and no
//! shared mutable state**: the actor is the sole mutator, which is what makes the whole permission
//! loop free of races by construction.
//!
//! The outside edges never touch the [`Coordinator`]; they hold a cheap-clone [`Daemon`] handle and
//! speak to the actor through it:
//! - [`Daemon::ask`] mints a [`HookId`], sends the request, and awaits the device's answer over a
//!   oneshot — and **fails safe to [`DaemonReply::Unbonded`]** if the actor is gone or drops the
//!   answer, so a dead daemon degrades the hook to the normal terminal prompt, never a silent allow.
//! - [`Daemon::link_up`] hands the actor a fresh outbound channel and returns its receiver — the
//!   *fake link* under test, the BLE writer in production.
//! - [`Daemon::link_down`] / [`Daemon::device_line`] / [`Daemon::keepalive`] are fire-and-forget.
//!
//! An [`Effect::SendLink`] is pushed onto the current outbound channel (dropped if the link is
//! down — the Coordinator only emits one while bonded); an [`Effect::ReplyHook`] resolves exactly
//! the waiting hook's oneshot.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use buddy_daemon_core::{Coordinator, Effect, HookId};
use buddy_permission::{DaemonReply, HookRequest};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

/// One message into the actor's single mailbox. Every outside edge becomes exactly one of these, so
/// the [`Coordinator`] sees a serialized stream of inputs and never a concurrent one.
enum Input {
    /// The link came up: the injected time + owner for the handshake, and the outbound channel the
    /// actor writes every subsequent `SendLink` line to until the link goes down.
    LinkUp {
        /// The injected wall-clock seconds for the time-sync handshake line.
        epoch: i64,
        /// The injected tz offset (seconds east of UTC) for the time-sync handshake line.
        tz_offset_s: i32,
        /// The owner label shown on the glass.
        owner: String,
        /// The outbound sink for this link session's serialized lines.
        out: mpsc::UnboundedSender<String>,
    },
    /// The link went down: drain every pending hook to Unbonded and forget the outbound channel.
    LinkDown,
    /// A hook connected: answer it over `reply` once the device decides (or the link drops).
    Hook {
        /// The id the handle minted for this waiting hook.
        id: HookId,
        /// The forwarded tool-call context.
        req: HookRequest,
        /// The oneshot the actor resolves with the device's answer.
        reply: oneshot::Sender<DaemonReply>,
    },
    /// A whole reassembled line arrived from the device.
    DeviceLine(Vec<u8>),
    /// A keepalive tick: re-emit the current gated snapshot iff bonded.
    Keepalive,
}

/// A cheap-clone handle to the running actor. Every outside edge (the socket server, the keepalive
/// loop, the BLE link pump) holds one; cloning shares the same mailbox and hook-id counter.
#[derive(Clone)]
pub struct Daemon {
    /// The actor's mailbox. A send failing means the actor task is gone — every method degrades safe.
    tx: mpsc::UnboundedSender<Input>,
    /// The monotonic source of [`HookId`]s, shared across clones so ids never collide.
    next_hook: Arc<AtomicU64>,
}

impl Daemon {
    /// Spawn the actor on the current tokio runtime and return its handle plus the task's
    /// [`JoinHandle`] (so the composition root can supervise it).
    pub fn spawn() -> (Daemon, JoinHandle<()>) {
        let (tx, rx): (mpsc::UnboundedSender<Input>, mpsc::UnboundedReceiver<Input>) =
            mpsc::unbounded_channel();
        let task: JoinHandle<()> = tokio::spawn(run(rx));
        let daemon: Daemon = Daemon {
            tx,
            next_hook: Arc::new(AtomicU64::new(0)),
        };
        (daemon, task)
    }

    /// Forward a hook's tool-call context and block on the device's answer.
    ///
    /// Fails safe: if the actor is gone (send fails) or drops the oneshot without answering, this
    /// resolves to [`DaemonReply::Unbonded`] — which the hook renders as silence, handing control to
    /// Claude Code's normal terminal prompt. There is no path here from a lost daemon to an `Allow`.
    pub async fn ask(&self, req: HookRequest) -> DaemonReply {
        let id: HookId = HookId(self.next_hook.fetch_add(1, Ordering::Relaxed));
        let (reply_tx, reply_rx): (oneshot::Sender<DaemonReply>, oneshot::Receiver<DaemonReply>) =
            oneshot::channel();
        let sent: Result<(), _> = self.tx.send(Input::Hook {
            id,
            req,
            reply: reply_tx,
        });
        // The actor is gone: degrade to the safe fail-open guard, never a guessed allow.
        if sent.is_err() {
            return DaemonReply::Unbonded;
        }
        // The actor dropped the answer without deciding (task ended mid-flight): same safe degrade.
        reply_rx.await.unwrap_or(DaemonReply::Unbonded)
    }

    /// Announce the link is up and return the receiver the actor will write this session's lines to
    /// — the fake link under test, the BLE writer in production. Time is INJECTED (`epoch` / `tz`).
    pub fn link_up(
        &self,
        epoch: i64,
        tz_offset_s: i32,
        owner: &str,
    ) -> mpsc::UnboundedReceiver<String> {
        let (out_tx, out_rx): (
            mpsc::UnboundedSender<String>,
            mpsc::UnboundedReceiver<String>,
        ) = mpsc::unbounded_channel();
        let _ = self.tx.send(Input::LinkUp {
            epoch,
            tz_offset_s,
            owner: owner.to_string(),
            out: out_tx,
        });
        out_rx
    }

    /// Announce the link went down; the actor drains every pending hook to Unbonded.
    pub fn link_down(&self) {
        let _ = self.tx.send(Input::LinkDown);
    }

    /// Hand the actor one whole reassembled line from the device.
    pub fn device_line(&self, bytes: Vec<u8>) {
        let _ = self.tx.send(Input::DeviceLine(bytes));
    }

    /// Tick the keepalive: the actor re-emits the current gated snapshot iff bonded.
    pub fn keepalive(&self) {
        let _ = self.tx.send(Input::Keepalive);
    }
}

/// The actor loop: own the [`Coordinator`], the waiting hooks, and the current outbound channel;
/// serialize every input through this one task.
async fn run(mut rx: mpsc::UnboundedReceiver<Input>) {
    // The owner's home, read once here — the one place in the daemon that may touch the
    // environment — so every hint on the glass shows `~/code/…` rather than spending a third of
    // its forty-eight characters on a prefix every directory on the machine shares.
    let mut coordinator: Coordinator = Coordinator::with_home(std::env::var("HOME").ok());
    // The oneshot back to each waiting hook, keyed by the id its ReplyHook effect names.
    let mut pending: HashMap<HookId, oneshot::Sender<DaemonReply>> = HashMap::new();
    // The current link session's outbound sink, if bonded.
    let mut link_out: Option<mpsc::UnboundedSender<String>> = None;
    while let Some(input) = rx.recv().await {
        step(&mut coordinator, &mut pending, &mut link_out, input);
    }
}

/// Fold one input into the [`Coordinator`] and perform the effects it returns. Each input's own
/// state (the outbound channel it carries, the oneshot it waits on) is registered here, so the
/// effects that follow can reach it.
fn step(
    coordinator: &mut Coordinator,
    pending: &mut HashMap<HookId, oneshot::Sender<DaemonReply>>,
    link_out: &mut Option<mpsc::UnboundedSender<String>>,
    input: Input,
) {
    match input {
        Input::LinkUp {
            epoch,
            tz_offset_s,
            owner,
            out,
        } => {
            // Register the outbound channel BEFORE folding, so the handshake's SendLink lines reach
            // it.
            *link_out = Some(out);
            let effects: Vec<Effect> = coordinator.on_link_up(epoch, tz_offset_s, &owner);
            perform(pending, link_out, effects);
        }
        Input::LinkDown => {
            let effects: Vec<Effect> = coordinator.on_link_down();
            perform(pending, link_out, effects);
            // The link is gone: forget the sink so a later stray SendLink cannot target it.
            *link_out = None;
        }
        Input::Hook { id, req, reply } => {
            // Register the waiting hook BEFORE folding, so a same-batch ReplyHook could resolve it.
            pending.insert(id, reply);
            let effects: Vec<Effect> = coordinator.on_hook(id, &req);
            perform(pending, link_out, effects);
        }
        Input::DeviceLine(bytes) => {
            let effects: Vec<Effect> = coordinator.on_device_line(&bytes);
            perform(pending, link_out, effects);
        }
        Input::Keepalive => {
            let effects: Vec<Effect> = coordinator.on_keepalive();
            perform(pending, link_out, effects);
        }
    }
}

/// Perform the Control's effects in order: push each `SendLink` line to the current outbound
/// channel (dropped if the link is down — the Coordinator only emits one while bonded), and resolve
/// each `ReplyHook` to exactly the waiting hook's oneshot.
fn perform(
    pending: &mut HashMap<HookId, oneshot::Sender<DaemonReply>>,
    link_out: &Option<mpsc::UnboundedSender<String>>,
    effects: Vec<Effect>,
) {
    effects.into_iter().for_each(|effect: Effect| match effect {
        Effect::SendLink(line) => {
            if let Some(out) = link_out.as_ref() {
                let _ = out.send(line);
            }
        }
        Effect::ReplyHook(id, reply) => {
            if let Some(answer) = pending.remove(&id) {
                let _ = answer.send(reply);
            }
        }
    });
}
