#![forbid(unsafe_code)]
//! claude-buddy — the composition root: a desk pet that gates Claude Code's tool calls.
//!
//! Press A on the stick to approve the tool call Claude Code is asking about, B to deny. A hook
//! on the owner's machine holds each call, a bridge daemon carries it over a bonded BLE link, and
//! this firmware is the end of that chain: it renders the question and sends the answer back.
//!
//! This root powers the LCD rails (AXP192), builds the board-generic adapters (the ST7789
//! [`Panel`], the G37/G39 [`GpioButton`]s, the MPU6886 IMU), brings up the NimBLE peripheral, and
//! wires them to the host-tested loops:
//!
//! - [`spawn_input`] (buddy-shell) polls the buttons and folds each press into the device state,
//!   carrying out whatever [`Effect`](buddy_shell::Effect) comes back — the permission response
//!   on the wire, the bond forgotten, the species persisted.
//! - [`spawn_sensors`] (buddy-shell) reads the IMU and the power rail every 100 ms and ticks the
//!   domain, so the buddy shakes, naps, and notices a charger.
//! - [`spawn_display`] (platform-runtime) is the *same* generic render loop the plant monitor and
//!   the pomodoro timer use, fed the shared state's [`BuddyView`] and a [`PanelScreen`] that
//!   paints it with `buddy_display::render`.
//! - The NUS receive callback feeds a [`Receiver`], which frames, classifies, and folds each
//!   inbound line — the only path by which a snapshot can reach the glass.
//!
//! ## The fixed passkey is gone
//!
//! The BLE spike this grew out of used a constant passkey (`123456`) so pairing was reproducible
//! across flashes. That is fatal in a product: a published constant defeats the MITM protection
//! LE Secure Connections bonding exists to provide — anyone who knows it can complete the
//! pairing. Here [`BLEServer::on_passkey_request`] draws a **fresh random passkey per pairing**
//! from the hardware RNG and publishes it to the device state, which puts it on the glass as a
//! full-screen takeover. The screen is what makes the random key possible: a secret nobody can
//! read is a secret nobody can type.
//!
//! ## Threads and the one state
//!
//! Four writers (input, sensors, the BLE receive callback, the BLE security callbacks) and one
//! reader (the display loop) share a single [`SharedDevice`]. It is poison-tolerant on purpose: a
//! panicking writer must not freeze the glass, because a frozen picture on a desk pet is a worse
//! failure than a stale one.

use std::sync::{Arc, Mutex};

use board_support::{internal_i2c, AccelRange, Axp192, Mpu6886};
use buddy_core::SpeciesIndex;
use buddy_display::BuddyView;
use buddy_shell::{
    spawn_input, spawn_sensors, Bond, DeviceState, Identity, Notifier, NotifyError, Receiver,
    SharedDevice, SpeciesStore,
};
use embedded_hal_bus::i2c::MutexDevice;
use esp32_nimble::{
    enums::{AuthReq, SecurityIOCap},
    utilities::{mutex::Mutex as NimbleMutex, BleUuid},
    BLEAdvertisementData, BLECharacteristic, BLEDevice, BLEServer, NimbleProperties, OnWriteArgs,
};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_hal::i2c::I2cDriver;
use esp_idf_hal::ledc::LowSpeed;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault};
use log::{info, warn};
use platform_adapters::{
    Axp192Backlight, Axp192PowerSource, Fixed, GpioButton, LedcBuzzer, Mpu6886Imu, Panel,
    PanelScreen, PekButton,
};
use platform_core::{ScreenRotation, Tick};
use platform_input::Buttons;
use platform_runtime::{
    spawn_buzzer, spawn_display, spawn_power_watch, BacklightSwitch, BuzzerHandle, BuzzerTask,
    DisplayConfig, LitFlag, Monotonic, PowerWatchConfig, DISPLAY_STACK_SIZE,
};
use static_cell::StaticCell;

