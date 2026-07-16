//! Emit the ESP-IDF link flags, and bake in the host-monitor's target address.
//!
//! Two jobs:
//!
//! 1. `embuild::espidf::sysenv::output()` records the ESP-IDF link/search flags so
//!    ldproxy forwards them to the final link — the same job every firmware bin's
//!    build script does.
//! 2. Read the `[host_monitor]` table of the git-ignored `firmware/secrets.toml` and
//!    emit its `address` (`host:port`) as `HOST_MONITOR_ADDRESS` rustc env, which
//!    `main.rs` picks up via `env!`. The exporter's address is deployment config, not a
//!    secret, but it rides the same road as the WiFi credentials so nothing device-
//!    specific is committed. A missing table fails the build loudly rather than shipping
//!    an image that scrapes nowhere.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The `[host_monitor]` table of `firmware/secrets.toml`.
#[derive(Deserialize)]
struct Secrets {
    host_monitor: HostMonitor,
}

/// The exporter endpoint. Field name mirrors `secrets.toml.example`.
#[derive(Deserialize)]
struct HostMonitor {
    /// `host:port` of the node_exporter to scrape, e.g. `"192.168.1.10:9100"`.
    address: String,
}

fn main() {
    // 1) ESP-IDF link/search flags for the final link.
    embuild::espidf::sysenv::output();

    // 2) Bake in the exporter address from the git-ignored secrets file.
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
             The host-monitor address comes from a git-ignored secrets file. Create it:\n    \
             cp firmware/secrets.toml.example firmware/secrets.toml\n\
             then fill in the [host_monitor] address (see secrets.toml.example).",
            secrets_path.display()
        )
    });

    let secrets: Secrets = toml::from_str(&raw).unwrap_or_else(|err| {
        panic!(
            "{} is malformed: {err}\nexpected a [host_monitor] table with an `address` \
             (see firmware/secrets.toml.example).",
            secrets_path.display()
        )
    });

    // cargo escapes the value; a host:port with unusual characters passes through intact.
    println!(
        "cargo:rustc-env=HOST_MONITOR_ADDRESS={}",
        secrets.host_monitor.address
    );
}
