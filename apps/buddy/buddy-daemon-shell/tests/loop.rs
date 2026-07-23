//! The permission loop end-to-end against a **fake link** — the outbound channel
//! [`Daemon::link_up`] returns. No BLE, no device: real async channels prove the plumbing the pure
//! Coordinator's tests cannot reach, that the actor performs its effects on the right edges in the
//! right order. Integration tests, kept few, to prove the wire made it through.

use std::time::Duration;

use buddy_daemon_shell::Daemon;
use buddy_permission::{DaemonReply, HookRequest};
use buddy_wire::{Decision, PermissionResponse};
use tokio::sync::mpsc::UnboundedReceiver;

/// The injected handshake time (seconds), fixed so every emitted line is deterministic.
const EPOCH: i64 = 1_700_000_000;
/// The injected tz offset (seconds east of UTC).
const TZ: i32 = -14_400;
/// The injected owner label.
const OWNER: &str = "Iouri";

/// A hook request for `session` invoking `tool`; the other fields are fixed context.
fn a_request(session: &str, tool: &str) -> HookRequest {
    HookRequest {
        session_id: session.to_string(),
        tool_name: tool.to_string(),
        cwd: "/repo".to_string(),
        permission_mode: "default".to_string(),
        transcript_path: "/tmp/t.jsonl".to_string(),
    }
}

/// The next outbound line from the fake link, or a test failure within two seconds.
async fn recv(out: &mut UnboundedReceiver<String>) -> String {
    tokio::time::timeout(Duration::from_secs(2), out.recv())
        .await
        .expect("a line within 2s")
        .expect("the fake link stays open")
}

/// Whether a serialized snapshot line carries a `prompt` key.
fn line_has_prompt(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .map(|value: serde_json::Value| value.get("prompt").is_some())
        .unwrap_or(false)
}

/// The `prompt.id` of a serialized snapshot line, if it carries one.
fn prompt_id(line: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()?
        .get("prompt")?
        .get("id")?
        .as_str()
        .map(str::to_string)
}

/// Bring a fresh daemon's link up and drain the three-line handshake, returning the daemon and the
/// fake link's receiver positioned just past the handshake.
async fn bonded() -> (Daemon, UnboundedReceiver<String>) {
    let (daemon, _task): (Daemon, _) = Daemon::spawn();
    let mut out: UnboundedReceiver<String> = daemon.link_up(EPOCH, TZ, OWNER);
    // The handshake: time sync, owner, the current (prompt-less) snapshot — three lines, in order.
    recv(&mut out).await;
    recv(&mut out).await;
    recv(&mut out).await;
    (daemon, out)
}

