//! WS2812 output adapter — the domain's [`LedOutput`] port driven straight off
//! the esp-hal RMT peripheral.
//!
//! Owned in-tree rather than via `esp-hal-smartled`: that crate pins
//! `esp-hal = ~1.0`, which would cap the whole firmware below the 1.1 line the
//! WiFi/OTA stack (`esp-radio`, `esp-rtos`, `esp-storage`) needs. Owning the
//! handful of lines of bit-timing here keeps the LED boundary from dictating the
//! firmware's `esp-hal` version — and pixel-to-pulse encoding is exactly the
//! kind of hardware detail a boundary adapter should hold.
//!
//! Timings are the datasheet WS2812 values (identical to the ones smartled
//! shipped), so the wire behaviour is unchanged.

use esp_hal::{
    gpio::{interconnect::PeripheralOutput, Level},
    rmt::{Channel, Error as RmtError, PulseCode, Tx, TxChannelConfig, TxChannelCreator},
    Blocking,
};
use led_core::{LedOutput, Rgb};

/// RMT source-clock frequency this encoder assumes, in MHz. The composition root
/// must build the `Rmt` at this rate — `Rmt::new(RMT, Rate::from_mhz(RMT_FREQ_MHZ))`
/// — because the pulse tick counts below are derived from it (with a clock
/// divider of 1, one tick = 1/80 µs = 12.5 ns).
pub const RMT_FREQ_MHZ: u32 = 80;

/// WS2812 bit period and high-times, in nanoseconds, per the datasheet
/// (800 kHz: a `0` bit holds high ~400 ns, a `1` bit ~850 ns, of a 1250 ns slot).
const CODE_PERIOD_NS: u32 = 1250;
const T0H_NS: u32 = 400;
const T1H_NS: u32 = 850;

/// RMT pulse entries needed to clock out one pixel: 3 bytes × 8 bits.
const PULSES_PER_LED: usize = 24;

/// RMT buffer length needed for `leds` pixels (plus one trailing end-marker).
///
/// Feed the result in as the adapter's `BUFFER_SIZE`:
/// `Ws2812Rmt::<{ buffer_size(LED_COUNT) }>::new(channel, pin)`.
pub const fn buffer_size(leds: usize) -> usize {
    leds * PULSES_PER_LED + 1
}

/// A failure writing a frame to the strip.
// `Transmission`'s inner cause is read only through `Debug` (logged in the
// composition root); the dead-code lint does not count that, hence the allow.
#[derive(Debug)]
#[allow(dead_code)]
pub enum Ws2812Error {
    /// The frame held more pixels than `BUFFER_SIZE` was sized for.
    FrameTooLarge,
    /// The underlying RMT transfer failed.
    Transmission(RmtError),
}

/// A WS2812 strip on one RMT TX channel, owning its pulse buffer.
///
/// `BUFFER_SIZE` must equal [`buffer_size`]`(led_count)`; a smaller buffer makes
/// [`write`](LedOutput::write) fail with [`Ws2812Error::FrameTooLarge`].
pub struct Ws2812Rmt<'ch, const BUFFER_SIZE: usize> {
    // `Option` because esp-hal's `transmit` consumes the channel and hands it
    // back from `wait`; it lives here, populated, between writes.
    channel: Option<Channel<'ch, Blocking, Tx>>,
    buffer: [PulseCode; BUFFER_SIZE],
    zero: PulseCode,
    one: PulseCode,
}

impl<'ch, const BUFFER_SIZE: usize> Ws2812Rmt<'ch, BUFFER_SIZE> {
    /// Claim an RMT TX channel and its data pin to drive a WS2812 strip.
    pub fn new<C, O>(channel: C, pin: O) -> Self
    where
        C: TxChannelCreator<'ch, Blocking>,
        O: PeripheralOutput<'ch>,
    {
        let channel = channel
            .configure_tx(&tx_config())
            .expect("RMT TX channel config is valid")
            .with_pin(pin);

        Self {
            channel: Some(channel),
            buffer: [PulseCode::end_marker(); BUFFER_SIZE],
            zero: pulse(T0H_NS),
            one: pulse(T1H_NS),
        }
    }
}

/// Blocking RMT TX config for WS2812: no divider, idle low, no carrier.
fn tx_config() -> TxChannelConfig {
    TxChannelConfig::default()
        .with_clk_divider(1)
        .with_idle_output_level(Level::Low)
        .with_idle_output(true)
        .with_carrier_modulation(false)
}

/// One WS2812 bit pulse: `high_ns` high, then the rest of the code period low,
/// expressed in RMT ticks at [`RMT_FREQ_MHZ`].
fn pulse(high_ns: u32) -> PulseCode {
    let ticks = |ns: u32| ((ns * RMT_FREQ_MHZ) / 1000) as u16;
    PulseCode::new(
        Level::High,
        ticks(high_ns),
        Level::Low,
        ticks(CODE_PERIOD_NS - high_ns),
    )
}

impl<const BUFFER_SIZE: usize> LedOutput for Ws2812Rmt<'_, BUFFER_SIZE> {
    type Error = Ws2812Error;

    fn write(&mut self, frame: &[Rgb]) -> Result<(), Self::Error> {
        // Encode the frame into RMT pulses: WS2812 takes GRB, most-significant
        // bit first, one pulse per bit, then an end-marker to close the frame.
        let mut slot = self.buffer.iter_mut();
        for px in frame {
            for byte in [px.g, px.r, px.b] {
                for bit in (0..8).rev() {
                    let pulse = if byte & (1 << bit) != 0 {
                        self.one
                    } else {
                        self.zero
                    };
                    *slot.next().ok_or(Ws2812Error::FrameTooLarge)? = pulse;
                }
            }
        }
        *slot.next().ok_or(Ws2812Error::FrameTooLarge)? = PulseCode::end_marker();

        // Fire the transfer. `transmit` takes the channel, `wait` returns it —
        // thread it back into `self` whichever way the transfer resolves.
        let channel = self.channel.take().expect("channel present between writes");
        let (result, channel) = match channel.transmit(&self.buffer) {
            Ok(transaction) => match transaction.wait() {
                Ok(channel) => (Ok(()), channel),
                Err((err, channel)) => (Err(Ws2812Error::Transmission(err)), channel),
            },
            Err((err, channel)) => (Err(Ws2812Error::Transmission(err)), channel),
        };
        self.channel = Some(channel);
        result
    }
}
