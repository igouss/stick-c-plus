//! Conformance oracle: the **real** Home Assistant client (aioesphomeapi) adopts
//! the **real** device — [`esphome_api::SensorDevice`], the production one-sensor
//! `Device` fed by a live source — served by this crate's full accept loop, with
//! its cap, timeouts, and per-connection thread.
//!
//! This is the host-first half of qhw.9: before the board exists, the exact device
//! the plant monitor serves is driven by the client HA actually ships. The oracle
//! asserts VALUES — device name and MAC, the entity's full descriptor (name, unit,
//! device_class, state_class, accuracy_decimals), and that at least two DISTINCT
//! states are observed — so a green means HA truly adopts and reads this device,
//! not merely that the sockets connected. The one difference from production is the
//! source: here a background thread toggles it between two readings (standing in
//! for the sampler shell publishing into the shared cache) so the client observes a
//! second, distinct state however its subscribe races the device.
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

use esphome_api::SensorDevice;
use esphome_server::Device;

use common::{
    loopback_config, moisture_config, plantmon_info, RunningServer, FIRST_READING, SECOND_READING,
};

/// The shared value-asserting oracle, owned by the FSM bead's crate.
fn oracle_script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../esphome-api/tests/oracle/ha_client_oracle.py")
}

#[test]
#[ignore = "requires aioesphomeapi; set ESPHOME_ORACLE_PYTHON and run with --ignored"]
fn the_real_ha_client_adopts_the_sensor_device_over_the_server_host() {
    let python: String = std::env::var("ESPHOME_ORACLE_PYTHON")
        .expect("set ESPHOME_ORACLE_PYTHON to a python interpreter that has aioesphomeapi");

    // The production device: identity + the moisture descriptor + a live source. A
    // shared flag stands in for the sampler's freshest reading; the source pulls it
    // on every poll, exactly as it will pull `SharedMoisture::latest` on-device.
    let high: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let source = {
        let high: Arc<AtomicBool> = Arc::clone(&high);
        move || {
            Some(if high.load(Ordering::SeqCst) {
                SECOND_READING
            } else {
                FIRST_READING
            })
        }
    };
    let device: Arc<SensorDevice<_>> = Arc::new(SensorDevice::new(
        plantmon_info(),
        moisture_config(),
        source,
    ));

    // Sanity: the device really is the one the oracle will assert against.
    assert_eq!(device.list_messages().len(), 1);

    let server: RunningServer = RunningServer::spawn(loopback_config(), device);
    let port: u16 = server.addr.port();

    // Toggle the reading so the client observes >=2 distinct states no matter when
    // its subscribe lands; the server broadcasts each change on its poll tick.
    let driving: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
    let driver: JoinHandle<()> = {
        let driving: Arc<AtomicBool> = Arc::clone(&driving);
        let high: Arc<AtomicBool> = Arc::clone(&high);
        thread::spawn(move || {
            while driving.load(Ordering::SeqCst) {
                high.fetch_xor(true, Ordering::SeqCst);
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
    assert_eq!(
        summary["mac_address"]
            .as_str()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("aa:bb:cc:dd:ee:ff")
    );
    assert_eq!(summary["entity_count"], 1);
    assert_eq!(summary["object_id"], "soil_moisture");
    assert_eq!(summary["entity_name"], "Soil Moisture");
    assert_eq!(summary["unit_of_measurement"], "%");
    assert_eq!(summary["device_class"], "moisture");
    assert_eq!(summary["accuracy_decimals"], 0);
    assert_eq!(summary["state_class"], 1); // SensorStateClass::StateClassMeasurement
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