/// Nordic UART Service, byte-compatible with the bridge and with Claude Desktop's Hardware Buddy.
const NUS_SERVICE: &str = "6e400001-b5a3-f393-e0a9-e50e24dcca9e";
/// Central → device. Written by the host bridge.
const NUS_RX: &str = "6e400002-b5a3-f393-e0a9-e50e24dcca9e";
/// Device → central, by notification.
const NUS_TX: &str = "6e400003-b5a3-f393-e0a9-e50e24dcca9e";

/// The NVS namespace the buddy keeps its settings in.
const NVS_NAMESPACE: &str = "buddy";

/// The firmware version the info screen reports.
const FIRMWARE: &str = env!("CARGO_PKG_VERSION");

/// The display thread's stack.
///
/// Double the platform default. Measured, not guessed: the plant monitor's network+display app
/// overflowed 8 KiB when the HTTP poller preempted a render mid-SPI, which corrupted the SPI lock
/// semaphore rather than faulting cleanly. This app has the same shape — a BLE stack preempting a
/// paint — so it takes the same headroom. See `kb/findings`.
const BUDDY_DISPLAY_STACK: usize = 2 * DISPLAY_STACK_SIZE;

/// The internal I2C bus' `'static` home, shared by the PMIC and the IMU.
static I2C_BUS: StaticCell<Mutex<I2cDriver<'static>>> = StaticCell::new();

/// Who the peer is, as last observed on the link — the handle and MTU an unsolicited notify needs.
///
/// A notify is not a reply, so nothing hands the transport a connection descriptor at the moment
/// the owner presses a button. This is where the last one is kept, refreshed on connect and on
/// every write, so the MTU used is the one the peer actually negotiated rather than the 23-byte
/// default it started from.
#[derive(Clone, Copy, Debug)]
struct Peer {
    conn_handle: u16,
    mtu: u16,
}

/// The device→central line sink: the NUS TX characteristic.
#[derive(Clone)]
struct NusNotifier {
    tx: Arc<NimbleMutex<BLECharacteristic>>,
    peer: Arc<Mutex<Option<Peer>>>,
}

impl Notifier for NusNotifier {
    fn notify(&self, line: &str) -> Result<(), NotifyError> {
        let peer: Peer = self
            .peer
            .lock()
            .map_err(|_| NotifyError::new("the peer cell was poisoned"))?
            .ok_or_else(|| NotifyError::new("no central is connected"))?;

        // esp32-nimble does NOT fragment: `send_value` reads the MTU only to reject a zero, then
        // hands the whole buffer to `ble_gatts_notify_custom`, which truncates. An ATT payload is
        // the MTU minus the 3-byte notification header.
        let payload: usize = usize::from(peer.mtu).saturating_sub(3).max(1);
        let mut framed: Vec<u8> = Vec::with_capacity(line.len() + 1);
        framed.extend_from_slice(line.as_bytes());
        framed.push(b'\n');

        for piece in framed.chunks(payload) {
            self.tx
                .lock()
                .notify_with(piece, peer.conn_handle)
                .map_err(|err| NotifyError::new(format!("{err:?}")))?;
        }
        Ok(())
    }
}

/// The BLE bond, as the reset overlay forgets it.
struct NimbleBond;

impl Bond for NimbleBond {
    fn forget(&self) {
        // Re-taking the device is how every other callback in this file reaches it; the handle is
        // a `'static` singleton, not an owned resource.
        match BLEDevice::take().delete_all_bonds() {
            Ok(()) => info!("bonds forgotten — the stick is pairable again"),
            Err(err) => warn!("could not forget the bonds: {err:?}"),
        }
    }
}

/// The selected species, persisted to NVS so a reboot keeps the owner's creature.
struct NvsSpecies {
    nvs: Mutex<EspNvs<NvsDefault>>,
}

