#![forbid(unsafe_code)]
//! # buddy-hook
//!
//! The driving adapter behind the `buddy-hook` binary: it turns a Claude Code PreToolUse invocation
//! into a fail-safe permission decision by asking the long-lived daemon over its unix socket.
//!
//! A hook process is spawned *per tool call* and cannot own a bonded BLE connection, so it forwards
//! the tool-call context to the daemon and blocks on the reply — but only for its OWN deadline,
//! shorter than Claude Code's configured hook timeout. This is the load-bearing fail-safe: Claude
//! Code treats a hook timeout as a non-blocking error and PROCEEDS with the tool call, so if the
//! device is asleep, out of range, unbonded, or the daemon is down, this adapter must resolve to a
//! non-`Decided` [`AskOutcome`] and print NOTHING, handing control to the normal terminal prompt.
//! Degrading to the ordinary prompt is safe; silently proceeding is not.
//!
//! Every failure funnels to a non-emitting outcome:
//! - socket refused / not found / write failed → [`AskOutcome::DaemonDown`];
//! - our read deadline expired → [`AskOutcome::TimedOut`];
//! - the daemon closed without answering, or answered gibberish → [`AskOutcome::DaemonDown`];
//! - the daemon answered [`DaemonReply::Unbonded`] → [`AskOutcome::Unbonded`].
//!
//! Only a real [`DaemonReply::Allow`]/[`DaemonReply::Deny`] becomes an emit, via
//! [`buddy_permission::decide`]. No `"ask"`, no `"defer"`, ever (upstream #39344).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use buddy_permission::{decide, parse_pretooluse, to_stdout, AskOutcome, DaemonReply, HookRequest};

/// Ask the daemon over the unix socket at `sock`, resolving to the [`AskOutcome`] the fail-safe
/// [`decide`] consumes. Holds its OWN deadline via the socket read/write timeout; every failure
/// mode maps to a non-`Decided` outcome, so nothing here can ever produce a silent allow.
pub fn resolve(request: &HookRequest, sock: &Path, deadline: Duration) -> AskOutcome {
    let mut stream: UnixStream = match UnixStream::connect(sock) {
        Ok(stream) => stream,
        // Not running / socket path absent / refused — the daemon cannot answer.
        Err(_) => return AskOutcome::DaemonDown,
    };
    if stream.set_read_timeout(Some(deadline)).is_err()
        || stream.set_write_timeout(Some(deadline)).is_err()
    {
        return AskOutcome::DaemonDown;
    }
    let mut line: String = match serde_json::to_string(request) {
        Ok(line) => line,
        Err(_) => return AskOutcome::DaemonDown,
    };
    line.push('\n');
    if stream.write_all(line.as_bytes()).is_err() {
        return AskOutcome::DaemonDown;
    }
    read_reply(stream)
}

/// Read exactly one reply line within the socket's read deadline and classify it.
fn read_reply(stream: UnixStream) -> AskOutcome {
    let mut reader: BufReader<UnixStream> = BufReader::new(stream);
    let mut reply: String = String::new();
    match reader.read_line(&mut reply) {
        // EOF before any byte: the daemon closed without answering.
        Ok(0) => AskOutcome::DaemonDown,
        Ok(_) => classify_reply(reply.trim()),
        Err(err) => outcome_for_read_error(&err),
    }
}

/// A well-formed [`DaemonReply`] maps through [`DaemonReply::to_outcome`]; gibberish is treated as
/// the daemon being unusable — a non-emitting [`AskOutcome::DaemonDown`], never a guessed decision.
fn classify_reply(line: &str) -> AskOutcome {
    match serde_json::from_str::<DaemonReply>(line) {
        Ok(reply) => reply.to_outcome(),
        Err(_) => AskOutcome::DaemonDown,
    }
}

/// A read timeout (our deadline) is [`AskOutcome::TimedOut`]; any other read error means the daemon
/// is unusable. Both are non-emitting — the point is that neither can slip an approval through.
fn outcome_for_read_error(err: &std::io::Error) -> AskOutcome {
    match err.kind() {
        // SO_RCVTIMEO expiry surfaces as WouldBlock on some platforms, TimedOut on others.
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => AskOutcome::TimedOut,
        _ => AskOutcome::DaemonDown,
    }
}

/// The whole hook decision as the string to write to stdout: parse → ask → decide → render.
///
/// A payload that cannot even be parsed short-circuits to the empty string — the same safe degrade
/// as every device-unavailable outcome — without touching the socket. It never guesses a request.
pub fn hook_stdout(stdin: &[u8], sock: &Path, deadline: Duration) -> String {
    let request: HookRequest = match parse_pretooluse(stdin) {
        Ok(request) => request,
        Err(_) => return String::new(),
    };
    to_stdout(&decide(resolve(&request, sock, deadline)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A malformed payload never opens the socket and prints nothing — the safe degrade.
    #[test]
    fn a_malformed_payload_prints_nothing() {
        let sock: &Path = Path::new("/nonexistent/buddy-hook/never.sock");
        assert_eq!(
            hook_stdout(b"not json", sock, Duration::from_millis(50)),
            ""
        );
    }

    /// A valid request against a dead socket resolves DaemonDown → silent: still the safe degrade.
    #[test]
    fn a_dead_socket_prints_nothing() {
        let sock: &Path = Path::new("/nonexistent/buddy-hook/never.sock");
        let stdin: &[u8] = br#"{"session_id":"s1","tool_name":"Bash"}"#;
        assert_eq!(hook_stdout(stdin, sock, Duration::from_millis(50)), "");
    }

    /// A dead socket is classified DaemonDown (not a decision), so `decide` stays silent.
    #[test]
    fn a_dead_socket_resolves_daemon_down() {
        let sock: &Path = Path::new("/nonexistent/buddy-hook/never.sock");
        let request: HookRequest = HookRequest {
            session_id: "s1".to_string(),
            tool_name: "Bash".to_string(),
            cwd: String::new(),
            permission_mode: String::new(),
            transcript_path: String::new(),
        };
        assert_eq!(
            resolve(&request, sock, Duration::from_millis(50)),
            AskOutcome::DaemonDown
        );
    }
}
