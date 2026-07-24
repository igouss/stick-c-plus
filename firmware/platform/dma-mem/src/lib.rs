//! DMA-capable memory, allocated once and leaked to `'static`.
//!
//! # Why this crate exists
//!
//! Every other crate in both workspaces opens with `#![forbid(unsafe_code)]`, and that stays
//! true. This is the second crate (beside `esp-metrics`) allowed to speak raw C — and the only
//! one that hands a caller a buffer.
//!
//! The ESP32's SPI DMA engine can only read from **DMA-capable internal SRAM**. A `static`
//! array (`.bss`) or a `Vec` from the global allocator may land in a region the DMA engine
//! cannot reach — the ESP32 has D/IRAM banks that are byte-addressable to the CPU but not valid
//! DMA sources. Handing such a buffer to a DMA transfer does not fail cleanly: it corrupts
//! whatever the descriptor points at and the board double-faults deep in the scheduler on the
//! next tick. That was watched, once: a 63 KiB `StaticCell` panel buffer, DMA on, `LoadProhibited`
//! in `xTaskIncrementTickOtherCores` before the first frame. The fix is not to guess where `.bss`
//! landed but to *ask* ESP-IDF for memory with the DMA capability — which is all this crate does.
//!
//! # The rule for this crate
//!
//! One `unsafe` region, wrapping one heap C call and the slice it returns. The buffer is never
//! freed — it lives for the whole program (leaked), which is what makes the `'static` lifetime
//! sound and suits its use: a display's frame buffer is built once at bring-up and reused every
//! frame. Nothing more interesting than a checked allocation belongs here.

#![no_std]

use core::slice;

use esp_idf_sys::{heap_caps_malloc, MALLOC_CAP_8BIT, MALLOC_CAP_DMA};

/// Allocate `len` bytes of zeroed, DMA-capable internal RAM, leaked to `'static`.
///
/// Returns `None` when the allocation fails — there is not a contiguous `len`-byte block of
/// DMA-capable RAM free. On a 520 KiB no-PSRAM ESP32 that is a real outcome under memory
/// pressure (the BLE stack alone costs tens of kilobytes), so the caller must handle it rather
/// than assume success; a null from the allocator is data, not a panic.
///
/// The returned buffer is byte-addressable ([`MALLOC_CAP_8BIT`]) and word-aligned, as
/// `heap_caps_malloc` guarantees for a DMA capability — the alignment an SPI DMA source needs.
pub fn dma_buffer(len: usize) -> Option<&'static mut [u8]> {
    // SAFETY: `heap_caps_malloc(len, caps)` returns either a valid pointer to `len` bytes of
    // memory carrying the requested capabilities, or null. We:
    //   - request `MALLOC_CAP_DMA | MALLOC_CAP_8BIT`, both constants from the same generated
    //     bindings, so the mask is valid by construction;
    //   - test for null and bail before forming any reference;
    //   - build a slice of *exactly* `len` bytes over the freshly returned region, which no
    //     other reference aliases (this allocation has never been handed out before);
    //   - never free it, so the `'static` lifetime cannot outlive the backing memory.
    // The memory is uninitialised by `malloc`, so we zero it through the unique reference before
    // returning it.
    let ptr: *mut u8 =
        unsafe { heap_caps_malloc(len, MALLOC_CAP_DMA | MALLOC_CAP_8BIT) as *mut u8 };
    if ptr.is_null() {
        return None;
    }
    let buffer: &'static mut [u8] = unsafe { slice::from_raw_parts_mut(ptr, len) };
    buffer.fill(0);
    Some(buffer)
}
