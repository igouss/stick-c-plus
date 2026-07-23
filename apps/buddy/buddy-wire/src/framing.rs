//! Line framing over the NUS byte stream, with truncation made a **typed error**.
//!
//! Two upstream decoders (USB serial and BLE) share one set of rules, reproduced here:
//! 1. the terminator is `\n` **or** `\r` (a CRLF yields one line plus one empty);
//! 2. empty lines are dropped silently;
//! 3. `line[0]` must be `{` — **no trimming**, so a single leading space kills the message;
//! 4. the line buffer holds [`LINE_CAPACITY`] usable bytes.
//!
//! ## Defect fix (c): truncation is never a silent drop
//!
//! Upstream truncates in three places with zero diagnostics — the BLE ring drops the tail of a
//! GATT write when full, the line buffer drops its tail without resetting, and the truncated
//! remnant then fails JSON parsing and vanishes. Here each becomes a typed [`FrameError`]:
//! [`FrameError::LineTooLong`] for the line buffer, [`FrameError::RxOverflow`] for the ring.
//! Callers get a `Result`, not a swallow.

/// The usable line-buffer capacity, in bytes: a line longer than this is
/// [`FrameError::LineTooLong`], never a silent truncation.
pub const LINE_CAPACITY: usize = 1023;

/// The usable BLE receive-ring capacity, in bytes: pushing past this is
/// [`FrameError::RxOverflow`], never a silent mid-line drop.
pub const RING_CAPACITY: usize = 2047;

/// Why framing could not accept some bytes — the truncation the upstream swallowed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameError {
    /// A line exceeded [`LINE_CAPACITY`] before its terminator — the line buffer would have
    /// dropped the tail. Reported instead of truncating.
    LineTooLong,
    /// The BLE receive ring was full and could not hold the pushed bytes — a GATT write would
    /// have been dropped mid-line. Reported instead of dropping.
    RxOverflow,
}

impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FrameError::LineTooLong => write!(f, "line exceeded {LINE_CAPACITY} bytes"),
            FrameError::RxOverflow => write!(f, "receive ring exceeded {RING_CAPACITY} bytes"),
        }
    }
}

impl std::error::Error for FrameError {}

/// A line framer: feeds raw bytes in, yields complete accepted lines out.
///
/// Accumulates bytes until a `\n` or `\r`, drops empty lines, and enforces the leading-`{`
/// rule. Overrunning [`LINE_CAPACITY`] before a terminator is [`FrameError::LineTooLong`].
#[derive(Debug, Default)]
pub struct Framer {
    line: Vec<u8>,
}

impl Framer {
    /// A fresh, empty framer.
    pub fn new() -> Self {
        Framer { line: Vec::new() }
    }

    /// Feed a chunk of raw bytes; return every complete, accepted line it completed.
    ///
    /// A returned line is non-empty and begins with `{` (leading whitespace is not trimmed, so
    /// a space-prefixed line is dropped as invalid, matching upstream). Errors with
    /// [`FrameError::LineTooLong`] if the pending line would exceed [`LINE_CAPACITY`] before a
    /// terminator — the truncation upstream did silently. On that error the pending line is
    /// discarded so the framer stays usable for the next terminator.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, FrameError> {
        let mut lines: Vec<Vec<u8>> = Vec::new();
        for &byte in chunk {
            if byte == b'\n' || byte == b'\r' {
                if !self.line.is_empty() && self.line[0] == b'{' {
                    lines.push(std::mem::take(&mut self.line));
                } else {
                    // Empty lines, and lines not starting with `{`, are dropped silently.
                    self.line.clear();
                }
            } else if self.line.len() >= LINE_CAPACITY {
                // The 1024th non-terminator byte would overflow the usable buffer. Upstream
                // dropped the tail with zero diagnostics; here it is a typed error (fix c).
                self.line.clear();
                return Err(FrameError::LineTooLong);
            } else {
                self.line.push(byte);
            }
        }
        Ok(lines)
    }
}

/// A bounded byte ring modelling the BLE receive buffer, upstream of the [`Framer`].
///
/// Pushing more than [`RING_CAPACITY`] free bytes is [`FrameError::RxOverflow`] — the drop the
/// upstream did silently on a bare `return`.
#[derive(Debug, Default)]
pub struct Ring {
    buf: std::collections::VecDeque<u8>,
}

