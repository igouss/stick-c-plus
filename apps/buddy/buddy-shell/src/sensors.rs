//! The sensor thread: the buddy's own senses, on a slow cadence.
//!
//! The IMU decides whether the stick is being shaken or has been left face-down; the PMIC
//! decides whether it is on a charger. Both feed [`DeviceState::tick`], which is also what
//! advances the domain's clock — so this loop is the buddy's heartbeat, and the display loop is
//! only a reader.
//!
//! ## Slow on purpose
//!
//! A hundred milliseconds. The two things being sensed are a *shake* (the domain's detector is an
//! EMA over samples, which does not need a fast one) and a *nap* (hysteresis counted in frames,
//! deliberately slow). Polling faster would put I2C traffic on the bus the panel is not sharing,
//! wake the core ten times as often, and change the nap's timing — the counter is in frames, so
//! its rate *is* this period.
//!
//! ## A failed read is skipped, not fatal
//!
//! A flaky I2C read must not take the buddy down: the frame is skipped, the previous reading
//! stands, and the next cycle tries again. What is *not* skipped is the tick — time passes
//! whether or not the sensor answered, and a domain whose clock stopped because a bus glitched
//! would freeze the persona.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use log::warn;
use platform_core::{Acceleration, Clock, Imu, PowerSource, Tick};

use crate::shared::SharedDevice;
use crate::state::DeviceState;

/// How often the senses are polled — see the module docs for why it is not faster.
pub const SENSE_PERIOD: Duration = Duration::from_millis(100);

/// The sensor thread's stack, in bytes.
///
/// Two register reads and a fold through the pure domain; nothing here allocates on the loop
/// path. Four kibibytes, validated against the high-water mark on the metal before it is trusted.
pub const SENSE_STACK_SIZE: usize = 4 * 1024;

/// A running sensor thread — a handle to stop and join it.
pub struct SenseTask {
    handle: JoinHandle<()>,
    stop: Arc<AtomicBool>,
}

impl SenseTask {
    /// Ask the loop to finish after its current cycle.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Block until the thread has exited, propagating a panic it carried.
    pub fn join(self) -> thread::Result<()> {
        self.handle.join()
    }
}

/// Spawn the sensor thread: read the IMU and the power rail, and tick the state.
pub fn spawn_sensors<I, P, C>(
    imu: I,
    power: P,
    shared: SharedDevice,
    clock: C,
) -> io::Result<SenseTask>
where
    I: Imu + Send + 'static,
    P: PowerSource + Send + 'static,
    C: Clock + Send + 'static,
{
    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let stop_in_thread: Arc<AtomicBool> = Arc::clone(&stop);
    let handle: JoinHandle<()> = thread::Builder::new()
        .name("buddy-sensors".to_string())
        .stack_size(SENSE_STACK_SIZE)
        .spawn(move || sense_loop(imu, power, shared, clock, stop_in_thread))?;
    Ok(SenseTask { handle, stop })
}

/// The thread body: read, fold, sleep — until asked to stop.
fn sense_loop<I, P, C>(
    mut imu: I,
    mut power: P,
    shared: SharedDevice,
    clock: C,
    stop: Arc<AtomicBool>,
) where
    I: Imu,
    P: PowerSource,
    C: Clock,
{
    // The reading the domain sees when the sensor has not answered yet: gravity alone, which is
    // neither a shake nor face-down — the honest "nothing is happening".
    let mut last: Acceleration = Acceleration::new(0, 0, 1_000);

    while !stop.load(Ordering::Relaxed) {
        let now: Tick = clock.now();

        match imu.acceleration() {
            Ok(accel) => last = accel,
            Err(_) => warn!("buddy-sensors: IMU read failed; holding the previous reading"),
        }
        // Both halves of the power question, and a failure in either holds the previous answer:
        // a flaky I2C read must not make the glass report a charge nobody measured, and must not
        // make a board on a charger read as unplugged and chime about it.
        match (power.on_usb(), power.battery_pct()) {
            (Ok(on_usb), Ok(pct)) => {
                shared.with(|state: &mut DeviceState| state.power(on_usb, pct))
            }
            _ => warn!("buddy-sensors: PMIC read failed; holding the previous power state"),
        }

        // The tick happens whatever the reads did: time passes regardless, and a domain whose
        // clock stopped because a bus glitched would freeze the persona on the glass.
        shared.with(|state: &mut DeviceState| state.tick(now, last, true));

        thread::sleep(SENSE_PERIOD);
    }
}
