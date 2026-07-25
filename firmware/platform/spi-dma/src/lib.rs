//! Non-blocking double-buffered SPI presentation.
//!
//! # Why this crate exists
//!
//! Every other crate in both workspaces opens with `#![forbid(unsafe_code)]`, and that stays true.
//! This is the third crate (beside `dma-mem` and `esp-metrics`) allowed to speak raw C.
//!
//! A full-screen sketch spends its frame two ways: the CPU draws the picture, then the SPI DMA
//! engine streams it to the panel. Done the obvious way — the blocking
//! `spi_device_polling_transmit` esp-idf-hal exposes — those two happen in series: the render
//! thread draws (~13 ms), then *waits* for the ~10 ms transfer before it can draw again. The bus
//! sits idle while the CPU draws, and the CPU sits idle while the bus transfers. Half the frame is
//! spent waiting on the other half.
//!
//! The ESP-IDF SPI driver can instead run a transfer on **interrupts**: `spi_device_queue_trans`
//! hands the DMA engine a buffer and returns *at once*, and `spi_device_get_trans_result` blocks
//! only when you finally need the transfer to be done. With **two** buffers, the render thread draws
//! into one while the DMA engine streams the other, and the transfer disappears under the next
//! frame's compute — the frame costs `max(draw, transfer)`, not their sum. esp-idf-hal exposes only
//! the polling path, so the queue is reachable only through the raw `spi_device_handle_t` (from
//! `SpiDeviceDriver::device()`) and these two C calls. That is the whole of this crate.
//!
//! # The rule for this crate
//!
//! One responsibility: own the two buffers and the raw handle, and ping-pong them across queued
//! transfers. The `unsafe` is two C calls, both wrapped. The safety is **local**: [`DoubleBuffer`]
//! alone decides which buffer is lent out to draw and which is queued, and it never lends the one in
//! flight — so no caller can draw into a buffer the DMA is reading, and no `unsafe` obligation
//! escapes the crate. The buffers are `'static` (leaked at bring-up, see `dma-mem`), so the DMA can
//! never read freed memory. The panel command that *arms* each transfer (`RAMWR`, the DC line) is
//! **not** here — that is panel knowledge, and it stays in the adapter that owns the pins; this
//! crate moves bytes and knows nothing of what they mean.

#![no_std]

use core::ptr;

use esp_idf_sys::{
    esp, spi_device_get_trans_result, spi_device_handle_t, spi_device_queue_trans,
    spi_transaction_t, spi_transaction_t__bindgen_ty_1, spi_transaction_t__bindgen_ty_2, EspError,
    TickType_t,
};

/// `portMAX_DELAY`: wait forever. Used for both the queue (which never actually waits — a reap
/// precedes every queue, so the one-deep device queue always has room) and the reap (which waits
/// only if the transfer has not finished, the backpressure that keeps the ping-pong honest).
const BLOCK: TickType_t = TickType_t::MAX;

/// A pair of full-screen wire buffers ping-ponged across non-blocking SPI DMA transfers.
///
/// At any instant one buffer is *idle* — lent to the caller to draw the next frame — and the other
/// is either *in flight* (the DMA engine is streaming it) or already reaped and idle too. The caller
/// draws into [`back`](Self::back), then calls [`reap`](Self::reap) to reclaim the previous frame's
/// buffer and [`queue`](Self::queue) to launch the freshly drawn one. Because this type alone moves
/// `draw` and tracks what is in flight, `back` can never hand out the buffer the DMA is reading —
/// which is what makes the raw pointer handed to the C queue call sound.
pub struct DoubleBuffer {
    /// The SPI device the transfers run on. Raw because esp-idf-hal exposes no non-blocking transmit;
    /// its owner (the panel adapter) keeps the `SpiDeviceDriver` alive, so this stays valid.
    handle: spi_device_handle_t,
    /// The two wire buffers. `'static` (leaked DMA-capable memory) so the DMA can never outlive them.
    buffers: [&'static mut [u8]; 2],
    /// The transaction descriptor for the in-flight transfer. Stored here, not on the stack, because
    /// the ESP-IDF driver holds a pointer to it from [`queue`](Self::queue) until
    /// [`reap`](Self::reap) — so it must outlive the call that launched it, and its address must not
    /// move (this struct lives at the composition root for the app's life).
    transaction: spi_transaction_t,
    /// Which buffer [`back`](Self::back) lends next — the idle one. Flipped by [`queue`](Self::queue).
    draw: usize,
    /// Whether a transfer is in flight, so [`reap`](Self::reap) knows whether there is one to wait
    /// for (there is not, before the first frame is queued).
    in_flight: bool,
}

// SAFETY: the raw `spi_device_handle_t` and the transaction's tx pointer make this `!Send` by
// default, but a `DoubleBuffer` is *moved* to the display thread at bring-up and then used only from
// there — never shared between threads (it is not `Sync`, and there is one per panel). That is the
// same guarantee esp-idf-hal makes for `SpiDeviceDriver`, whose handle this borrows: an SPI device
// is safe to drive from the single thread that owns it.
unsafe impl Send for DoubleBuffer {}

impl DoubleBuffer {
    /// Wrap a raw SPI handle and two equal-length `'static` buffers as a double buffer.
    ///
    /// `handle` must outlive this — its `SpiDeviceDriver` must not be dropped while transfers run.
    /// The two buffers must be the same length (both are the panel's full-frame wire size) and live
    /// in DMA-capable memory (see `dma-mem`); that they are `'static` is what makes the in-flight
    /// pointer sound.
    pub fn new(handle: spi_device_handle_t, a: &'static mut [u8], b: &'static mut [u8]) -> Self {
        debug_assert_eq!(
            a.len(),
            b.len(),
            "the two wire buffers must be the same size"
        );
        Self {
            handle,
            buffers: [a, b],
            transaction: default_transaction(),
            draw: 0,
            in_flight: false,
        }
    }