impl SpeciesStore for NvsSpecies {
    fn store(&self, index: u8) {
        let stored: Result<(), String> = self
            .nvs
            .lock()
            .map_err(|_| "the NVS handle was poisoned".to_string())
            .and_then(|nvs| {
                nvs.set_u8(buddy_core::SPECIES_NVS_KEY, index)
                    .map_err(|err| format!("{err:?}"))
            });
        match stored {
            Ok(()) => info!("species #{index} persisted"),
            // Not fatal: the buddy keeps the creature for this boot and loses it on the next one,
            // which is a far better failure than taking the device down over a cosmetic setting.
            Err(reason) => warn!("species #{index} not persisted: {reason}"),
        }
    }
}

// A boot-time bring-up failure is unrecoverable, so this root panics with context rather than
// propagating — the same convention every other composition root in this workspace follows.
fn main() {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();
    info!("claude-buddy {FIRMWARE}: std/ESP-IDF up — the desk pet that gates your tool calls");

    let peripherals: Peripherals = Peripherals::take().expect("peripherals already taken");

    // ---- persistence -------------------------------------------------------------------
    let partition: EspDefaultNvsPartition =
        EspDefaultNvsPartition::take().expect("the default NVS partition");
    let nvs: EspNvs<NvsDefault> =
        EspNvs::new(partition, NVS_NAMESPACE, true).expect("the buddy NVS namespace");
    // A missing key is a first boot, not a fault: fall back to the first registered creature.
    let species: SpeciesIndex = SpeciesIndex::new(
        nvs.get_u8(buddy_core::SPECIES_NVS_KEY)
            .ok()
            .flatten()
            .unwrap_or(0),
    );
    info!("species #{} restored from NVS", species.get());
    let species_store: NvsSpecies = NvsSpecies {
        nvs: Mutex::new(nvs),
    };

    // ---- the board ---------------------------------------------------------------------
    let i2c: I2cDriver<'static> = internal_i2c(
        peripherals.i2c0,
        peripherals.pins.gpio21,
        peripherals.pins.gpio22,
    )
    .expect("internal I2C bring-up");
    let bus: &'static Mutex<I2cDriver<'static>> = I2C_BUS.init(Mutex::new(i2c));

    // Power the LCD/TFT rails before building the panel — an unpowered panel takes a correct init
    // and still shows nothing.
    let mut axp: Axp192<MutexDevice<'static, I2cDriver<'static>>> =
        Axp192::new(MutexDevice::new(bus));
    axp.power_on().expect("AXP192 LCD/TFT rail power-on");
    let power_source: Axp192PowerSource<_> = Axp192PowerSource::new(axp);

    // `init` checks WHO_AM_I before it writes anything, so this panics on a mis-wired or
    // unpowered sensor rather than reporting a shake read from a floating bus.
    let mut imu: Mpu6886<MutexDevice<'static, I2cDriver<'static>>> =
        Mpu6886::new(MutexDevice::new(bus), AccelRange::G4);
    imu.init(&mut FreeRtos).expect("MPU6886 IMU bring-up");
    let imu: Mpu6886Imu<_> = Mpu6886Imu::new(imu);

    let panel: Panel = Panel::new(
        peripherals.spi2,
        peripherals.pins.gpio13, // SCLK
        peripherals.pins.gpio15, // MOSI
        peripherals.pins.gpio5,  // CS
        peripherals.pins.gpio23, // DC
        peripherals.pins.gpio18, // RST
    )
    .expect("ST7789 panel bring-up");
    // `new`, not `turning`: the buddy sits on a desk and is read from one side. Choosing the fixed
    // policy is what keeps the rotation path out of this binary; the picture still has a portrait
    // layout, which the screen-rotation bead can switch on by changing this one constructor.
    let screen: PanelScreen<BuddyView, _, Fixed> = PanelScreen::new(
        panel,
        |target: &mut _, view: BuddyView, elapsed: Tick, rotation: ScreenRotation| {
            buddy_display::render(target, &view, elapsed, rotation)
        },
    );

    let front: GpioButton = GpioButton::new(peripherals.pins.gpio37).expect("front button G37");
    let side: GpioButton = GpioButton::new(peripherals.pins.gpio39).expect("side button G39");
    let power_button: PekButton<MutexDevice<'static, I2cDriver<'static>>> =
        PekButton::new(Axp192::new(MutexDevice::new(bus)));
    let buttons: Buttons<_, _, _> =
        Buttons::new(front, side, power_button, buddy_shell::INPUT_CONFIG);

    let backlight: BacklightSwitch<_> = BacklightSwitch::new(Axp192Backlight::new(
        Axp192::new(MutexDevice::new(bus)),
        true,
    ));
    let lit: LitFlag = backlight.flag();

    // The passive buzzer on G2, through LEDC. One buzzer, one owner: it moves into a single
    // owner thread and every caller plays through a `Clone + Send` handle, so two sounds can
    // never interleave or truncate one another. Held for the life of main.
    let buzzer: LedcBuzzer<'_, LowSpeed> = LedcBuzzer::new(
        peripherals.ledc.timer0,
        peripherals.ledc.channel0,
        peripherals.pins.gpio2,
    )
    .expect("buzzer G2 (LEDC)");
    let (_buzzer_owner, tone): (BuzzerTask, BuzzerHandle) =
        spawn_buzzer(buzzer).expect("spawn buzzer owner");

    // ---- the state ---------------------------------------------------------------------
    let device: &mut BLEDevice = BLEDevice::take();
    let address: String = address_of(device);
    let name: String = advertised_name(&address);
    let shared: SharedDevice = SharedDevice::new(DeviceState::new(
        Identity::new(&name, FIRMWARE, &address),
        species,
    ));
    shared.with(|state: &mut DeviceState| {
        state.set_bonded(
            !BLEDevice::take()
                .bonded_addresses()
                .unwrap_or_default()
                .is_empty(),
        )
    });

    // ---- BLE ---------------------------------------------------------------------------
    let peer: Arc<Mutex<Option<Peer>>> = Arc::new(Mutex::new(None));
    let tx: Arc<NimbleMutex<BLECharacteristic>> =
        bring_up_ble(device, &name, shared.clone(), peer.clone());
    let notifier: NusNotifier = NusNotifier {
        tx,
        peer: peer.clone(),
    };

    // ---- the loops ---------------------------------------------------------------------
    let clock: Monotonic = Monotonic::start();

    let _sensors =
        spawn_sensors(imu, power_source, shared.clone(), clock).expect("spawn buddy-sensors");
    info!("sensor thread up: MPU6886 + VBUS polled at 10 Hz, ticking the domain");

    let _input = spawn_input(
        buttons,
        shared.clone(),
        clock,
        notifier,
        NimbleBond,
        species_store,
    )
    .expect("spawn buddy-input");
    info!("input thread up: A allows the pending tool call, B denies it, hold A for the menu");

    let source = {
        let shared: SharedDevice = shared.clone();
        move |_now: Tick| shared.view()
    };
    let _display = spawn_display(
        screen,
        source,
        |_now: Tick| ScreenRotation::Deg0,
        lit,
        clock,
        DisplayConfig {
            stack_size: BUDDY_DISPLAY_STACK,
            ..DisplayConfig::default()
        },
    )
    .expect("spawn buddy-display");
    info!("display thread up: ST7789 rendering the creature, the HUD and the approval screen");

    // Power-watch: poll VBUS on its own AXP192 handle, debounce it, and sound the spool-up /
    // spool-down chime a settled USB plug or unplug decides — through the same one buzzer owner
    // the rest of the app plays through. Silent at boot: the first sample only seeds the
    // baseline. A second handle rather than a shared one because the sensor thread owns the
    // first: both are thin readers of the same PMIC over the bus mutex.
    let vbus: Axp192PowerSource<MutexDevice<'static, I2cDriver<'static>>> =
        Axp192PowerSource::new(Axp192::new(MutexDevice::new(bus)));
    let _power_watch = spawn_power_watch(vbus, tone, clock, PowerWatchConfig::default())
        .expect("spawn buddy power-watch");
    info!("power-watch thread up: USB plug = spool-up, unplug = spool-down");

    // The backlight switch has no other owner in this app — the buddy's glass stays lit, and the
    // display loop's LitFlag reads the `true` the wrapper published at power-on. Held so the
    // switch is not dropped.
    let _backlight = backlight;

    // Supervisory loop: a heartbeat only. A reboot loop is what this reports, and the heap is what
    // predicts one — `minimum_free_heap` catches a transient the periodic sample strolls past.
    let mut seconds: u32 = 0;
    loop {
        FreeRtos::delay_ms(10_000);
        seconds += 10;
        let view: BuddyView = shared.view();
        info!(
            "alive {seconds}s persona={:?} linked={} running={} waiting={} prompt={} \
             free={} B min_ever={} B",
            view.persona,
            view.device.linked,
            view.sessions_running,
            view.sessions_waiting,
            view.prompt.is_some(),
            esp_metrics::free_heap(),
            esp_metrics::minimum_free_heap()
        );
    }
}

