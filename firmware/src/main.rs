//! M5StickC Plus firmware — composition root.
//!
//! Wires the driven adapters (WS2812 strip, monotonic clock) to the `led-core`
//! [`Animator`] and runs the render loop. All policy lives in the framework-free
//! domain; this file only assembles hardware and turns the crank.

#![no_std]
#![no_main]

mod adapters;

use adapters::{EspClock, Ws2812Strip};
use esp_backtrace as _;
use esp_hal::{rmt::Rmt, time::Rate, Config};
use esp_hal_smartled::{smart_led_buffer, SmartLedsAdapter};
use esp_println::println;
use led_core::{Animator, Clock, Rainbow, Rgb};

/// Number of pixels on the strip. Set this to your hardware.
const LED_COUNT: usize = 30;

/// Data line into the strip. GPIO32 is the M5StickC Plus Grove port pin.
/// (The pin is chosen below; this constant documents the wiring.)
const DATA_PIN: &str = "GPIO32";

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(Config::default());
    println!("stick-led-firmware: booting — {LED_COUNT} LEDs on {DATA_PIN}");

    // --- Driven adapters (boundary) ---
    let rmt = Rmt::new(peripherals.RMT, Rate::from_mhz(80)).expect("RMT init failed");
    let mut rmt_buffer = smart_led_buffer!(LED_COUNT);
    let sink = SmartLedsAdapter::new(rmt.channel0, peripherals.GPIO32, &mut rmt_buffer);
    let mut strip = Ws2812Strip::new(sink);
    let clock = EspClock::start();

    // --- Domain wiring (control) ---
    let mut animator = Animator::new(Rainbow { spatial: 8, speed: 40, sat: 255, val: 128 });
    let mut frame = [Rgb::BLACK; LED_COUNT];
    let delay = esp_hal::delay::Delay::new();

    // --- Render loop (~50 fps) ---
    loop {
        let now = clock.now();
        if let Err(err) = animator.tick(now, &mut frame, &mut strip) {
            println!("strip write error: {err:?}");
        }
        delay.delay_millis(20);
    }
}