    /// The idle buffer to draw the next frame into — never the one in flight.
    ///
    /// The caller fills this, then calls [`reap`](Self::reap) and [`queue`](Self::queue). `draw` only
    /// advances in `queue`, so calling this twice before a `queue` returns the same buffer.
    pub fn back(&mut self) -> &mut [u8] {
        &mut self.buffers[self.draw][..]
    }

    /// Block until the previous frame's transfer finishes, freeing its buffer for reuse.
    ///
    /// This is the barrier that hides the transfer: the previous frame's transfer was launched a
    /// whole frame ago and ran on the DMA engine while this frame was drawn, so by the time the
    /// caller reaps it here it is almost always already done and this returns at once. If drawing
    /// ever gets shorter than the transfer, this is where the render thread waits — the correct
    /// place, since it cannot reuse a buffer the DMA is still reading. A no-op before the first
    /// frame is queued.
    pub fn reap(&mut self) -> Result<(), EspError> {
        if !self.in_flight {
            return Ok(());
        }
        let mut done: *mut spi_transaction_t = ptr::null_mut();
        // SAFETY: `handle` is valid (its driver outlives us), and exactly one transfer is in flight
        // — queued by the matching `queue` — so the driver has a result to hand back. `done` is
        // written with a pointer to our own `transaction`, which we ignore.
        esp!(unsafe { spi_device_get_trans_result(self.handle, &mut done, BLOCK) })?;
        self.in_flight = false;
        Ok(())
    }

    /// Launch a non-blocking transfer of the just-drawn buffer and flip to the other one.
    ///
    /// Returns as soon as the transfer is queued; the DMA engine streams the buffer in the
    /// background while the caller draws the next frame into the now-current [`back`](Self::back).
    /// Must be preceded by a [`reap`](Self::reap) each frame, so the one-deep device queue has room
    /// and no in-flight transfer is still reading the buffer about to become idle.
    pub fn queue(&mut self) -> Result<(), EspError> {
        let buffer: &[u8] = self.buffers[self.draw];
        self.transaction = spi_transaction_t {
            length: buffer.len() * 8, // the driver counts bits, not bytes
            __bindgen_anon_1: spi_transaction_t__bindgen_ty_1 {
                tx_buffer: buffer.as_ptr() as *const _,
            },
            ..default_transaction()
        };
        // SAFETY: `handle` is valid; `transaction` lives in this struct (stable address) until the
        // matching `reap` reads its result, as the C API requires; `tx_buffer` points into a
        // `'static` buffer that stays valid and — because `draw` will now flip and `back` never
        // returns the in-flight buffer — is not written again until this transfer is reaped.
        esp!(unsafe { spi_device_queue_trans(self.handle, &mut self.transaction, BLOCK) })?;
        self.in_flight = true;
        self.draw ^= 1;
        Ok(())
    }
}

/// A zeroed transaction: no flags, no receive, length filled in per transfer. Bindgen gives
/// `spi_transaction_t` a `Default`, but the tx/rx union is spelled out so the write path is one
/// obvious place.
fn default_transaction() -> spi_transaction_t {
    spi_transaction_t {
        flags: 0,
        length: 0,
        rxlength: 0,
        __bindgen_anon_1: spi_transaction_t__bindgen_ty_1 {
            tx_buffer: ptr::null(),
        },
        __bindgen_anon_2: spi_transaction_t__bindgen_ty_2 {
            rx_buffer: ptr::null_mut(),
        },
        ..Default::default()
    }
}
