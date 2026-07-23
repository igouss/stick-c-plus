//! The TX MTU chunker — how a framed line is split for the negotiated ATT MTU.
//!
//! GATT writes (and notifications) carry at most `mtu - 3` payload bytes; the firmware chunks
//! its notifications exactly this way (`notify_chunked`, Handoff 1 finding 2), and the central's
//! RX writes must match. This is the whole policy, pure and borrow-only: it slices the payload
//! in place, so the caller frames the line (appends `\n`) and then writes each piece.

/// Split `payload` into GATT-sized pieces for the negotiated `mtu`.
///
/// Each piece is at most `mtu - 3` bytes; an `mtu` of 0..=3 clamps the piece size to 1 (never a
/// zero-length slice, matching the firmware's `.max(1)`), so a tiny or bogus MTU still makes
/// progress rather than looping on empty writes. An empty payload yields no pieces. The pieces
/// borrow `payload` and, concatenated, reconstruct it exactly.
pub fn chunk(payload: &[u8], mtu: u16) -> Vec<&[u8]> {
    let piece: usize = usize::from(mtu).saturating_sub(3).max(1);
    payload.chunks(piece).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn an_empty_payload_yields_no_pieces() {
        let pieces: Vec<&[u8]> = chunk(b"", 517);
        assert!(pieces.is_empty());
    }

    #[test]
    fn a_payload_within_one_piece_is_a_single_write() {
        // mtu 23 -> 20-byte pieces; 20 bytes fit in one.
        let pieces: Vec<&[u8]> = chunk(b"01234567890123456789", 23);
        assert_eq!(pieces.len(), 1);
    }

    #[test]
    fn a_payload_over_the_piece_size_splits_into_many() {
        // mtu 23 -> 20-byte pieces; 41 bytes -> 20 + 20 + 1.
        let payload: Vec<u8> = vec![b'x'; 41];
        let pieces: Vec<&[u8]> = chunk(&payload, 23);
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0].len(), 20);
        assert_eq!(pieces[1].len(), 20);
        assert_eq!(pieces[2].len(), 1);
    }

    #[test]
    fn a_degenerate_mtu_clamps_the_piece_to_one_byte() {
        // mtu 3 -> saturating_sub(3) = 0 -> clamped to 1; three 1-byte pieces, no empty slice.
        let pieces: Vec<&[u8]> = chunk(b"abc", 3);
        assert_eq!(pieces.len(), 3);
        assert!(pieces.iter().all(|piece: &&[u8]| piece.len() == 1));
    }

    proptest! {
        /// Concatenating the pieces reconstructs the payload, and no piece exceeds `mtu - 3`
        /// (or the 1-byte clamp) — for any payload and any MTU.
        #[test]
        fn pieces_reassemble_and_respect_the_bound(
            payload in proptest::collection::vec(any::<u8>(), 0..2048),
            mtu in any::<u16>(),
        ) {
            let bound: usize = usize::from(mtu).saturating_sub(3).max(1);
            let pieces: Vec<&[u8]> = chunk(&payload, mtu);
            let rejoined: Vec<u8> = pieces.concat();
            prop_assert_eq!(rejoined.as_slice(), payload.as_slice());
            prop_assert!(pieces.iter().all(|piece: &&[u8]| !piece.is_empty() && piece.len() <= bound));
        }
    }
}
