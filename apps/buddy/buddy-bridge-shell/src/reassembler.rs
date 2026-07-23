//! Turn fragmented TX notifications back into whole JSON lines.
//!
//! GATT provides no framing: the device chunks a line at `mtu - 3` and (upstream C++) even
//! sends the terminating `\n` as its own notification. Rather than re-derive a line splitter,
//! this reuses `buddy-wire`'s [`Ring`] + [`Framer`] — the single, host-tested source of the
//! framing rules (terminator `\n`|`\r`, leading `{`, typed truncation via [`FrameError`]). A
//! `role=domain` crate may not depend on `buddy-wire` (it points outward), so the reuse lives
//! here in the driving-adapter, exactly where the raw bytes arrive.

use buddy_wire::{FrameError, Framer, Ring};

/// Accumulates raw notification payloads and yields the complete lines they frame.
#[derive(Debug, Default)]
pub struct RxReassembler {
    ring: Ring,
    framer: Framer,
}

impl RxReassembler {
    /// A fresh reassembler.
    pub fn new() -> Self {
        RxReassembler {
            ring: Ring::new(),
            framer: Framer::new(),
        }
    }

    /// Accept one raw notification payload; return every whole line it completed.
    ///
    /// The path is the one the wire crate models: bytes → [`Ring`] (bounded, typed overflow) →
    /// [`Framer`] (line rules, typed over-length). A truncation is surfaced as a
    /// [`FrameError`], never swallowed. A lone `\n` notification simply flushes the pending
    /// line — the device's separate-terminator quirk needs no special handling.
    pub fn accept(&mut self, notification: &[u8]) -> Result<Vec<Vec<u8>>, FrameError> {
        self.ring.push(notification)?;
        let bytes: Vec<u8> = self.ring.drain();
        self.framer.push(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_terminator_yet_yields_no_line() {
        let mut rx: RxReassembler = RxReassembler::new();
        let lines: Vec<Vec<u8>> = rx.accept(b"{\"alive\":1}").expect("within capacity");
        assert!(
            lines.is_empty(),
            "a line without its terminator is still pending"
        );
    }

    #[test]
    fn a_line_fragmented_then_a_lone_newline_is_one_line() {
        // The load-bearing device behavior: chunked body across notifications, then the
        // terminating `\n` as its OWN notification.
        let mut rx: RxReassembler = RxReassembler::new();
        assert!(rx.accept(b"{\"al").expect("fits").is_empty());
        assert!(rx.accept(b"ive\":42}").expect("fits").is_empty());
        let lines: Vec<Vec<u8>> = rx.accept(b"\n").expect("fits");
        assert_eq!(lines, vec![b"{\"alive\":42}".to_vec()]);
    }

    #[test]
    fn two_lines_split_arbitrarily_reassemble_to_two() {
        let mut rx: RxReassembler = RxReassembler::new();
        assert!(rx.accept(b"{\"a\":1}\n{\"b\":").expect("fits").len() == 1);
        let lines: Vec<Vec<u8>> = rx.accept(b"2}\n").expect("fits");
        assert_eq!(lines, vec![b"{\"b\":2}".to_vec()]);
    }

    #[test]
    fn an_over_length_line_surfaces_a_typed_error_not_a_silent_drop() {
        let mut rx: RxReassembler = RxReassembler::new();
        // buddy_wire::LINE_CAPACITY is 1023; push 1024 non-terminator body bytes past `{`.
        let mut body: Vec<u8> = vec![b'{'];
        body.extend(std::iter::repeat_n(b'x', buddy_wire::LINE_CAPACITY));
        let err: FrameError = rx.accept(&body).expect_err("must surface truncation");
        assert_eq!(err, FrameError::LineTooLong);
    }

    #[test]
    fn a_ring_overflow_surfaces_a_typed_error() {
        let mut rx: RxReassembler = RxReassembler::new();
        // RING_CAPACITY is 2047; one push past it, with no terminator to drain it, overflows.
        let too_much: Vec<u8> = vec![b'z'; buddy_wire::RING_CAPACITY + 1];
        let err: FrameError = rx.accept(&too_much).expect_err("must surface the overflow");
        assert_eq!(err, FrameError::RxOverflow);
    }
}