impl Ring {
    /// A fresh, empty ring.
    pub fn new() -> Self {
        Ring {
            buf: std::collections::VecDeque::new(),
        }
    }

    /// Push bytes into the ring; error with [`FrameError::RxOverflow`] if they do not fit.
    ///
    /// Upstream did a bare `return` on a full ring, dropping the remainder of a GATT write
    /// mid-line with no signal; here the overflow is a typed error (fix c). The push is
    /// all-or-nothing: on overflow no bytes are appended.
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), FrameError> {
        if self.buf.len() + bytes.len() > RING_CAPACITY {
            return Err(FrameError::RxOverflow);
        }
        self.buf.extend(bytes.iter().copied());
        Ok(())
    }

    /// Drain all buffered bytes (to feed the [`Framer`]).
    pub fn drain(&mut self) -> Vec<u8> {
        self.buf.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ---- Framer: the leading-`{` + terminator rules --------------------------------------

    #[test]
    fn no_terminator_yet_yields_no_lines() {
        let mut framer: Framer = Framer::new();
        let lines: Vec<Vec<u8>> = framer.push(b"{\"a\":1}").expect("no overflow");
        assert!(
            lines.is_empty(),
            "a line without a terminator is still pending"
        );
    }

    #[test]
    fn one_newline_terminated_object_is_one_line() {
        let mut framer: Framer = Framer::new();
        let lines: Vec<Vec<u8>> = framer.push(b"{\"a\":1}\n").expect("no overflow");
        assert_eq!(lines, vec![b"{\"a\":1}".to_vec()]);
    }

    #[test]
    fn a_carriage_return_also_terminates() {
        let mut framer: Framer = Framer::new();
        let lines: Vec<Vec<u8>> = framer.push(b"{\"a\":1}\r").expect("no overflow");
        assert_eq!(lines, vec![b"{\"a\":1}".to_vec()]);
    }

    #[test]
    fn a_crlf_pair_yields_one_line_and_one_dropped_empty() {
        let mut framer: Framer = Framer::new();
        let lines: Vec<Vec<u8>> = framer.push(b"{\"a\":1}\r\n").expect("no overflow");
        assert_eq!(
            lines,
            vec![b"{\"a\":1}".to_vec()],
            "the empty second line is dropped"
        );
    }

    #[test]
    fn two_objects_in_one_chunk_are_two_lines() {
        let mut framer: Framer = Framer::new();
        let lines: Vec<Vec<u8>> = framer.push(b"{\"a\":1}\n{\"b\":2}\n").expect("no overflow");
        assert_eq!(lines, vec![b"{\"a\":1}".to_vec(), b"{\"b\":2}".to_vec()]);
    }

    #[test]
    fn an_empty_line_is_dropped_silently() {
        let mut framer: Framer = Framer::new();
        let lines: Vec<Vec<u8>> = framer.push(b"\n").expect("no overflow");
        assert!(lines.is_empty());
    }

    #[test]
    fn a_leading_space_kills_the_message() {
        // No trimming: a single leading space fails the `line[0] == '{'` rule.
        let mut framer: Framer = Framer::new();
        let lines: Vec<Vec<u8>> = framer.push(b" {\"a\":1}\n").expect("no overflow");
        assert!(lines.is_empty(), "a space-prefixed line is not accepted");
    }

    #[test]
    fn a_line_split_across_two_chunks_reassembles() {
        let mut framer: Framer = Framer::new();
        let first: Vec<Vec<u8>> = framer.push(b"{\"a\":").expect("no overflow");
        assert!(first.is_empty());
        let second: Vec<Vec<u8>> = framer.push(b"1}\n").expect("no overflow");
        assert_eq!(second, vec![b"{\"a\":1}".to_vec()]);
    }

    // ---- Defect fix (c): truncation is a TYPED error, never a silent drop -----------------

    #[test]
    fn a_line_at_capacity_still_completes() {
        // Exactly 1023 usable bytes plus the terminator is accepted, not an error.
        let mut body: Vec<u8> = vec![b'{'];
        body.extend(std::iter::repeat_n(b'x', LINE_CAPACITY - 1));
        let mut chunk: Vec<u8> = body.clone();
        chunk.push(b'\n');
        let mut framer: Framer = Framer::new();
        let lines: Vec<Vec<u8>> = framer.push(&chunk).expect("a full-capacity line is fine");
        assert_eq!(lines, vec![body]);
    }

    #[test]
    fn overrunning_the_line_buffer_is_a_typed_error_not_a_silent_truncation() {
        // 1024 non-terminator bytes: the upstream would have dropped the tail with zero
        // diagnostics; here it is LineTooLong.
        let mut chunk: Vec<u8> = vec![b'{'];
        chunk.extend(std::iter::repeat_n(b'x', LINE_CAPACITY));
        let mut framer: Framer = Framer::new();
        let err: FrameError = framer.push(&chunk).expect_err("must surface truncation");
        assert_eq!(err, FrameError::LineTooLong);
    }

    // ---- Ring: bounded, typed overflow ---------------------------------------------------

    #[test]
    fn the_ring_round_trips_bytes_through_drain() {
        let mut ring: Ring = Ring::new();
        ring.push(b"hello").expect("fits");
        assert_eq!(ring.drain(), b"hello".to_vec());
    }

    #[test]
    fn draining_an_empty_ring_yields_nothing() {
        let mut ring: Ring = Ring::new();
        assert!(ring.drain().is_empty());
    }

    #[test]
    fn a_ring_filled_to_capacity_accepts_the_bytes() {
        let mut ring: Ring = Ring::new();
        let full: Vec<u8> = vec![b'z'; RING_CAPACITY];
        ring.push(&full).expect("exactly capacity fits");
        assert_eq!(ring.drain().len(), RING_CAPACITY);
    }

    #[test]
    fn overrunning_the_ring_is_a_typed_error_not_a_silent_mid_line_drop() {
        let mut ring: Ring = Ring::new();
        let too_much: Vec<u8> = vec![b'z'; RING_CAPACITY + 1];
        let err: FrameError = ring.push(&too_much).expect_err("must surface the overflow");
        assert_eq!(err, FrameError::RxOverflow);
    }

    #[test]
    fn a_ring_overflow_appends_nothing() {
        let mut ring: Ring = Ring::new();
        ring.push(&vec![b'a'; RING_CAPACITY - 1]).expect("fits");
        let _ = ring.push(b"bb").expect_err("2 bytes do not fit in 1 free");
        // All-or-nothing: the failed push left the buffer at its prior length.
        assert_eq!(ring.drain().len(), RING_CAPACITY - 1);
    }

    // ---- Properties ----------------------------------------------------------------------

    proptest! {
        // Any object body (no terminator bytes, within capacity) round-trips as exactly one
        // accepted line when newline-terminated.
        #[test]
        fn a_terminated_object_round_trips(body in proptest::collection::vec(1u8..=254, 0..1000)
            .prop_filter("no terminators", |v: &Vec<u8>| !v.contains(&b'\n') && !v.contains(&b'\r'))) {
            let mut line: Vec<u8> = vec![b'{'];
            line.extend_from_slice(&body);
            let mut chunk: Vec<u8> = line.clone();
            chunk.push(b'\n');
            let mut framer: Framer = Framer::new();
            let lines: Vec<Vec<u8>> = framer.push(&chunk).expect("within capacity");
            prop_assert_eq!(lines, vec![line]);
        }

        // Framing never panics on arbitrary bytes — it returns Ok or a typed Err.
        #[test]
        fn framing_never_panics_on_garbage(chunk in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let mut framer: Framer = Framer::new();
            let _: Result<Vec<Vec<u8>>, FrameError> = framer.push(&chunk);
        }

        // The ring never panics, and its overflow decision matches the capacity bound.
        #[test]
        fn the_ring_overflow_bound_holds(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let mut ring: Ring = Ring::new();
            let result: Result<(), FrameError> = ring.push(&bytes);
            prop_assert_eq!(result.is_ok(), bytes.len() <= RING_CAPACITY);
        }
    }
}
