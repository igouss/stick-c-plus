//! The coordinator's output alphabet — what the async daemon must perform, in order.
//!
//! The [`Coordinator`](crate::coordinator::Coordinator) owns no effects; it names them. Each fold
//! returns a `Vec<Effect>` whose **order is significant** — the handshake precedes the first
//! snapshot, and a hook reply is emitted together with (and before) the fresh snapshot that clears
//! its prompt. The async adapter walks the vec front-to-back and performs each effect.

use buddy_permission::DaemonReply;

/// An opaque handle the async layer assigns to a waiting hook connection, so an [`Effect`] can name
/// which hook to answer.
///
/// The coordinator never invents these — the async adapter mints one per accepted hook connection
/// and hands it to [`Coordinator::on_hook`](crate::coordinator::Coordinator::on_hook); the
/// coordinator only stores it and later names it in a [`Effect::ReplyHook`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct HookId(pub u64);

/// One effect the async daemon performs. **Order within the returned `Vec` is significant.**
///
/// - [`SendLink`](Effect::SendLink) — one already-serialized JSON line to write down the bonded
///   link (a handshake message, or a gated snapshot). Every snapshot line here has passed the
///   emission gate, so it can never clear a live prompt.
/// - [`ReplyHook`](Effect::ReplyHook) — answer a specific waiting hook with the daemon's decision.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Effect {
    /// Write one serialized JSON line down the bonded link.
    SendLink(String),
    /// Answer the named waiting hook with a [`DaemonReply`].
    ReplyHook(HookId, DaemonReply),
}