/// Bring up the NimBLE peripheral: security, the server callbacks, the NUS service, advertising.
///
/// Returns the TX characteristic the notifier sends through.
fn bring_up_ble(
    device: &mut BLEDevice,
    name: &str,
    shared: SharedDevice,
    peer: Arc<Mutex<Option<Peer>>>,
) -> Arc<NimbleMutex<BLECharacteristic>> {
    let before_free: usize = esp_metrics::free_heap();

    // LE Secure Connections + bonding + MITM. DisplayOnly means the peer types the passkey this
    // device shows — the association model the whole passkey screen exists to serve.
    device
        .security()
        .set_auth(AuthReq::all())
        .set_io_cap(SecurityIOCap::DisplayOnly)
        .resolve_rpa();

    // Keep the GAP device name (0x2A00) equal to the advertised Complete Local Name. NimBLE
    // defaults it to "nimble", and BlueZ caches that GATT-read name and serves it back to a
    // central's name filter — so a bridge matching the "Claude-" prefix silently never sees the
    // device once it has connected. One name, one source of truth.
    BLEDevice::set_device_name(name).expect("set the GAP device name");

    let server: &mut BLEServer = device.get_server();

    // THE FIXED PASSKEY DIES HERE. A fresh draw per pairing, from the hardware RNG (the radio is
    // up by definition at this point, which is the condition under which it is a true one), shown
    // on the glass for the owner to type on the host.
    let passkey_state: SharedDevice = shared.clone();
    server.on_passkey_request(move || {
        let passkey: u32 = esp_entropy::passkey();
        passkey_state.with(|state: &mut DeviceState| state.show_passkey(Some(passkey)));
        info!("pairing: showing a fresh passkey on the glass");
        passkey
    });

    let connect_state: SharedDevice = shared.clone();
    let connect_peer: Arc<Mutex<Option<Peer>>> = peer.clone();
    server.on_connect(move |_server, desc| {
        info!(
            "connected: {} mtu={} bonded={} encrypted={}",
            desc.address(),
            desc.mtu(),
            desc.bonded(),
            desc.encrypted()
        );
        remember(&connect_peer, desc.conn_handle(), desc.mtu());
        connect_state.with(|state: &mut DeviceState| state.set_bonded(desc.bonded()));
    });

    let auth_state: SharedDevice = shared.clone();
    server.on_authentication_complete(move |_server, desc, result| {
        info!(
            "auth complete {result:?}: bonded={} encrypted={} authenticated={}",
            desc.bonded(),
            desc.encrypted(),
            desc.authenticated()
        );
        // The passkey has done its job either way — a successful bond does not need it and a
        // failed one must not leave it on the glass for the next person to read.
        auth_state.with(|state: &mut DeviceState| {
            state.show_passkey(None);
            state.set_bonded(desc.bonded());
        });
    });

    let disconnect_peer: Arc<Mutex<Option<Peer>>> = peer.clone();
    let disconnect_state: SharedDevice = shared.clone();
    server.on_disconnect(move |_desc, reason| {
        info!("disconnected: {reason:?}");
        forget_peer(&disconnect_peer);
        disconnect_state.with(|state: &mut DeviceState| state.show_passkey(None));
        // Advertising restarts on its own after a disconnect; nothing to do but say so.
    });

    let service = server.create_service(nus_uuid(NUS_SERVICE));

    // TX notifies, and is readable only over an encrypted, authenticated link. The CCCD itself
    // stays unencrypted: NimBLE creates that descriptor and esp32-nimble panics if you build your
    // own. A sniffer therefore learns subscription state, never payload.
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

    // The receive path: raw fragments in, framed and classified lines folded into the state. The
    // clock is started here rather than shared with the loops because the callback runs on the
    // NimBLE host task, and a `Monotonic` is a cheap value over the same underlying counter.
    let clock: Monotonic = Monotonic::start();
    let mut receiver: Receiver = Receiver::new(shared);
    let write_peer: Arc<Mutex<Option<Peer>>> = peer;
    rx.lock().on_write(move |args: &mut OnWriteArgs| {
        remember(&write_peer, args.desc().conn_handle(), args.desc().mtu());
        let now: Tick = platform_core::Clock::now(&clock);
        receiver.feed(args.recv_data(), now);
    });

    let advertising = device.get_advertising();
    // A 128-bit service UUID plus a name overflows the 31-byte advertisement; `set_data` moves the
    // name into the scan response automatically rather than truncating it.
    advertising
        .lock()
        .set_data(
            BLEAdvertisementData::new()
                .name(name)
                .add_service_uuid(nus_uuid(NUS_SERVICE)),
        )
        .expect("advertisement data");
    advertising.lock().start().expect("start advertising");

    info!(
        "advertising as {name}; BLE cost {} B of free heap",
        before_free.saturating_sub(esp_metrics::free_heap())
    );
    tx
}

