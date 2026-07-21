//! The always-on server-host suite: a pure-Rust loopback client drives the accept
//! loop through every guarantee — handshake, device-driven broadcast, the
//! concurrency cap and its slot reclaim, stalled-client reaping, and no fd leak
//! over many cycles. No aioesphomeapi here, so `cargo test` runs all of it; the
//! real-client conformance lives in the `#[ignore]`d `aioesphomeapi_oracle`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{
    empty_device, loopback_config, one_sensor_device, publish_reading, sensor_value, wait_until,
    Client, RunningServer, FIRST_READING, SECOND_READING,
};
use esphome_api::{encode_frame, message_id};
use esphome_server::{ServerConfig, PLAINTEXT_STACK_SIZE};

/// A full adoption handshake: Hello, DeviceInfo, ListEntities (+Done),
/// SubscribeStates — the exact conversation HA has on adoption — is answered with
/// the device's real identity, entity, and reading.
#[test]
fn serves_a_full_adoption_handshake() {
    let server: RunningServer =
        RunningServer::spawn(loopback_config(), Arc::new(one_sensor_device()));
    let mut client: Client = Client::connect(server.addr).expect("connect");

    assert_eq!(
        client.handshake_hello(),
        "plantmon",
        "device name in HelloResponse"
    );

    client.send_empty("DeviceInfoRequest").expect("device info");
    client.expect("DeviceInfoResponse");

    client
        .send_empty("ListEntitiesRequest")
        .expect("list entities");
    client.expect("ListEntitiesSensorResponse");
    client.expect("ListEntitiesDoneResponse");

    assert_eq!(
        client.subscribe_one(),
        FIRST_READING,
        "initial subscribed state"
    );

    server.stop();
}

/// The zero case: a device with no entities still handshakes, and `ListEntities`
/// is just the terminating `Done` with no entity in between.
#[test]
fn an_empty_device_handshakes_and_lists_nothing() {
    let server: RunningServer = RunningServer::spawn(loopback_config(), Arc::new(empty_device()));
    let mut client: Client = Client::connect(server.addr).expect("connect");

    assert_eq!(client.handshake_hello(), "plantmon");
    client
        .send_empty("ListEntitiesRequest")
        .expect("list entities");
    client.expect("ListEntitiesDoneResponse"); // no ListEntitiesSensorResponse before it

    server.stop();
}

/// A reading published after the client subscribes is pushed to it — the
/// device-driven update path the pure FSM cannot cover on its own.
#[test]
fn broadcasts_a_changed_reading_to_a_subscriber() {
    let device = Arc::new(one_sensor_device());
    let states = device.states();
    let server: RunningServer = RunningServer::spawn(loopback_config(), device);
    let mut client: Client = Client::connect(server.addr).expect("connect");

    client.handshake_hello();
    assert_eq!(
        client.subscribe_one(),
        FIRST_READING,
        "initial state streamed"
    );

    publish_reading(&states, SECOND_READING);
    let update = client.expect("SensorStateResponse");
    assert_eq!(
        sensor_value(&update),
        SECOND_READING,
        "the new reading was pushed"
    );

    server.stop();
}

/// An unknown message after the handshake is ignored, and the connection keeps
/// serving — one odd frame from a rich client must not drop the device.
#[test]
fn an_unknown_message_does_not_break_the_connection() {
    let server: RunningServer =
        RunningServer::spawn(loopback_config(), Arc::new(one_sensor_device()));
    let mut client: Client = Client::connect(server.addr).expect("connect");

    client.handshake_hello();
    // A well-formed frame the device does not act on (SubscribeLogs, id 28).
    client
        .send_empty("SubscribeLogsRequest")
        .expect("send unknown");
    // The connection survives: a following request is still answered.
    client.send_empty("PingRequest").expect("ping");
    client.expect("PingResponse");

    server.stop();
}

/// Two connections serve at once under a cap of two (the many case); the third is
/// closed at once while the first two keep serving.
#[test]
fn serves_up_to_the_cap_and_rejects_beyond_it() {
    let config: ServerConfig = ServerConfig {
        max_connections: 2,
        ..loopback_config()
    };
    let server: RunningServer = RunningServer::spawn(config, Arc::new(one_sensor_device()));

    let mut a: Client = Client::connect(server.addr).expect("connect A");
    let mut b: Client = Client::connect(server.addr).expect("connect B");
    a.handshake_hello();
    b.handshake_hello();

    // Both slots taken: a third connection is rejected.
    assert!(
        wait_until(Duration::from_secs(2), || server
            .handle
            .active_connections()
            == 2),
        "both connections should be active"
    );
    let mut c: Client = Client::connect(server.addr).expect("connect C");
    c.assert_closed_by_server();

    // A and B are unharmed: each still answers a ping.
    a.send_empty("PingRequest").expect("ping A");
    a.expect("PingResponse");
    b.send_empty("PingRequest").expect("ping B");
    b.expect("PingResponse");

    server.stop();
}

/// When a served connection ends, its slot is reclaimed and a later connection
/// succeeds — the cap is a live count, not a high-water mark.
#[test]
fn reclaims_a_slot_when_a_connection_ends() {
    let config: ServerConfig = ServerConfig {
        max_connections: 1,
        ..loopback_config()
    };
    let server: RunningServer = RunningServer::spawn(config, Arc::new(one_sensor_device()));

    let mut a: Client = Client::connect(server.addr).expect("connect A");
    a.handshake_hello();
    assert!(
        wait_until(Duration::from_secs(2), || server
            .handle
            .active_connections()
            == 1),
        "A holds the only slot"
    );

    // A leaves; the slot must free.
    drop(a);
    assert!(
        wait_until(Duration::from_secs(3), || server
            .handle
            .active_connections()
            == 0),
        "A's slot should be reclaimed"
    );

    // A fresh connection now handshakes on the reclaimed slot.
    let mut c: Client = Client::connect(server.addr).expect("connect C");
    assert_eq!(c.handshake_hello(), "plantmon");

    server.stop();
}

