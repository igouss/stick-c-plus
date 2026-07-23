//! The unix-socket boundary against a **real** [`UnixListener`] and a real client connection — the
//! few tests that prove a hook process's request crosses the socket, reaches the actor, and the
//! device's answer crosses back. The pure loop is proven in `loop.rs`; here we exercise the socket,
//! the line framing, and the request/reply round-trip against a live server.

use std::path::PathBuf;
use std::time::Duration;

use buddy_daemon_shell::{socket, Daemon};
use buddy_permission::{DaemonReply, HookRequest};
use buddy_wire::{Decision, PermissionResponse};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc::UnboundedReceiver;

/// The injected handshake time (seconds).
const EPOCH: i64 = 1_700_000_000;
/// The injected tz offset (seconds east of UTC).
const TZ: i32 = -14_400;
/// The injected owner label.
const OWNER: &str = "Iouri";

/// A unique socket path per test (name) and run (pid), under the system temp dir.
fn temp_sock(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("buddy-daemon-{name}-{}.sock", std::process::id()))
}

/// A hook request for `session`; the other fields are fixed context.
fn a_request(session: &str) -> HookRequest {
    HookRequest {
        session_id: session.to_string(),
        tool_name: "Bash".to_string(),
        cwd: "/repo".to_string(),
        permission_mode: "default".to_string(),
        transcript_path: "/tmp/t.jsonl".to_string(),
    }
}

/// Spawn the socket server for `daemon` on a fresh listener at `path`, returning once it is bound.
async fn serve_at(daemon: Daemon, path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let listener: UnixListener = UnixListener::bind(path).expect("bind daemon socket");
    tokio::spawn(socket::serve(daemon, listener));
}

/// Connect as a hook would, send one request line, and read the one reply line back.
async fn ask_over_socket(path: &std::path::Path, req: &HookRequest) -> DaemonReply {
    let stream: UnixStream = UnixStream::connect(path).await.expect("connect to daemon");
    let (read_half, mut write_half) = stream.into_split();
    let mut line: String = serde_json::to_string(req).expect("encode request");
    line.push('\n');
    write_half
        .write_all(line.as_bytes())
        .await
        .expect("send request");
    let mut reader: BufReader<_> = BufReader::new(read_half);
    let mut reply: String = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut reply))
        .await
        .expect("a reply within 2s")
        .expect("read reply");
    serde_json::from_str(reply.trim()).expect("a DaemonReply line")
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

/// The next outbound line from the fake link, or a test failure within two seconds.
async fn recv(out: &mut UnboundedReceiver<String>) -> String {
    tokio::time::timeout(Duration::from_secs(2), out.recv())
        .await
        .expect("a line within 2s")
        .expect("the fake link stays open")
}

/// A hook request over the socket, with nothing bonded, gets the Unbonded fail-safe back.
#[tokio::test]
async fn a_hook_over_the_socket_while_unbonded_reads_unbonded() {
    let (daemon, _task): (Daemon, _) = Daemon::spawn();
    let path: PathBuf = temp_sock("unbonded");
    serve_at(daemon, &path).await;
    let reply: DaemonReply = ask_over_socket(&path, &a_request("s1")).await;
    assert_eq!(reply, DaemonReply::Unbonded);
    let _ = std::fs::remove_file(&path);
}

/// The full boundary path: a hook request over the socket raises a prompt on the fake link, a device
/// `once` answers it, and the hook reads Allow back over the same socket.
#[tokio::test]
async fn a_hook_over_the_socket_reads_allow_when_the_device_approves() {
    let (daemon, _task): (Daemon, _) = Daemon::spawn();
    let mut out: UnboundedReceiver<String> = daemon.link_up(EPOCH, TZ, OWNER);
    // Drain the three-line handshake.
    recv(&mut out).await;
    recv(&mut out).await;
    recv(&mut out).await;
    let path: PathBuf = temp_sock("allow");
    serve_at(daemon.clone(), &path).await;

    // The hook blocks on the socket; drive the device answer concurrently.
    let client: tokio::task::JoinHandle<DaemonReply> = {
        let path: PathBuf = path.clone();
        tokio::spawn(async move { ask_over_socket(&path, &a_request("s1")).await })
    };
    let id: String =
        prompt_id(&recv(&mut out).await).expect("the raised snapshot carries a prompt");
    daemon.device_line(
        PermissionResponse::new(id, Decision::Once)
            .to_json()
            .into_bytes(),
    );
    assert_eq!(
        client.await.expect("the hook client joins"),
        DaemonReply::Allow
    );
    let _ = std::fs::remove_file(&path);
}

/// A malformed request line is dropped: the hook client reads EOF (an empty reply), never a guessed
/// decision.
#[tokio::test]
async fn a_malformed_request_line_is_dropped_with_no_reply() {
    let (daemon, _task): (Daemon, _) = Daemon::spawn();
    let path: PathBuf = temp_sock("malformed");
    serve_at(daemon, &path).await;
    let stream: UnixStream = UnixStream::connect(&path).await.expect("connect to daemon");
    let (read_half, mut write_half) = stream.into_split();
    write_half
        .write_all(b"not json\n")
        .await
        .expect("send line");
    let mut reader: BufReader<_> = BufReader::new(read_half);
    let mut reply: String = String::new();
    let read: usize = tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut reply))
        .await
        .expect("EOF within 2s")
        .expect("read to EOF");
    assert_eq!(read, 0, "a malformed request yields EOF, not a reply line");
    let _ = std::fs::remove_file(&path);
}
