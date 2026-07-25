//! Runtime resource counters, read safely.
//!
//! # Why this crate exists
//!
//! Every other crate in both workspaces opens with `#![forbid(unsafe_code)]`, and that stays
//! true. Some figures the firmware genuinely needs — how much heap is left, how close the
//! board has ever come to exhausting it — exist only as C functions in ESP-IDF, with no safe
//! wrapper in `esp-idf-svc` or `esp-idf-hal` (checked, not assumed: neither crate mentions
//! them). Rather than sprinkle `unsafe` at each call site, all of it lives here.
//!
//! The rule for this crate: every `unsafe` block wraps exactly one C call, takes no pointer
//! from and hands no pointer to its caller, and is justified in a comment. Anything more
//! interesting than reading an integer out of the allocator does not belong here.
//!
//! # Why the numbers are worth having
//!
//! On a 520 KB no-PSRAM ESP32 the BLE stack alone costs tens of kilobytes, and the failure
//! mode is not a compile error — it is an allocation returning null deep inside a driver, at
//! runtime, on a board with nobody watching. [`largest_free_block`] is the figure that
//! actually predicts it: [`free_heap`] can read healthy while fragmentation has already made
//! a contiguous frame buffer impossible.

#![no_std]

use esp_idf_sys::{
    esp_get_free_heap_size, esp_get_minimum_free_heap_size, heap_caps_get_largest_free_block,
    uxTaskGetStackHighWaterMark, MALLOC_CAP_8BIT,
};

/// Total free heap, in bytes, across every region.
///
/// Sums all free space, so it overstates what a single allocation can get once the heap is
/// fragmented. Use [`largest_free_block`] when the question is "will this buffer fit".
pub fn free_heap() -> usize {
    // SAFETY: reads an allocator counter and returns it by value. Takes no arguments, touches
    // no memory the caller owns, and is safe to call from any task at any time.
    let bytes: u32 = unsafe { esp_get_free_heap_size() };
    bytes as usize
}

/// The lowest [`free_heap`] has ever been since boot, in bytes.
///
/// A low-water mark, so it survives the spike it records: a transient that nearly exhausted
/// the heap during startup is invisible to [`free_heap`] a second later but shows up here.
pub fn minimum_free_heap() -> usize {
    // SAFETY: as `free_heap` — an argument-free read of an allocator counter, returned by
    // value.
    let bytes: u32 = unsafe { esp_get_minimum_free_heap_size() };
    bytes as usize
}

/// The largest single allocation the heap can still serve, in bytes.
///
/// Byte-addressable memory only (`MALLOC_CAP_8BIT`), which is what a `Vec<u8>`, a frame
/// buffer, or a DMA-less driver buffer draws from.
pub fn largest_free_block() -> usize {
    // SAFETY: `heap_caps_get_largest_free_block` takes a capability bitmask by value and
    // returns a size. `MALLOC_CAP_8BIT` is a constant from the same generated bindings, so the
    // mask is valid by construction; the call allocates nothing and dereferences nothing.
    unsafe { heap_caps_get_largest_free_block(MALLOC_CAP_8BIT) }
}

/// The least free stack, in bytes, the **calling task** has ever had since it started.
///
/// A low-water mark on the calling thread's own stack — the stack analogue of
/// [`minimum_free_heap`]. Call it *from* the task you want to size: pass a stack that is too
/// generous and this reports how much of it was never touched, which is exactly the slack a
/// bring-up guess can be trimmed by. The smaller the number, the closer that task has come to
/// overflowing.
///
/// The returned figure is **bytes**, not words. The generic FreeRTOS header comment says
/// "words", but that division is by `sizeof(StackType_t)`, and on this Xtensa port
/// `StackType_t` is a single byte — so the raw count is already a byte count here. (Verified
/// against the generated bindings, not assumed.)
pub fn minimum_free_stack() -> usize {
    // SAFETY: `uxTaskGetStackHighWaterMark` takes a task handle by value and returns a count. A
    // null handle selects the calling task — it dereferences no pointer the caller owns, hands
    // back no pointer, allocates nothing, and is safe to call from any task at any time.
    let bytes: u32 = unsafe { uxTaskGetStackHighWaterMark(core::ptr::null_mut()) };
    bytes as usize
}
