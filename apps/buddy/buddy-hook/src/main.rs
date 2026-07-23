#![forbid(unsafe_code)]
//! The `buddy-hook` binary — the thin composition root for the PreToolUse hook.
//!
//! It owns only the effects the adapter cannot: reading stdin, resolving the socket path and the
//! deadline from the environment, and writing stdout. Every decision lives in [`buddy_hook`] and
//! [`buddy_permission`]. It ALWAYS exits 0: printing nothing on any failure is the fail-safe that
//! hands control to Claude Code's normal permission prompt.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use buddy_permission::{DEFAULT_SOCK_FILENAME, SOCK_ENV};

/// Environment variable overriding the hook's own deadline, in milliseconds. Keep it comfortably
/// shorter than the `timeout` configured for this hook in settings.json.
const DEADLINE_ENV: &str = "BUDDY_HOOK_DEADLINE_MS";
/// The default deadline: 25 s, under the 30 s `timeout` the kb guide wires in settings.json.
const DEFAULT_DEADLINE_MS: u64 = 25_000;
/// The per-user runtime directory the socket lives under when [`SOCK_ENV`] is unset.
const RUNTIME_DIR_ENV: &str = "XDG_RUNTIME_DIR";

/// Resolve the daemon socket: an explicit [`SOCK_ENV`] wins; otherwise the default filename under
/// `$XDG_RUNTIME_DIR`, falling back to the system temp dir when that is unset.
fn socket_path() -> PathBuf {
    if let Ok(explicit) = std::env::var(SOCK_ENV) {
        return PathBuf::from(explicit);
    }
    let dir: PathBuf = std::env::var(RUNTIME_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    dir.join(DEFAULT_SOCK_FILENAME)
}

/// Resolve the hook's own deadline from [`DEADLINE_ENV`], defaulting to [`DEFAULT_DEADLINE_MS`].
fn deadline() -> Duration {
    let ms: u64 = std::env::var(DEADLINE_ENV)
        .ok()
        .and_then(|value: String| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_DEADLINE_MS);
    Duration::from_millis(ms)
}

fn main() {
    let mut stdin: Vec<u8> = Vec::new();
    // Cannot even read the payload → safe degrade: print nothing, exit 0.
    if std::io::stdin().read_to_end(&mut stdin).is_err() {
        return;
    }
    let out: String = buddy_hook::hook_stdout(&stdin, &socket_path(), deadline());
    if !out.is_empty() {
        let _ = std::io::stdout().write_all(out.as_bytes());
    }
}
