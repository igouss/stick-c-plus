//! Shared test support: a [`Device`] whose state a test can change, a spawned
//! server that shuts down on drop, and a pure-Rust native-API client that speaks
//! the exact frames Home Assistant does — so the accept loop is driven
//! deterministically, with no aioesphomeapi dependency for the always-on suite.
#![allow(dead_code)] // each integration binary uses a different subset of these.

use std::io::{self, ErrorKind, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use prost::Message;

use esphome_api::proto::{DeviceInfoResponse, SensorStateClass, SensorStateResponse};
use esphome_api::{
    message_id, ApiEntity, Frame, FrameReader, FrameWriter, HelloRequest, HelloResponse,
    ListMessage, SensorConfig, SensorEntity, StateMessage,
};
use esphome_server::{Device, Server, ServerConfig, ServerHandle};

/// The device's initial and updated soil readings — two distinct states so a
/// broadcast is observable, matching the aioesphomeapi oracle's expectation.
pub const FIRST_READING: f32 = 42.5;
pub const SECOND_READING: f32 = 55.0;

/// A [`Device`] backed by a shared, mutable state vector so a test can publish a
/// new reading mid-connection and watch it broadcast. Identity and entity list are
/// fixed, exactly as a real device's are.
pub struct TestDevice {
    info: DeviceInfoResponse,
    list: Vec<ListMessage>,
    states: Arc<Mutex<Vec<StateMessage>>>,
}

impl TestDevice {
    /// A handle to the device's states, for a test (or a background driver thread)
    /// to publish a new reading through [`publish_reading`].
    pub fn states(&self) -> Arc<Mutex<Vec<StateMessage>>> {
        Arc::clone(&self.states)
    }
}

impl Device for TestDevice {
    fn device_info(&self) -> DeviceInfoResponse {
        self.info.clone()
    }
    fn list_messages(&self) -> Vec<ListMessage> {
        self.list.clone()
    }
    fn state_messages(&self) -> Vec<StateMessage> {
        self.states.lock().expect("states lock").clone()
    }
}

/// The plant monitor's one moisture sensor at a given reading — the entity HA
/// adopts. Its name/object_id/unit match what the aioesphomeapi oracle asserts.
pub fn moisture_sensor(reading: f32) -> SensorEntity {
    let mut sensor: SensorEntity = SensorEntity::new(SensorConfig {
        object_id: "soil_moisture".to_string(),
        name: "Soil Moisture".to_string(),
        unit_of_measurement: "%".to_string(),
        accuracy_decimals: 0,
        device_class: "moisture".to_string(),
        state_class: SensorStateClass::StateClassMeasurement,
    });
    sensor.set_state(reading);
    sensor
}

fn plantmon_info() -> DeviceInfoResponse {
    DeviceInfoResponse {
        name: "plantmon".to_string(),
        esphome_version: "rust-0.1".to_string(),
        mac_address: "AA:BB:CC:DD:EE:FF".to_string(),
        model: "M5StickC Plus".to_string(),
        ..Default::default()
    }
}

/// A device advertising one moisture sensor at [`FIRST_READING`].
pub fn one_sensor_device() -> TestDevice {
    let sensor: SensorEntity = moisture_sensor(FIRST_READING);
    TestDevice {
        info: plantmon_info(),
        list: vec![sensor.list_message()],
        states: Arc::new(Mutex::new(vec![sensor.state_message()])),
    }
}

/// A device with no entities — the zero case: it still handshakes and lists
/// nothing.
pub fn empty_device() -> TestDevice {
    TestDevice {
        info: plantmon_info(),
        list: Vec::new(),
        states: Arc::new(Mutex::new(Vec::new())),
    }
}

/// Publish a new moisture reading into a device's shared state, as the sampler
/// shell would; the server's next poll tick broadcasts it to subscribers.
pub fn publish_reading(states: &Arc<Mutex<Vec<StateMessage>>>, reading: f32) {
    *states.lock().expect("states lock") = vec![moisture_sensor(reading).state_message()];
}

/// A loopback config with fast timeouts and a host-generous stack — the default
/// for tests that are not specifically probing the cap, the reap window, or the
/// on-device stack budget (those override the relevant field).
pub fn loopback_config() -> ServerConfig {
    ServerConfig {
        bind_addr: "127.0.0.1:0".parse().expect("loopback addr"),
        max_connections: 2,
        read_timeout: Duration::from_millis(100),
        idle_timeout: Duration::from_secs(30),
        // The 12 KB device budget is probed by its own test; keep the rest robust
        // against host stack quirks so they never false-fail on an unrelated abort.
        stack_size: 256 * 1024,
    }
}

/// A server running its accept loop on a background thread, shut down and joined on
/// [`stop`](Self::stop) or on drop (so a failing test never hangs).
pub struct RunningServer {
    pub addr: SocketAddr,
    pub handle: ServerHandle,
    join: Option<JoinHandle<io::Result<()>>>,
}

impl RunningServer {
    pub fn spawn<D: Device + 'static>(config: ServerConfig, device: Arc<D>) -> RunningServer {
        let server: Server = Server::bind(config).expect("bind server");
        let addr: SocketAddr = server.local_addr();
        let handle: ServerHandle = server.handle();
        let join: JoinHandle<io::Result<()>> = thread::spawn(move || server.serve(device));
        RunningServer {
            addr,
            handle,
            join: Some(join),
        }
    }

    /// Stop the server and assert its accept loop returned without error.
    pub fn stop(mut self) {
        self.handle.shutdown();
        if let Some(join) = self.join.take() {
            join.join()
                .expect("server thread panicked")
                .expect("server accept loop errored");
        }
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.shutdown();
            let _ = join.join();
        }
    }
}

