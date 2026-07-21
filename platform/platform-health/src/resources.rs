//! The one resource reading this crate judges: free heap.

/// A reading of the board's free heap, in bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Resources {
    /// Bytes free on the heap at the moment this was read.
    pub free_heap_bytes: u32,
}
