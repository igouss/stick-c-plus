//! Emit the ESP-IDF link flags, and bake in the hostpulse endpoint and token.
//!
//! Two jobs:
//!
//! 1. `embuild::espidf::sysenv::output()` records the ESP-IDF link/search flags so ldproxy
//!    forwards them to the final link — the same job every firmware bin's build script does.
//! 2. Read the `[host_monitor]` table of the git-ignored `firmware/secrets.toml` and emit its
//!    `endpoint` (`host:port`) as `HOST_MONITOR_ENDPOINT` and its `token` (the bearer secret)
//!    as `HOST_MONITOR_TOKEN`, both rustc env `main.rs` picks up via `env!`. The endpoint is
//!    deployment config; the token is a secret — but both ride the same road as the WiFi
//!    credentials, git-ignored and baked into the image, so nothing device-specific is
//!    committed. A missing table fails the build loudly rather than shipping an image that
//!    fetches nowhere or without its bearer.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The `[host_monitor]` table of `firmware/secrets.toml`.
#[derive(Deserialize)]
struct Secrets {
    host_monitor: HostMonitor,
}

/// The hostpulse endpoint and its bearer token. Field names mirror `secrets.toml.example`.
#[derive(Deserialize)]
struct HostMonitor {
    /// `host:port` of the hostpulse endpoint, e.g. `"10.0.0.10:9099"`.
    endpoint: String,
    /// The 64-hex bearer token the endpoint requires. Secret — never committed.
    token: String,
}

fn main() {
    // 1) ESP-IDF link/search flags for the final link.
    embuild::espidf::sysenv::output();

    // 2) Bake in the endpoint and token from the git-ignored secrets file.
    let manifest_dir: String =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is always set by cargo");
    let firmware_root: &Path = Path::new(&manifest_dir)
        .ancestors()
        .find(|dir: &&Path| dir.join("secrets.toml.example").exists())
        .expect("firmware workspace root (with secrets.toml.example) not found above this crate");
    let secrets_path: PathBuf = firmware_root.join("secrets.toml");

    println!("cargo:rerun-if-changed={}", secrets_path.display());
    println!("cargo:rerun-if-changed=build.rs");

    let raw: String = std::fs::read_to_string(&secrets_path).unwrap_or_else(|err| {
        panic!(
            "cannot read {}: {err}\n\
             The hostpulse endpoint and token come from a git-ignored secrets file. Create it:\n    \
             cp firmware/secrets.toml.example firmware/secrets.toml\n\
             then fill in the [host_monitor] endpoint and token (see secrets.toml.example; the \
             token is in the Bitwarden observability-secrets vault).",
            secrets_path.display()
        )
    });

    let secrets: Secrets = toml::from_str(&raw).unwrap_or_else(|err| {
        panic!(
            "{} is malformed: {err}\nexpected a [host_monitor] table with an `endpoint` and a \
             `token` (see firmware/secrets.toml.example).",
            secrets_path.display()
        )
    });

    // cargo escapes the values; a host:port or a hex token passes through intact. These are
    // read back by `main.rs` via `env!`, so the token reaches the image without ever touching
    // a tracked source file.
    println!(
        "cargo:rustc-env=HOST_MONITOR_ENDPOINT={}",
        secrets.host_monitor.endpoint
    );
    println!(
        "cargo:rustc-env=HOST_MONITOR_TOKEN={}",
        secrets.host_monitor.token
    );
}
