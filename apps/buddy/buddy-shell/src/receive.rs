//! The receive path: raw GATT bytes in, folded state out.
//!
//! A BLE write arrives fragmented at the MTU boundary with no framing of its own, so bytes
//! accumulate in a [`Framer`] until a terminator, each complete line is classified by
//! [`parse_inbound`], and only then does it reach [`DeviceState::apply`].
//!
//! ## Every failure is reported and skipped
//!
//! Three things can go wrong with a chunk — the line overran the buffer, it was not JSON, or the
//! time array had the wrong arity — and all three are *logged and dropped*. None of them may
//! reach the state, and in particular none of them may be treated as a snapshot: upstream's
//! wrong-arity time sync fell through into the snapshot merge and cleared a pending prompt off
//! the glass. The classification is what stops that, and this module's job is to not undo it by
//! being clever about a malformed line.

use buddy_wire::{parse_inbound, FrameError, Framer, Inbound};
use log::warn;
use platform_core::Tick;

use crate::shared::SharedDevice;
use crate::state::DeviceState;

/// The receive path for one link: a framer, and where the lines go.
pub struct Receiver {
    framer: Framer,
    shared: SharedDevice,
}

impl Receiver {
    /// A receiver feeding `shared`.
    pub fn new(shared: SharedDevice) -> Self {
        Receiver {
            framer: Framer::new(),
            shared,
        }
    }

    /// Feed one chunk of raw bytes, folding every complete line it completes.
    ///
    /// Returns how many lines actually reached the state, so the transport can log a chunk that
    /// carried nothing usable rather than silently doing nothing with it.
    pub fn feed(&mut self, chunk: &[u8], now: Tick) -> usize {
        let lines: Vec<Vec<u8>> = match self.framer.push(chunk) {
            Ok(lines) => lines,
            Err(FrameError::LineTooLong) => {
                warn!("buddy-receive: a line overran the buffer and was dropped");
                return 0;
            }
            Err(error) => {
                warn!("buddy-receive: {error}");
                return 0;
            }
        };

        lines
            .iter()
            .filter_map(|line: &Vec<u8>| match parse_inbound(line) {
                Ok(inbound) => Some(inbound),
                Err(error) => {
                    // Dropped, never guessed at. A line that cannot be classified is emphatically
                    // not a snapshot, and treating it as one is how a keepalive wipes a prompt.
                    warn!("buddy-receive: unusable line dropped: {error}");
                    None
                }
            })
            .map(|inbound: Inbound| {
                self.shared
                    .with(|state: &mut DeviceState| state.apply(inbound, now));
            })
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use buddy_core::SpeciesIndex;
    use platform_core::Acceleration;

    use crate::identity::Identity;

    const REST: Acceleration = Acceleration::new(0, 0, 1_000);

    fn receiver() -> (Receiver, SharedDevice) {
        let shared: SharedDevice = SharedDevice::new(DeviceState::new(
            Identity::new("Claude-4F2A", "0.1.0", "A0:B7:65:4F:2A:11"),
            SpeciesIndex::new(0),
        ));
        (Receiver::new(shared.clone()), shared)
    }

    /// A snapshot carrying a prompt, as one complete line.
    const PROMPT_LINE: &[u8] =
        br#"{"waiting":1,"prompt":{"id":"p1","tool":"Bash","hint":"ls -la"}}"#;

    /// Zero: a chunk with no terminator completes no line yet.
    #[test]
    fn a_chunk_without_a_terminator_completes_nothing() {
        let (mut receiver, _shared): (Receiver, SharedDevice) = receiver();
        assert_eq!(receiver.feed(PROMPT_LINE, 1_000), 0);
    }

    /// One: a terminated line reaches the state.
    #[test]
    fn a_terminated_line_reaches_the_state() {
        let (mut receiver, shared): (Receiver, SharedDevice) = receiver();
        assert_eq!(receiver.feed(b"{\"running\":2}\n", 1_000), 1);
        assert_eq!(shared.tick_and_view(1_000, REST, true).sessions_running, 2);
    }

    /// Many: a line split across MTU-sized chunks is reassembled — the case a BLE write always
    /// produces and a naive receiver always gets wrong.
    #[test]
    fn a_line_split_across_chunks_is_reassembled() {
        let (mut receiver, shared): (Receiver, SharedDevice) = receiver();
        let (head, tail): (&[u8], &[u8]) = PROMPT_LINE.split_at(20);
        assert_eq!(receiver.feed(head, 1_000), 0);
        assert_eq!(receiver.feed(tail, 1_000), 0);
        assert_eq!(receiver.feed(b"\n", 1_000), 1);
        assert!(shared.tick_and_view(1_000, REST, true).prompt.is_some());
    }

    /// A malformed line is dropped rather than guessed at — and, crucially, does NOT reach the
    /// state as a snapshot, which is how a bad line would wipe a pending prompt.
    #[test]
    fn a_malformed_line_is_dropped_and_leaves_the_prompt_alone() {
        let (mut receiver, shared): (Receiver, SharedDevice) = receiver();
        receiver.feed(PROMPT_LINE, 1_000);
        receiver.feed(b"\n", 1_000);
        assert_eq!(receiver.feed(b"{not json at all}\n", 2_000), 0);
        assert_eq!(receiver.feed(b"{\"time\":[1]}\n", 3_000), 0);
        assert!(
            shared.tick_and_view(3_000, REST, true).prompt.is_some(),
            "a malformed line reached the snapshot merge and cleared the prompt"
        );
    }

    /// Two lines in one chunk both land — a busy link really does deliver them that way.
    #[test]
    fn two_lines_in_one_chunk_both_land() {
        let (mut receiver, shared): (Receiver, SharedDevice) = receiver();
        assert_eq!(
            receiver.feed(b"{\"running\":1}\n{\"running\":7}\n", 1_000),
            2
        );
        assert_eq!(shared.tick_and_view(1_000, REST, true).sessions_running, 7);
    }
}
