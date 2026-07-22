#![forbid(unsafe_code)]
//! Prove the BLE transport the buddy is built on, before any of the buddy is written.
//!
//! Advertises as `Claude-XXXX`, exposes the Nordic UART Service, demands LE Secure
//! Connections bonding, and echoes newline-delimited lines back at the negotiated MTU.
//! If this cannot boot and stay up, the epic's shape changes — so it is flashed first.
//!
//! It also reports what it costs. The planning estimate was ~250 KB of flash and
//! ~60-75 KB of heap; the rest of the design rests on those numbers, so the spike
//! measures rather than taking the estimate on trust.

use std::sync::{Arc, Mutex};

use esp32_nimble::{
    enums::{AuthReq, SecurityIOCap},
    utilities::{mutex::Mutex as NimbleMutex, BleUuid},
    BLEAdvertisementData, BLECharacteristic, BLEDevice, BLEServer, NimbleProperties, OnWriteArgs,
};
use esp_idf_svc::hal::delay::FreeRtos;

/// Nordic UART Service, byte-compatible with claude-desktop-buddy's REFERENCE.md so the
/// stick would also pair with Claude Desktop's Hardware Buddy window.
const NUS_SERVICE: &str = "6e400001-b5a3-f393-e0a9-e50e24dcca9e";
/// Central -> device. Written by the host bridge.
const NUS_RX: &str = "6e400002-b5a3-f393-e0a9-e50e24dcca9e";
/// Device -> central, by notification.
const NUS_TX: &str = "6e400003-b5a3-f393-e0a9-e50e24dcca9e";

/// A line longer than this is a peer bug, not a heartbeat. Upstream caps at 1024 (data.h)
/// and drops the excess rather than the line; same policy here so both ends degrade alike.
const MAX_LINE: usize = 1024;

/// Fixed for the spike so pairing is reproducible across flashes. The real firmware shows a
/// per-pairing passkey on the glass; that lands with the display bead.
const SPIKE_PASSKEY: u32 = 123_456;

// A boot-time bring-up failure is unrecoverable, so this root panics with context rather than
// propagating — the same convention the other composition roots in this workspace follow.
fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let before_free: usize = esp_metrics::free_heap();
    let before_block: usize = esp_metrics::largest_free_block();
    log::info!("heap before BLE init: free={before_free} B largest_block={before_block} B");

    let device: &mut BLEDevice = BLEDevice::take();

    // LE Secure Connections + bonding + MITM. DisplayOnly means the peer types the passkey
    // this device shows, which is the association model REFERENCE.md asks for.
    device
        .security()
        .set_auth(AuthReq::all())
        .set_passkey(SPIKE_PASSKEY)
        .set_io_cap(SecurityIOCap::DisplayOnly)
        .resolve_rpa();

    // Upstream derives the advertised name from the last two bytes of the BT MAC
    // (main.cpp:14-19) so several sticks stay distinguishable in the desktop's picker.
    let name: String = advertised_name(device);
    log::info!("advertising as {name}, passkey {SPIKE_PASSKEY:06}");

    let server: &mut BLEServer = device.get_server();
    server.on_connect(|_server, desc| {
        log::info!(
            "connected: {} mtu={} bonded={} encrypted={}",
            desc.address(),
            desc.mtu(),
            desc.bonded(),
            desc.encrypted()
        );
    });
    server.on_disconnect(|_desc, reason| log::info!("disconnected: {reason:?}"));
    server.on_authentication_complete(|_server, desc, result| {
        // The whole point of the spike: this line is what proves bonding worked.
        log::info!(
            "auth complete {:?}: bonded={} encrypted={} authenticated={} key_size={}",
            result,
            desc.bonded(),
            desc.encrypted(),
            desc.authenticated(),
            desc.sec_key_size()
        );
    });

    let service = server.create_service(nus_uuid(NUS_SERVICE));

    // TX notifies, and is readable only over an encrypted, authenticated link. Note the CCCD
    // itself stays unencrypted: NimBLE creates that descriptor and esp32-nimble panics if you
    // try to build your own. A sniffer therefore learns subscription state, never payload.
    let tx: Arc<NimbleMutex<BLECharacteristic>> = service.lock().create_characteristic(
        nus_uuid(NUS_TX),
        NimbleProperties::NOTIFY | NimbleProperties::READ_ENC | NimbleProperties::READ_AUTHEN,
    );

    let rx: Arc<NimbleMutex<BLECharacteristic>> = service.lock().create_characteristic(
        nus_uuid(NUS_RX),
        NimbleProperties::WRITE
            | NimbleProperties::WRITE_NO_RSP
            | NimbleProperties::WRITE_ENC
            | NimbleProperties::WRITE_AUTHEN,
    );

    // Writes arrive fragmented at the MTU boundary with no framing, so bytes accumulate here
    // until a terminator shows up.
    let pending: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::with_capacity(MAX_LINE)));
    let echo_tx: Arc<NimbleMutex<BLECharacteristic>> = tx.clone();

    rx.lock().on_write(move |args: &mut OnWriteArgs| {
        let conn_handle: u16 = args.desc().conn_handle();
        let mtu: u16 = args.desc().mtu();
        let mut buffer = pending.lock().expect("rx line buffer poisoned");

        for line in split_lines(&mut buffer, args.recv_data()) {
            log::info!(
                "rx line ({} B): {}",
                line.len(),
                String::from_utf8_lossy(&line)
            );
            notify_chunked(&echo_tx, conn_handle, mtu, &line);
        }
    });

    let advertising = device.get_advertising();
    // A 128-bit service UUID plus a name overflows the 31-byte advertisement; set_data moves
    // the name into the scan response automatically rather than truncating it.
    advertising
        .lock()
        .set_data(
            BLEAdvertisementData::new()
                .name(&name)
                .add_service_uuid(nus_uuid(NUS_SERVICE)),
        )
        .expect("advertisement data");
    advertising.lock().start().expect("start advertising");

    // The number the rest of the epic rests on: what the BLE stack actually took, measured
    // rather than estimated. Planning guessed 60-75 KB.
    let after_free: usize = esp_metrics::free_heap();
    let after_block: usize = esp_metrics::largest_free_block();
    log::info!(
        "heap after BLE init: free={after_free} B largest_block={after_block} B \
         (BLE cost {} B free, {} B contiguous)",
        before_free.saturating_sub(after_free),
        before_block.saturating_sub(after_block)
    );
    log::info!("bonded addresses: {:?}", device.bonded_addresses());

    // A reboot loop is the failure this spike exists to catch, so it reports liveness on a
    // slow cadence rather than exiting. 10 s is well clear of the board's 10 ms tick floor.
    let mut ticks: u32 = 0;
    loop {
        FreeRtos::delay_ms(10_000);
        ticks += 1;
        // `minimum_free_heap` is the one that catches a transient the periodic sample would
        // otherwise stroll straight past.
        log::info!(
            "alive {}s connections={} free={} B largest_block={} B min_ever={} B",
            ticks * 10,
            server.connected_count(),
            esp_metrics::free_heap(),
            esp_metrics::largest_free_block(),
            esp_metrics::minimum_free_heap()
        );

        // An unprompted notification to every subscriber, so the device -> host direction can
        // be observed without a write to trigger it. This is also the shape the real firmware
        // needs: the desktop sends a heartbeat every 10 s and the device answers on its own
        // schedule, never only in reply.
        let beat: String = format!("{{\"alive\":{}}}\n", ticks * 10);
        tx.lock().set_value(beat.as_bytes()).notify();
    }
}