/// The link-up handshake is exactly the time sync, then the owner, then a prompt-less snapshot.
#[tokio::test]
async fn link_up_emits_the_handshake_in_order() {
    let (daemon, _task): (Daemon, _) = Daemon::spawn();
    let mut out: UnboundedReceiver<String> = daemon.link_up(EPOCH, TZ, OWNER);
    assert_eq!(
        recv(&mut out).await,
        format!(r#"{{"time":[{EPOCH},{TZ}]}}"#)
    );
    assert_eq!(recv(&mut out).await, buddy_wire::serialize_owner(OWNER));
    assert!(!line_has_prompt(&recv(&mut out).await));
}

/// A hook against an unbonded daemon resolves Unbonded at once — the HOST fail-safe, with no link.
#[tokio::test]
async fn a_hook_while_unbonded_resolves_unbonded() {
    let (daemon, _task): (Daemon, _) = Daemon::spawn();
    let reply: DaemonReply = daemon.ask(a_request("s1", "bash")).await;
    assert_eq!(reply, DaemonReply::Unbonded);
}

/// The full loop: a hook raises a prompt on the glass, a device `once` answers it — the hook
/// resolves Allow and a fresh prompt-less snapshot clears the glass.
#[tokio::test]
async fn a_device_once_resolves_the_hook_allow_and_clears_the_glass() {
    let (daemon, mut out): (Daemon, UnboundedReceiver<String>) = bonded().await;
    let asker: Daemon = daemon.clone();
    let ask: tokio::task::JoinHandle<DaemonReply> =
        tokio::spawn(async move { asker.ask(a_request("s1", "bash")).await });
    // The on_hook snapshot carries the fresh prompt.
    let raised: String = recv(&mut out).await;
    let id: String = prompt_id(&raised).expect("the raised snapshot carries a prompt id");
    // Answer it on the device.
    daemon.device_line(
        PermissionResponse::new(id, Decision::Once)
            .to_json()
            .into_bytes(),
    );
    // The fresh snapshot clears the prompt, and the waiting hook resolves Allow.
    assert!(!line_has_prompt(&recv(&mut out).await));
    assert_eq!(ask.await.expect("the ask task joins"), DaemonReply::Allow);
}

/// The deny path mirrors the allow path: a device `deny` resolves the hook Deny.
#[tokio::test]
async fn a_device_deny_resolves_the_hook_deny() {
    let (daemon, mut out): (Daemon, UnboundedReceiver<String>) = bonded().await;
    let asker: Daemon = daemon.clone();
    let ask: tokio::task::JoinHandle<DaemonReply> =
        tokio::spawn(async move { asker.ask(a_request("s1", "bash")).await });
    let id: String = prompt_id(&recv(&mut out).await).expect("a raised prompt id");
    daemon.device_line(
        PermissionResponse::new(id, Decision::Deny)
            .to_json()
            .into_bytes(),
    );
    assert!(!line_has_prompt(&recv(&mut out).await));
    assert_eq!(ask.await.expect("the ask task joins"), DaemonReply::Deny);
}

/// A keepalive tick while a prompt is outstanding re-emits a snapshot that STILL carries the prompt
/// — the DEVICE fail-safe survives the async edge: a heartbeat never clears a live prompt.
#[tokio::test]
async fn a_keepalive_keeps_a_live_prompt_on_the_glass() {
    let (daemon, mut out): (Daemon, UnboundedReceiver<String>) = bonded().await;
    let asker: Daemon = daemon.clone();
    let _ask: tokio::task::JoinHandle<DaemonReply> =
        tokio::spawn(async move { asker.ask(a_request("s1", "bash")).await });
    // Consume the raise snapshot, then tick a keepalive.
    assert!(line_has_prompt(&recv(&mut out).await));
    daemon.keepalive();
    assert!(line_has_prompt(&recv(&mut out).await));
}

/// A hook that arrives after the actor task is gone resolves Unbonded — a dead daemon degrades the
/// hook to the terminal prompt, never a guessed allow (the send-failed branch of the fail-safe).
#[tokio::test]
async fn a_hook_after_the_actor_died_resolves_unbonded() {
    let (daemon, task): (Daemon, tokio::task::JoinHandle<()>) = Daemon::spawn();
    task.abort();
    // Await the aborted task so its receiver is dropped before we ask — the send then fails.
    let _ = task.await;
    assert_eq!(
        daemon.ask(a_request("s1", "bash")).await,
        DaemonReply::Unbonded
    );
}

/// A hook already waiting when the actor task dies resolves Unbonded — the dropped oneshot degrades
/// safe, never leaving the hook to guess an allow (the answer-dropped branch of the fail-safe).
#[tokio::test]
async fn a_waiting_hook_resolves_unbonded_when_the_actor_dies() {
    let (daemon, task): (Daemon, tokio::task::JoinHandle<()>) = Daemon::spawn();
    let mut out: UnboundedReceiver<String> = daemon.link_up(EPOCH, TZ, OWNER);
    recv(&mut out).await;
    recv(&mut out).await;
    recv(&mut out).await;
    let asker: Daemon = daemon.clone();
    let ask: tokio::task::JoinHandle<DaemonReply> =
        tokio::spawn(async move { asker.ask(a_request("s1", "bash")).await });
    // The prompt is raised (pending stored); now kill the actor mid-flight.
    assert!(line_has_prompt(&recv(&mut out).await));
    task.abort();
    let _ = task.await;
    assert_eq!(
        ask.await.expect("the ask task joins"),
        DaemonReply::Unbonded
    );
}

/// The link dropping drains a waiting hook to Unbonded — no stale approval lingers on a dead link.
#[tokio::test]
async fn a_link_down_drains_a_waiting_hook_unbonded() {
    let (daemon, mut out): (Daemon, UnboundedReceiver<String>) = bonded().await;
    let asker: Daemon = daemon.clone();
    let ask: tokio::task::JoinHandle<DaemonReply> =
        tokio::spawn(async move { asker.ask(a_request("s1", "bash")).await });
    // Wait until the prompt is actually raised, so the drain has something to drain.
    assert!(line_has_prompt(&recv(&mut out).await));
    daemon.link_down();
    assert_eq!(
        ask.await.expect("the ask task joins"),
        DaemonReply::Unbonded
    );
}
