//! Conformance oracle: the pure connection FSM, wrapped in a minimal blocking
//! device, driven by the **real** Home Assistant client (aioesphomeapi).
//!
//! The value-tests in `src/connection.rs` prove the FSM against our own
//! understanding of the protocol; this proves that understanding matches the
//! client HA actually ships. A Python subprocess (`tests/oracle/
//! ha_client_oracle.py`) connects over loopback, fetches device info, lists
//! entities, subscribes, and asserts the values it observes — device name, the
//! exact entity, and two DISTINCT sensor readings — then reports a JSON verdict.
//!
//! `#[ignore]` by default: aioesphomeapi is not a Cargo dependency, so a plain
//! `cargo test` neither builds nor fakes it — the test shows as *ignored*, never
//! a false green. Run it against a Python that has aioesphomeapi:
//!
//! ```sh
//! ESPHOME_ORACLE_PYTHON=/path/to/venv/bin/python \
//!   cargo test -p esphome-api --test aioesphomeapi_oracle -- --ignored --nocapture
//! ```

use std::io;
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::thread;

use esphome_api::proto::{DeviceInfoResponse, SensorStateClass};
use esphome_api::{
    broadcast_states, handle, ApiEntity, Connection, FrameReader, FrameWriter, Inbound, Outbound,
    SensorConfig, SensorEntity, Snapshot, StateMessage,
};

/// The initial and updated readings the oracle must observe as two distinct
/// states.
const FIRST_READING: f32 = 42.5;
const SECOND_READING: f32 = 55.0;

/// Serve exactly one native-API connection off `stream`, driving the FSM until
/// the client disconnects. After the client subscribes, push a second, distinct
/// reading so the oracle can confirm live updates flow — not just the initial
/// state.
fn serve_device(stream: TcpStream) -> io::Result<()> {
    let mut reader: FrameReader<TcpStream> = FrameReader::new(stream.try_clone()?);
    let mut writer: FrameWriter<TcpStream> = FrameWriter::new(stream);

    // The plant monitor's one sensor, built through the real entity model so its
    // List/State messages are what HA actually receives.
    let mut sensor: SensorEntity = SensorEntity::new(SensorConfig {
        object_id: "soil_moisture".to_string(),
        name: "Soil Moisture".to_string(),
        unit_of_measurement: "%".to_string(),
        accuracy_decimals: 0,
        device_class: "moisture".to_string(),
        state_class: SensorStateClass::StateClassMeasurement,
    });
    sensor.set_state(FIRST_READING);

    let device_info: DeviceInfoResponse = DeviceInfoResponse {
        name: "plantmon".to_string(),
        esphome_version: "rust-0.1".to_string(),
        mac_address: "AA:BB:CC:DD:EE:FF".to_string(),
        model: "M5StickC Plus".to_string(),
        ..Default::default()
    };

    let list: Vec<esphome_api::ListMessage> = vec![sensor.list_message()];
    let mut states: Vec<StateMessage> = vec![sensor.state_message()];
    let mut conn: Connection = Connection::new();

    while let Some(frame) = reader.read_frame()? {
        let inbound: Inbound = Inbound::from_frame(&frame)
            .map_err(|e: prost::DecodeError| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Scope the snapshot so `states` can be reassigned after handling.
        let (next, outbound): (Connection, Vec<Outbound>) = {
            let snapshot: Snapshot = Snapshot {
                device_info: &device_info,
                list: &list,
                states: &states,
            };
            handle(conn, &inbound, &snapshot)
        };
        conn = next;
        write_all(&mut writer, &outbound)?;

        // Right after the initial subscription stream, publish a changed reading
        // so the client observes a second, distinct state.
        if matches!(inbound, Inbound::SubscribeStates(_)) {
            sensor.set_state(SECOND_READING);
            states = vec![sensor.state_message()];
            let updates: Vec<Outbound> = broadcast_states(&conn, &states);
            write_all(&mut writer, &updates)?;
        }

        if conn.is_closed() {
            break;
        }
    }
    Ok(())
}

fn write_all(writer: &mut FrameWriter<TcpStream>, messages: &[Outbound]) -> io::Result<()> {
    for message in messages {
        let (msg_type, payload): (u32, Vec<u8>) = message.wire();
        writer.write_frame(msg_type, &payload)?;
    }
    Ok(())
}

#[test]
#[ignore = "requires aioesphomeapi; set ESPHOME_ORACLE_PYTHON and run with --ignored"]
fn the_real_ha_client_adopts_and_reads_the_device() {
    let python: String = std::env::var("ESPHOME_ORACLE_PYTHON")
        .expect("set ESPHOME_ORACLE_PYTHON to a python interpreter that has aioesphomeapi");

    let listener: TcpListener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
    let port: u16 = listener.local_addr().unwrap().port();

    // The device runs on its own thread; the accept blocks until the client
    // connects.
    let server: thread::JoinHandle<io::Result<()>> = thread::spawn(move || {
        let (stream, _addr) = listener.accept()?;
        serve_device(stream)
    });

    let script: String = format!(
        "{}/tests/oracle/ha_client_oracle.py",
        env!("CARGO_MANIFEST_DIR")
    );
    let output: std::process::Output = Command::new(&python)
        .arg(&script)
        .arg(port.to_string())
        .output()
        .expect("spawn the aioesphomeapi oracle");

    let stdout: String = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr: String = String::from_utf8_lossy(&output.stderr).into_owned();

    let server_result: io::Result<()> = server.join().expect("device thread must not panic");

    assert!(
        output.status.success(),
        "aioesphomeapi oracle failed (exit {:?})\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        output.status.code()
    );
    server_result.expect("device served the connection without an I/O error");

    // The Python side already asserted the values; re-check its summary so a
    // silently-wrong summary can't pass.
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
}