/// Parses one of the three Nordic UART UUIDs.
///
/// The inputs are compile-time constants from REFERENCE.md, so a parse failure is a typo in
/// this file and nothing a running device could recover from — panic with the offender named.
fn nus_uuid(uuid: &str) -> BleUuid {
    BleUuid::from_uuid128_string(uuid).unwrap_or_else(|e| panic!("malformed UUID {uuid}: {e:?}"))
}

/// `Claude-XXXX` from the last two bytes of the BT address, matching upstream so the desktop
/// picker's `Claude` prefix filter still finds this device.
fn advertised_name(device: &BLEDevice) -> String {
    let bytes: [u8; 6] = device
        .get_addr()
        .expect("BT controller address")
        .as_be_bytes();
    format!("Claude-{:02X}{:02X}", bytes[4], bytes[5])
}

/// Drains whole lines out of `buffer` as `chunk` arrives, leaving any partial tail behind.
///
/// Terminates on `\n` or `\r`, skips empty lines, and drops bytes past [`MAX_LINE`] rather
/// than the line itself — upstream's behaviour (data.h), kept so a truncating peer degrades
/// the same way on both firmwares.
fn split_lines(buffer: &mut Vec<u8>, chunk: &[u8]) -> Vec<Vec<u8>> {
    let mut lines: Vec<Vec<u8>> = Vec::new();

    for &byte in chunk {
        if byte == b'\n' || byte == b'\r' {
            if !buffer.is_empty() {
                lines.push(core::mem::take(buffer));
            }
            continue;
        }
        if buffer.len() < MAX_LINE {
            buffer.push(byte);
        }
    }

    lines
}

/// Notifies `line` (plus its terminator) in MTU-sized pieces.
///
/// esp32-nimble does NOT fragment for you: `send_value` reads the MTU only to reject a zero,
/// then hands the whole buffer to `ble_gatts_notify_custom`, which truncates. An ATT payload
/// is the MTU minus the 3-byte notification header.
fn notify_chunked(
    characteristic: &Arc<NimbleMutex<BLECharacteristic>>,
    conn_handle: u16,
    mtu: u16,
    line: &[u8],
) {
    let payload: usize = usize::from(mtu).saturating_sub(3).max(1);
    let mut framed: Vec<u8> = Vec::with_capacity(line.len() + 1);
    framed.extend_from_slice(line);
    framed.push(b'\n');

    for piece in framed.chunks(payload) {
        if let Err(err) = characteristic.lock().notify_with(piece, conn_handle) {
            log::warn!("notify failed ({} B): {err:?}", piece.len());
            return;
        }
    }
}
