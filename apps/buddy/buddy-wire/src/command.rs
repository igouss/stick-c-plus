//! The device command set, and its exhaustive dispatch to an ack.
//!
//! [`Command`] is the `{"cmd":..}` vocabulary. [`dispatch`] is **exhaustive** with an explicit
//! negative-ack default arm, and [`Ack`] is the reply shape.
//!
//! ## Defect fix (a / D3): an unknown command gets a nack, never a swallow
//!
//! Upstream swallowed unknown commands (one path returned `true` with no ack, another let the
//! snapshot merge clobber the prompt). Here [`Command::Unknown`] is a real variant and
//! [`dispatch`] answers it with `ok = false` — every command is acked.
//!
//! ## D7: `entries` are oldest-first
//!
//! The transcript `entries` are ordered **oldest-first** (index 0 is the oldest). Upstream's
//! doc said newest-first, but its de-dup only works oldest-first, and we write the bridge — so
//! oldest-first is the contract, stated here.
//!
//! ## Out of scope: `file.path` traversal (defect e)
//!
//! Folder push is dropped from this port, so [`Command::File`] carries the path but no handler
//! acts on it. **If path handling is ever added here**, it must reject `..` and absolute paths
//! (the upstream `file.path` was unvalidated despite the spec asking for it) — this is the
//! module where that guard belongs.

use serde::{Deserialize, Serialize};

/// A device command — the `{"cmd":..}` vocabulary.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
pub enum Command {
    /// Set the buddy's name.
    Name(String),
    /// Select a species by registry index; `0xFF` means "use the installed GIF".
    Species(u8),
    /// Forget the current BLE bond.
    Unpair,
    /// Set the owner label.
    Owner(String),
    /// Request a status ack.
    Status,
    /// Begin a character/GIF upload.
    CharBegin,
    /// A file header — carries the path (see the traversal note in the module docs).
    File {
        /// The declared file path (unvalidated; folder push is out of scope).
        path: String,
    },
    /// A file data chunk.
    Chunk,
    /// End the current file.
    FileEnd,
    /// End a character/GIF upload.
    CharEnd,
    /// Any command not in the set above — answered with a negative ack, never swallowed.
    Unknown(String),
}

/// A command acknowledgement — `{"ack":<what>,"ok":<bool>,"n":<u32>}`.
///
/// `n` is **always** present (contrary to the upstream doc's examples): for a chunk it is the
/// per-file byte count, reset at each `file`; for other commands it carries the relevant count
/// or `0`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ack {
    /// What is being acked (the `cmd` name).
    pub what: String,
    /// Whether the command succeeded.
    pub ok: bool,
    /// The always-present count.
    pub n: u32,
}

/// The on-wire shape of an ack, with the field order fixed at `ack`, `ok`, `n`.
#[derive(Serialize)]
struct AckWire<'a> {
    ack: &'a str,
    ok: bool,
    n: u32,
}

impl Ack {
    /// Serialize to the wire ack line `{"ack":<what>,"ok":<bool>,"n":<u32>}` (no trailing
    /// newline; the transport adds framing).
    pub fn to_json(&self) -> String {
        let wire: AckWire<'_> = AckWire {
            ack: &self.what,
            ok: self.ok,
            n: self.n,
        };
        serde_json::to_string(&wire).expect("an ack always serializes")
    }
}

/// Dispatch a command to its ack — **exhaustive**, with [`Command::Unknown`] answered by an
/// explicit negative-ack default arm (defect fix a/D3).
///
/// `n` is always `0` here: this is the pure protocol dispatch, with no filesystem or transfer
/// state to count against. The firmware adapter that owns the byte counters (per-file bytes for
/// a chunk, etc.) sets `n` accordingly; the contract is only that `n` is always present.
pub fn dispatch(command: &Command) -> Ack {
    // Exhaustive: `what` is the cmd name, `ok` is true for every known command and false for
    // the Unknown default arm — every command is acked, none is swallowed.
    let (what, ok): (&str, bool) = match command {
        Command::Name(_) => ("name", true),
        Command::Species(_) => ("species", true),
        Command::Unpair => ("unpair", true),
        Command::Owner(_) => ("owner", true),
        Command::Status => ("status", true),
        Command::CharBegin => ("char_begin", true),
        Command::File { .. } => ("file", true),
        Command::Chunk => ("chunk", true),
        Command::FileEnd => ("file_end", true),
        Command::CharEnd => ("char_end", true),
        Command::Unknown(name) => (name.as_str(), false),
    };
    Ack {
        what: what.to_string(),
        ok,
        n: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ---- Ack wire shape ------------------------------------------------------------------

    #[test]
    fn an_ack_serializes_with_ack_ok_and_n_in_order() {
        let ack: Ack = Ack {
            what: "name".to_string(),
            ok: true,
            n: 0,
        };
        assert_eq!(ack.to_json(), r#"{"ack":"name","ok":true,"n":0}"#);
    }

    #[test]
    fn n_is_always_present_even_when_zero() {
        let ack: Ack = Ack {
            what: "status".to_string(),
            ok: true,
            n: 0,
        };
        assert!(ack.to_json().contains(r#""n":0"#), "n is always emitted");
    }

    // ---- Dispatch: every known command is acked ok --------------------------------------

    #[test]
    fn a_known_command_is_acked_ok_with_its_name() {
        let ack: Ack = dispatch(&Command::Status);
        assert_eq!(ack.what, "status");
        assert!(ack.ok);
    }

    #[test]
    fn char_begin_acks_with_the_snake_case_wire_name() {
        let ack: Ack = dispatch(&Command::CharBegin);
        assert_eq!(ack.what, "char_begin");
        assert!(ack.ok);
    }

    #[test]
    fn a_file_command_acks_file_regardless_of_path() {
        let ack: Ack = dispatch(&Command::File {
            path: "sprite.pix".to_string(),
        });
        assert_eq!(ack.what, "file");
        assert!(ack.ok);
    }

    // ---- Defect fix (a / D3): an unknown command gets a nack, never a swallow ------------

    #[test]
    fn an_unknown_command_is_nacked_not_swallowed() {
        // The pin: upstream returned true (swallow) or let the merge clobber the prompt. Here
        // the Unknown default arm answers ok=false — the command is acked, never dropped.
        let ack: Ack = dispatch(&Command::Unknown("frobnicate".to_string()));
        assert_eq!(ack.what, "frobnicate");
        assert!(!ack.ok, "an unknown command must be nacked");
    }

    #[test]
    fn an_unknown_command_serializes_a_negative_ack() {
        let ack: Ack = dispatch(&Command::Unknown("frobnicate".to_string()));
        assert_eq!(ack.to_json(), r#"{"ack":"frobnicate","ok":false,"n":0}"#);
    }

    // ---- Properties ----------------------------------------------------------------------

    proptest! {
        // Every dispatched ack round-trips through JSON with n always present.
        #[test]
        fn a_dispatched_ack_always_carries_n(name in "[a-z_]{1,12}") {
            let ack: Ack = dispatch(&Command::Unknown(name.clone()));
            let json: String = ack.to_json();
            prop_assert!(json.contains(r#""n":0"#));
            prop_assert!(json.contains(r#""ok":false"#));
        }

        // A known command (Name) is always acked ok, whatever the payload string.
        #[test]
        fn a_name_command_is_always_ok(value in ".*") {
            let ack: Ack = dispatch(&Command::Name(value));
            prop_assert_eq!(ack.what, "name".to_string());
            prop_assert!(ack.ok);
        }
    }
}
