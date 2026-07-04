//! Conformance oracle: the **real** Home Assistant client (aioesphomeapi) adopts a
//! device served by the full accept loop — not the minimal per-connection loop the
//! `esphome-api` oracle uses, but this crate's `Server`, with its cap, timeouts and
//! per-connection thread.
//!
//! The always-on `server_host` suite proves the loop against a wire-identical
//! pure-Rust client; this proves that wire is the one the client HA actually ships
//! speaks — connect, device info, list entities, subscribe, and observe live
//! updates. The Python subprocess is the same value-asserting oracle the FSM bead
//! uses (`esphome-api/tests/oracle/ha_client_oracle.py`); a background driver
//! toggles the reading so the client sees two distinct states however its
//! subscribe races the device.
//!
//! `#[ignore]` by default: aioesphomeapi is not a Cargo dependency, so a plain
//! `cargo test` shows it *ignored*, never a false green. Run it against a Python
//! that has aioesphomeapi:
//!
//! ```sh
//! ESPHOME_ORACLE_PYTHON=/path/to/venv/bin/python \
//!   cargo test -p esphome-server --test aioesphomeapi_oracle -- --ignored --nocapture
//! ```

mod common;

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use common::{
    loopback_config, one_sensor_device, publish_reading, RunningServer, FIRST_READING,
    SECOND_READING,
};

/// The shared value-asserting oracle, owned by the FSM bead's crate.
fn oracle_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../esphome-api/tests/oracle/ha_client_oracle.py")
}

#[test]
#[ignore = "requires aioesphomeapi; set ESPHOME_ORACLE_PYTHON and run with --ignored"]
fn the_real_ha_client_adopts_over_the_server_host() {
    let python: String = std::env::var("ESPHOME_ORACLE_PYTHON")
        .expect("set ESPHOME_ORACLE_PYTHON to a python interpreter that has aioesphomeapi");

    let device = Arc::new(one_sensor_device());
    let states = device.states();
    let server: RunningServer = RunningServer::spawn(loopback_config(), device);
    let port: u16 = server.addr.port();

    // Toggle the reading so the client observes >=2 distinct states no matter when
    // its subscribe lands; the server broadcasts each change on its poll tick.
    let driving: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
    let driver: JoinHandle<()> = {
        let driving: Arc<AtomicBool> = Arc::clone(&driving);
        thread::spawn(move || {
            let mut high: bool = false;
            while driving.load(Ordering::SeqCst) {
                publish_reading(&states, if high { SECOND_READING } else { FIRST_READING });
                high = !high;
                thread::sleep(Duration::from_millis(400));
            }
        })
    };

    let output: Output = Command::new(&python)
        .arg(oracle_script())
        .arg(port.to_string())
        .output()
        .expect("spawn the aioesphomeapi oracle");

    driving.store(false, Ordering::SeqCst);
    let _ = driver.join();

    let stdout: String = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr: String = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        output.status.success(),
        "aioesphomeapi oracle failed (exit {:?})\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        output.status.code()
    );

    // Re-check the JSON verdict so a silently-wrong summary cannot pass.
    let summary: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("oracle stdout was not JSON ({e}): {stdout:?}"));
    assert_eq!(summary["device_name"], "plantmon");
    assert_eq!(summary["entity_count"], 1);
    assert_eq!(summary["object_id"], "soil_moisture");
    assert!(
        summary["distinct_states"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0)
            >= 2,
        "expected >=2 distinct states in {summary}"
    );

    server.stop();
}