/// A pure-Rust native-API client: it frames prost messages exactly as HA's client
/// does, so a test drives a real handshake without aioesphomeapi.
pub struct Client {
    reader: FrameReader<TcpStream>,
    writer: FrameWriter<TcpStream>,
    ctl: TcpStream,
}

impl Client {
    pub fn connect(addr: SocketAddr) -> io::Result<Client> {
        let stream: TcpStream = TcpStream::connect(addr)?;
        // Never let a stuck test block forever: bound every client read.
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        Ok(Client {
            reader: FrameReader::new(stream.try_clone()?),
            writer: FrameWriter::new(stream.try_clone()?),
            ctl: stream,
        })
    }

    /// Frame a message by its prost type name and payload.
    pub fn send(&mut self, type_name: &str, payload: &[u8]) -> io::Result<()> {
        let id: u32 = message_id(type_name).unwrap_or_else(|| panic!("no wire id for {type_name}"));
        self.writer.write_frame(id, payload)
    }

    /// Frame an empty-body request (`ListEntities`, `SubscribeStates`, `Ping`, …).
    pub fn send_empty(&mut self, type_name: &str) -> io::Result<()> {
        self.send(type_name, &[])
    }

    /// Send the opening `HelloRequest`.
    pub fn hello(&mut self) -> io::Result<()> {
        let hello: HelloRequest = HelloRequest {
            client_info: "esphome-server-test".to_string(),
            api_version_major: 1,
            api_version_minor: 14,
        };
        self.send("HelloRequest", &hello.encode_to_vec())
    }

    /// Write raw bytes on the underlying stream — used to send a *partial* frame in
    /// the stalled-client test.
    pub fn send_raw(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.ctl.write_all(bytes)
    }

    pub fn recv(&mut self) -> io::Result<Option<Frame>> {
        self.reader.read_frame()
    }

    /// Read the next frame and assert it is of the expected message type,
    /// returning it for further decoding.
    pub fn expect(&mut self, type_name: &str) -> Frame {
        let want: u32 = message_id(type_name).unwrap();
        let frame: Frame = self
            .recv()
            .expect("read frame")
            .expect("a frame, not a closed stream");
        assert_eq!(
            frame.msg_type, want,
            "expected {type_name} (id {want}), got id {}",
            frame.msg_type
        );
        frame
    }

    /// Hello + read `HelloResponse`; returns the server's device name.
    pub fn handshake_hello(&mut self) -> String {
        self.hello().expect("send hello");
        let frame: Frame = self.expect("HelloResponse");
        HelloResponse::decode(frame.payload.as_slice())
            .expect("decode HelloResponse")
            .name
    }

    /// Subscribe and read the one initial `SensorStateResponse`; returns its value.
    /// (Assumes the one-sensor device.)
    pub fn subscribe_one(&mut self) -> f32 {
        self.send_empty("SubscribeStatesRequest")
            .expect("subscribe");
        sensor_value(&self.expect("SensorStateResponse"))
    }

    /// Assert the server has closed this connection (a clean FIN or an RST),
    /// distinguishing that from a still-open connection (which would time out and
    /// must fail the test, not pass it).
    pub fn assert_closed_by_server(&mut self) {
        match self.recv() {
            Ok(None) => {}                                             // clean FIN on a boundary
            Err(ref e) if e.kind() == ErrorKind::ConnectionReset => {} // RST
            Ok(Some(frame)) => panic!(
                "expected the server to close the connection, got a frame (id {})",
                frame.msg_type
            ),
            Err(e) => {
                panic!("expected a closed connection (EOF/RST), got a live-but-idle stream: {e:?}")
            }
        }
    }
}

/// Decode a `SensorStateResponse` frame's value.
pub fn sensor_value(frame: &Frame) -> f32 {
    SensorStateResponse::decode(frame.payload.as_slice())
        .expect("decode SensorStateResponse")
        .state
}

/// Poll `cond` until it holds or `timeout` elapses; returns whether it held.
pub fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let start: Instant = Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    cond()
}
