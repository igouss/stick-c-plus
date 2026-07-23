//! Integration proof of the hook↔daemon socket plumbing: a real [`UnixListener`] stands in for the
//! daemon, and we assert the outcome the hook resolves for each answer it can get. These are the
//! few tests that prove the wire actually made it through — the pure decision is unit-tested in the
//! library; here we exercise the socket, the framing, and the deadline against a live fake.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use buddy_hook::resolve;
use buddy_permission::{AskOutcome, HookRequest};
use buddy_wire::Decision;

/// A unique socket path per test (name) and run (pid), under the system temp dir.
fn temp_sock(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("buddy-hook-{name}-{}.sock", std::process::id()))
}

/// A one-shot fake daemon: binds `path`, accepts a single connection, and hands the stream to
/// `handler`. On drop it joins the responder thread and removes the socket file.
struct FakeDaemon {
    path: PathBuf,
    thread: Option<JoinHandle<()>>,
}

impl FakeDaemon {
    fn spawn(name: &str, handler: impl FnOnce(UnixStream) + Send + 'static) -> Self {
        let path: PathBuf = temp_sock(name);
        let _ = std::fs::remove_file(&path);
        let listener: UnixListener = UnixListener::bind(&path).expect("bind fake daemon socket");
        let thread: JoinHandle<()> = thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                handler(stream);
            }
        });
        FakeDaemon {
            path,
            thread: Some(thread),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FakeDaemon {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The request a hook forwards, with the two required fields set.
fn a_request() -> HookRequest {
    HookRequest {
        session_id: "s1".to_string(),
        tool_name: "Bash".to_string(),
        cwd: "/w".to_string(),
        permission_mode: "default".to_string(),
        transcript_path: "/t.jsonl".to_string(),
    }
}

/// Read the one request line the hook sends, then write `reply` as one JSON line.
fn read_then_reply(mut stream: UnixStream, reply: &str) {
    let mut line: String = String::new();
    let mut reader: BufReader<&UnixStream> = BufReader::new(&stream);
    let _ = reader.read_line(&mut line);
    let _ = stream.write_all(format!("{reply}\n").as_bytes());
    let _ = stream.flush();
}

/// An `Allow` reply resolves to a real device decision to approve.
#[test]
fn an_allow_reply_resolves_to_a_once_decision() {
    let daemon: FakeDaemon = FakeDaemon::spawn("allow", |stream: UnixStream| {
        read_then_reply(stream, "\"Allow\"");
    });
    let outcome: AskOutcome = resolve(&a_request(), daemon.path(), Duration::from_millis(500));
    assert_eq!(outcome, AskOutcome::Decided(Decision::Once));
}

/// A `Deny` reply resolves to a real device decision to deny.
#[test]
fn a_deny_reply_resolves_to_a_deny_decision() {
    let daemon: FakeDaemon = FakeDaemon::spawn("deny", |stream: UnixStream| {
        read_then_reply(stream, "\"Deny\"");
    });
    let outcome: AskOutcome = resolve(&a_request(), daemon.path(), Duration::from_millis(500));
    assert_eq!(outcome, AskOutcome::Decided(Decision::Deny));
}

/// An `Unbonded` reply is the daemon-is-up-but-nothing-bonded fail-safe: a non-emitting outcome.
#[test]
fn an_unbonded_reply_resolves_to_unbonded() {
    let daemon: FakeDaemon = FakeDaemon::spawn("unbonded", |stream: UnixStream| {
        read_then_reply(stream, "\"Unbonded\"");
    });
    let outcome: AskOutcome = resolve(&a_request(), daemon.path(), Duration::from_millis(500));
    assert_eq!(outcome, AskOutcome::Unbonded);
}

/// The daemon receives exactly the request the hook forwarded — the round-trip carries the fields.
#[test]
fn the_daemon_receives_the_forwarded_request() {
    let (tx, rx): (_, Receiver<HookRequest>) = mpsc::channel();
    let daemon: FakeDaemon = FakeDaemon::spawn("roundtrip", move |stream: UnixStream| {
        let mut line: String = String::new();
        let mut reader: BufReader<&UnixStream> = BufReader::new(&stream);
        let _ = reader.read_line(&mut line);
        let received: HookRequest = serde_json::from_str(line.trim()).expect("a HookRequest line");
        let _ = tx.send(received);
        let mut writer: &UnixStream = &stream;
        let _ = writer.write_all(b"\"Allow\"\n");
    });
    let _ = resolve(&a_request(), daemon.path(), Duration::from_millis(500));
    let received: HookRequest = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("request arrived");
    assert_eq!(received, a_request());
}

/// A daemon that closes without answering is DaemonDown — non-emitting, never a guessed decision.
#[test]
fn a_silent_close_resolves_to_daemon_down() {
    let daemon: FakeDaemon = FakeDaemon::spawn("close", |stream: UnixStream| {
        let mut line: String = String::new();
        let mut reader: BufReader<UnixStream> = BufReader::new(stream);
        let _ = reader.read_line(&mut line);
        // drop: close the connection without replying.
    });
    let outcome: AskOutcome = resolve(&a_request(), daemon.path(), Duration::from_millis(500));
    assert_eq!(outcome, AskOutcome::DaemonDown);
}

/// A gibberish reply the daemon should never send is treated as unusable → DaemonDown.
#[test]
fn a_gibberish_reply_resolves_to_daemon_down() {
    let daemon: FakeDaemon = FakeDaemon::spawn("gibberish", |stream: UnixStream| {
        read_then_reply(stream, "not-a-reply");
    });
    let outcome: AskOutcome = resolve(&a_request(), daemon.path(), Duration::from_millis(500));
    assert_eq!(outcome, AskOutcome::DaemonDown);
}

/// A daemon that accepts but never answers rides the hook's OWN deadline out to TimedOut — the
/// core fail-safe. The handler holds the connection open well past the client deadline.
#[test]
fn a_hung_daemon_resolves_to_timed_out() {
    let daemon: FakeDaemon = FakeDaemon::spawn("hang", |stream: UnixStream| {
        let mut line: String = String::new();
        let mut reader: BufReader<&UnixStream> = BufReader::new(&stream);
        let _ = reader.read_line(&mut line);
        // Hold the link open, silent, past the client's short deadline.
        thread::sleep(Duration::from_secs(2));
    });
    let outcome: AskOutcome = resolve(&a_request(), daemon.path(), Duration::from_millis(150));
    assert_eq!(outcome, AskOutcome::TimedOut);
}

/// A missing socket (no daemon at all) is DaemonDown without any server present.
#[test]
fn a_missing_socket_resolves_to_daemon_down() {
    let sock: PathBuf = temp_sock("absent");
    let _ = std::fs::remove_file(&sock);
    let outcome: AskOutcome = resolve(&a_request(), &sock, Duration::from_millis(150));
    assert_eq!(outcome, AskOutcome::DaemonDown);
}
