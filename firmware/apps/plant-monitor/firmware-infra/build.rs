//! Bake the WiFi credentials into the image as compile-time env.
//!
//! The credentials MUST NOT be committed (qhw.7): they live only in the
//! git-ignored `firmware/secrets.toml`, mirrored in Bitwarden. This script reads
//! that file at build time and emits `WIFI_SSID` / `WIFI_PASSWORD` as rustc env,
//! which `wifi.rs` picks up via `env!` — so the secrets reach the firmware
//! without ever touching a tracked source file. Change the secrets file and cargo
//! rebuilds (the `rerun-if-changed` below); nothing else depends on it.
//!
//! Missing or malformed secrets fail the build loudly rather than silently
//! producing an image that can never join the network.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The `[wifi]` table of `firmware/secrets.toml`.
#[derive(Deserialize)]
struct Secrets {
    wifi: Wifi,
}

/// The 2.4 GHz station credentials. Field names mirror `secrets.toml.example`.
#[derive(Deserialize)]
struct Wifi {
    ssid: String,
    password: String,
}

fn main() {
    // `secrets.toml` sits at the firmware workspace root, alongside the committed
    // `secrets.toml.example`. Walk up from this crate to find that root, so the crate can sit
    // at any depth under firmware/ (it moved to apps/plant-monitor/ in the reorg) without
    // hardcoding the number of `..`.
    let manifest_dir: String =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is always set by cargo");
    let firmware_root: &Path = Path::new(&manifest_dir)
        .ancestors()
        .find(|dir: &&Path| dir.join("secrets.toml.example").exists())
        .expect("firmware workspace root (with secrets.toml.example) not found above this crate");
    let secrets_path: PathBuf = firmware_root.join("secrets.toml");

    // Re-run only when the inputs change: the secrets file or this script itself.
    println!("cargo:rerun-if-changed={}", secrets_path.display());
    println!("cargo:rerun-if-changed=build.rs");

    let raw: String = std::fs::read_to_string(&secrets_path).unwrap_or_else(|err| {
        panic!(
            "cannot read {}: {err}\n\
             WiFi credentials come from a git-ignored secrets file. Create it:\n    \
             cp firmware/secrets.toml.example firmware/secrets.toml\n\
             then fill in the real SSID/password (see secrets.toml.example; values in the \
             Bitwarden homelab vault).",
            secrets_path.display()
        )
    });

    let secrets: Secrets = toml::from_str(&raw).unwrap_or_else(|err| {
        panic!(
            "{} is malformed: {err}\nexpected a [wifi] table with `ssid` and `password` \
             (see firmware/secrets.toml.example).",
            secrets_path.display()
        )
    });

    // Bake the credentials in for `wifi.rs` to read via `env!`. cargo escapes the
    // values; a password with spaces or symbols passes through intact.
    println!("cargo:rustc-env=WIFI_SSID={}", secrets.wifi.ssid);
    println!("cargo:rustc-env=WIFI_PASSWORD={}", secrets.wifi.password);
}