/// A client that dribbles a partial frame then stalls is reaped within the idle
/// window, and its slot is freed — so one wedged client cannot brick the API.
#[test]
fn reaps_a_stalled_client_and_keeps_serving() {
    let config: ServerConfig = ServerConfig {
        max_connections: 1,
        read_timeout: Duration::from_millis(100),
        idle_timeout: Duration::from_millis(800),
        ..loopback_config()
    };
    let server: RunningServer = RunningServer::spawn(config, Arc::new(one_sensor_device()));

    // Send all but the last two bytes of a Hello frame, then stall (send nothing).
    let mut stalled: Client = Client::connect(server.addr).expect("connect stalled");
    let mut frame: Vec<u8> = Vec::new();
    encode_frame(message_id("HelloRequest").unwrap(), &[], &mut frame);
    stalled
        .send_raw(&frame[..frame.len() - 2])
        .expect("send partial frame");

    // The server reaps it within its idle window, closing the socket.
    stalled.assert_closed_by_server();
    assert!(
        wait_until(Duration::from_secs(2), || server
            .handle
            .active_connections()
            == 0),
        "the stalled connection's slot should be reclaimed"
    );

    // The API is not bricked: a healthy client is served on the freed slot.
    let mut healthy: Client = Client::connect(server.addr).expect("connect healthy");
    assert_eq!(healthy.handshake_hello(), "plantmon");

    server.stop();
}

/// 1000 connect/handshake/disconnect cycles leak neither a socket (fd count is
/// stable) nor a connection slot (the active count returns to zero) — a wedged
/// resource would show as monotonic growth.
#[cfg(target_os = "linux")]
#[test]
fn serves_a_thousand_cycles_without_leaking() {
    let config: ServerConfig = ServerConfig {
        max_connections: 2,
        read_timeout: Duration::from_millis(50),
        idle_timeout: Duration::from_secs(5),
        stack_size: 128 * 1024,
        ..loopback_config()
    };
    let server: RunningServer = RunningServer::spawn(config, Arc::new(one_sensor_device()));

    // Warm up so lazily-created fds (allocator arenas, the first thread) are not
    // miscounted as a leak, then take the baseline.
    one_cycle(&server);
    assert!(wait_until(Duration::from_secs(2), || server
        .handle
        .active_connections()
        == 0));
    let baseline: usize = open_fd_count();

    for _ in 0..1000 {
        one_cycle(&server);
    }
    assert!(
        wait_until(Duration::from_secs(5), || server
            .handle
            .active_connections()
            == 0),
        "every connection slot must be reclaimed"
    );

    let after: usize = open_fd_count();
    assert!(
        after <= baseline + 8,
        "fd leak: {baseline} open before 1000 cycles, {after} after (a per-cycle leak would be ~+1000)"
    );

    server.stop();
}

/// One connect/handshake/disconnect against the running server, completed before it returns.
///
/// The wait at the end is not politeness, it is the difference between this test measuring fd
/// reclamation and it measuring a race. Closing the socket only *starts* the server's teardown:
/// its connection thread has to observe EOF and drop the slot guard, and until it does the slot
/// is still counted. A loop that reconnects immediately can therefore find the cap already full
/// and be rejected — the server closing the connection exactly as
/// [`serves_up_to_the_cap_and_rejects_beyond_it`] asks it to — and the next read fails with a
/// FIN or an RST, depending on how the write raced the close. That was an intermittent failure
/// under load, at around iteration 600 of 1000.
///
/// So each cycle waits for its own slot back. It also makes the assertion stronger: the slot is
/// now shown to be reclaimed on every one of the thousand cycles, not merely by the end.
#[cfg(target_os = "linux")]
fn one_cycle(server: &RunningServer) {
    let mut client: Client = Client::connect(server.addr).expect("connect");
    assert_eq!(client.handshake_hello(), "plantmon");
    // Drop closes the socket; the server observes EOF and frees the slot.
    drop(client);
    assert!(
        wait_until(Duration::from_secs(2), || server
            .handle
            .active_connections()
            == 0),
        "the cycle's slot must be reclaimed before the next cycle connects"
    );
}

/// Open file descriptors of this process, via `/proc/self/fd`.
#[cfg(target_os = "linux")]
fn open_fd_count() -> usize {
    std::fs::read_dir("/proc/self/fd")
        .expect("read /proc/self/fd")
        .count()
}

/// A full handshake completes within the on-device plaintext per-connection stack
/// budget — a host proxy that the ~12 KB budget is not already overflowed by the
/// plaintext path before Noise is even added.
#[test]
fn serves_within_the_plaintext_stack_budget() {
    let config: ServerConfig = ServerConfig {
        stack_size: PLAINTEXT_STACK_SIZE,
        ..loopback_config()
    };
    let server: RunningServer = RunningServer::spawn(config, Arc::new(one_sensor_device()));
    let mut client: Client = Client::connect(server.addr).expect("connect");

    assert_eq!(client.handshake_hello(), "plantmon");
    assert_eq!(client.subscribe_one(), FIRST_READING);

    server.stop();
}