/// Record who the peer is, for the unsolicited notify a button press produces.
fn remember(peer: &Arc<Mutex<Option<Peer>>>, conn_handle: u16, mtu: u16) {
    match peer.lock() {
        Ok(mut cell) => *cell = Some(Peer { conn_handle, mtu }),
        Err(_) => warn!("the peer cell was poisoned; a notify will report no central"),
    }
}

/// Forget the peer — there is nobody to notify until someone connects again.
fn forget_peer(peer: &Arc<Mutex<Option<Peer>>>) {
    match peer.lock() {
        Ok(mut cell) => *cell = None,
        Err(_) => warn!("the peer cell was poisoned; a notify will report no central"),
    }
}

/// Parses one of the three Nordic UART UUIDs.
///
/// The inputs are compile-time constants, so a parse failure is a typo in this file and nothing a
/// running device could recover from — panic with the offender named.
fn nus_uuid(uuid: &str) -> BleUuid {
    BleUuid::from_uuid128_string(uuid).unwrap_or_else(|e| panic!("malformed UUID {uuid}: {e:?}"))
}

/// The controller's Bluetooth address, as the info screen shows it.
fn address_of(device: &BLEDevice) -> String {
    let bytes: [u8; 6] = device
        .get_addr()
        .expect("BT controller address")
        .as_be_bytes();
    bytes
        .iter()
        .map(|byte: &u8| format!("{byte:02X}"))
        .collect::<Vec<String>>()
        .join(":")
}

/// `Claude-XXXX` from the last two bytes of the BT address, so the bridge's `Claude` prefix filter
/// finds this device and several sticks stay distinguishable.
fn advertised_name(address: &str) -> String {
    let tail: String = address.replace(':', "");
    format!("Claude-{}", &tail[tail.len().saturating_sub(4)..])
}
