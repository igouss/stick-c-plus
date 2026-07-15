//! The ESPHome native-API **plaintext frame codec**, as pure functions over
//! byte buffers.
//!
//! A plaintext frame on the wire is
//!
//! ```text
//! ┌──────────┬──────────────────┬──────────────────┬───────────────┐
//! │ 0x00     │ varuint(len)     │ varuint(type)    │ payload[len]  │
//! │ preamble │ payload size     │ message type id  │ protobuf body │
//! └──────────┴──────────────────┴──────────────────┴───────────────┘
//! ```
//!
//! — **one** length field, and the **size comes before the type**. This is the
//! shape aioesphomeapi actually speaks; it is verified byte-for-byte against
//! frames captured from that client in `tests/golden_frames.rs`, not merely
//! asserted here. Encrypted frames use preamble `0x01` and a different,
//! fixed-width length; this module recognises `0x01` and rejects it cleanly
//! ([`FrameError::RequiresEncryption`]) so the Noise handshake (a later bead)
//! can own that path without this one guessing.
//!
//! ## Purity — the effect boundary
//! Everything here is a **pure function of buffers**: it allocates and copies,
//! but performs no I/O. The blocking `std::io` pump that turns a socket into a
//! stream of frames lives in [`crate::frame_io`], deliberately separate, so the
//! byte-level protocol stays host-testable with plain `Vec<u8>` values and the
//! effect-audit domain boundary stays meaningful.
//!
//! The varuint is the standard protobuf base-128 (LEB128) unsigned encoding,
//! **capped at 4 bytes** — the same cap aioesphomeapi enforces, which keeps any
//! decoded length below `2^28` and well above the 16-bit frame-size ceiling, so
//! a hostile peer cannot use a long varuint to force a huge allocation.

/// The plaintext preamble byte that opens every unencrypted frame.
pub const PLAINTEXT_PREAMBLE: u8 = 0x00;

/// The preamble byte that marks a Noise-encrypted frame — rejected here until
/// the encryption bead owns that path.
pub const ENCRYPTED_PREAMBLE: u8 = 0x01;

/// The largest payload a single plaintext frame may carry, in bytes.
///
/// Matches aioesphomeapi's `MAX_PLAINTEXT_FRAME_SIZE` (the firmware's 16-bit
/// wire ceiling). A length varuint above this is rejected before any allocation
/// so a malformed or hostile length cannot request an outsized buffer.
pub const MAX_FRAME_SIZE: usize = 65535;

/// The most bytes a length or type varuint may occupy. A 5th byte is a protocol
/// error, not a value — this bounds the decoded magnitude to `2^28 - 1`.
const MAX_VARUINT_BYTES: usize = 4;

/// A fully-parsed plaintext frame: the message type id and its raw protobuf
/// payload. The payload is *not* decoded here — [`crate::message_name`] resolves
/// the type and the caller's prost message decodes the bytes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Frame {
    /// The native-API message type id (see [`crate::MESSAGE_IDS`]).
    pub msg_type: u32,
    /// The protobuf-encoded message body, exactly `len` bytes.
    pub payload: Vec<u8>,
}

/// Why a byte buffer could not be decoded as a plaintext frame.
///
/// Every variant is a *protocol* fault — a peer that sent something the wire
/// format forbids. A buffer that is merely too short yet is not an error: it is
/// reported as `Ok(None)` ("need more bytes"), so a caller can keep reading.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameError {
    /// The frame opened with the encrypted preamble (`0x01`). Valid on the wire,
    /// but this codec only speaks plaintext; the Noise bead handles encryption.
    RequiresEncryption,
    /// The first byte was neither the plaintext (`0x00`) nor the encrypted
    /// (`0x01`) preamble — the stream is not a native-API frame at all.
    InvalidPreamble(u8),
    /// A length or type varuint ran past its 4-byte cap.
    VarintTooLong,
    /// The declared payload length exceeds [`MAX_FRAME_SIZE`].
    FrameTooLarge {
        /// The oversized length the peer declared.
        length: u32,
    },
}

impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FrameError::RequiresEncryption => {
                write!(
                    f,
                    "frame is Noise-encrypted (preamble 0x01); plaintext codec cannot decode it"
                )
            }
            FrameError::InvalidPreamble(b) => {
                write!(
                    f,
                    "invalid frame preamble 0x{b:02x} (expected 0x00 plaintext or 0x01 encrypted)"
                )
            }
            FrameError::VarintTooLong => {
                write!(f, "varuint exceeds the {MAX_VARUINT_BYTES}-byte limit")
            }
            FrameError::FrameTooLarge { length } => {
                write!(
                    f,
                    "frame length {length} exceeds the {MAX_FRAME_SIZE}-byte limit"
                )
            }
        }
    }
}

impl std::error::Error for FrameError {}

/// Append `value` to `out` as a base-128 (LEB128) unsigned varuint.
///
/// Little-endian groups of 7 bits, high bit set on every byte but the last;
/// `0` encodes as a single `0x00`. Identical to aioesphomeapi's
/// `_varuint_to_bytes`, so a length of `0` yields the `00` that a `PingRequest`
/// frame (`00 00 07`) carries.
pub fn write_varuint(mut value: u32, out: &mut Vec<u8>) {
    loop {
        let byte: u8 = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Read a base-128 unsigned varuint from the front of `buf`.
///
/// Returns `Ok(Some((value, consumed)))` on a complete varuint, `Ok(None)` if
/// `buf` ended mid-varuint (a short read — ask for more bytes), or
/// [`FrameError::VarintTooLong`] if it would need a 5th byte. Never panics and
/// never overflows: the 4-byte cap keeps the value under `2^28`.
fn read_varuint(buf: &[u8]) -> Result<Option<(u32, usize)>, FrameError> {
    let mut result: u32 = 0;
    let mut shift: u32 = 0;
    for (i, &byte) in buf.iter().enumerate() {
        if i >= MAX_VARUINT_BYTES {
            return Err(FrameError::VarintTooLong);
        }
        result |= ((byte & 0x7f) as u32) << shift;
        if byte & 0x80 == 0 {
            return Ok(Some((result, i + 1)));
        }
        shift += 7;
    }
    Ok(None)
}

/// Encode one plaintext frame for `msg_type` carrying `payload`, appending to
/// `out`.
///
/// Writes preamble `0x00`, the payload length as a varuint, the type as a
/// varuint, then the payload — the exact order aioesphomeapi expects.
///
/// The caller is responsible for `payload.len() <= `[`MAX_FRAME_SIZE`]; every
/// native-API message this project emits is far smaller, and the decode side
/// enforces the ceiling defensively for untrusted input.
pub fn encode_frame(msg_type: u32, payload: &[u8], out: &mut Vec<u8>) {
    out.push(PLAINTEXT_PREAMBLE);
    write_varuint(payload.len() as u32, out);
    write_varuint(msg_type, out);
    out.extend_from_slice(payload);
}

/// Encode one plaintext frame into a fresh `Vec` — the owned-buffer convenience
/// over [`encode_frame`].
pub fn encode_frame_vec(msg_type: u32, payload: &[u8]) -> Vec<u8> {
    // preamble + up to two 4-byte varuints + payload.
    let mut out: Vec<u8> = Vec::with_capacity(1 + 8 + payload.len());
    encode_frame(msg_type, payload, &mut out);
    out
}

/// Try to decode one plaintext frame from the front of `buf`.
///
/// Three outcomes, and only one of them is an error:
/// * `Ok(Some((frame, consumed)))` — a whole frame parsed; the caller should
///   drop `consumed` bytes from the front and may decode again.
/// * `Ok(None)` — `buf` holds only a *prefix* of a frame; read more and retry.
///   Truncation is not corruption, so it is not a [`FrameError`].
/// * `Err(_)` — a protocol fault (bad preamble, over-long varuint, oversized
///   length). The stream is unusable; the caller should close it.
///
/// Pure and total: any `&[u8]` yields one of the three, never a panic.
pub fn decode_frame(buf: &[u8]) -> Result<Option<(Frame, usize)>, FrameError> {
    let preamble: u8 = match buf.first() {
        Some(&b) => b,
        None => return Ok(None),
    };
    match preamble {
        PLAINTEXT_PREAMBLE => {}
        ENCRYPTED_PREAMBLE => return Err(FrameError::RequiresEncryption),
        other => return Err(FrameError::InvalidPreamble(other)),
    }

    let mut pos: usize = 1;

    let length: u32 = match read_varuint(&buf[pos..])? {
        Some((value, n)) => {
            pos += n;
            value
        }
        None => return Ok(None),
    };
    if length as usize > MAX_FRAME_SIZE {
        return Err(FrameError::FrameTooLarge { length });
    }

    let msg_type: u32 = match read_varuint(&buf[pos..])? {
        Some((value, n)) => {
            pos += n;
            value
        }
        None => return Ok(None),
    };

    let length: usize = length as usize;
    if buf.len() - pos < length {
        return Ok(None);
    }

    let payload: Vec<u8> = buf[pos..pos + length].to_vec();
    let consumed: usize = pos + length;
    Ok(Some((Frame { msg_type, payload }, consumed)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn empty_buffer_is_incomplete_not_an_error() {
        assert_eq!(decode_frame(&[]), Ok(None));
    }

    #[test]
    fn a_ping_frame_round_trips_through_the_codec() {
        // The canonical empty-payload frame: preamble, len 0, type 7.
        let bytes: Vec<u8> = encode_frame_vec(7, &[]);
        assert_eq!(bytes, vec![0x00, 0x00, 0x07]);

        let (frame, consumed): (Frame, usize) =
            decode_frame(&bytes).unwrap().expect("a whole frame");
        assert_eq!(consumed, bytes.len());
        assert_eq!(
            frame,
            Frame {
                msg_type: 7,
                payload: vec![]
            }
        );
    }

    #[test]
    fn a_payload_bearing_frame_round_trips() {
        let payload: Vec<u8> = vec![0xde, 0xad, 0xbe, 0xef];
        let bytes: Vec<u8> = encode_frame_vec(25, &payload);
        let (frame, consumed): (Frame, usize) =
            decode_frame(&bytes).unwrap().expect("a whole frame");
        assert_eq!(consumed, bytes.len());
        assert_eq!(frame.msg_type, 25);
        assert_eq!(frame.payload, payload);
    }

    #[test]
    fn two_frames_decode_one_at_a_time() {
        // Back-to-back frames in one buffer: decode consumes exactly the first.
        let mut buf: Vec<u8> = Vec::new();
        encode_frame(7, &[], &mut buf);
        encode_frame(8, &[0x01], &mut buf);

        let (first, consumed): (Frame, usize) = decode_frame(&buf).unwrap().expect("first frame");
        assert_eq!(
            first,
            Frame {
                msg_type: 7,
                payload: vec![]
            }
        );

        let (second, _): (Frame, usize) = decode_frame(&buf[consumed..])
            .unwrap()
            .expect("second frame");
        assert_eq!(
            second,
            Frame {
                msg_type: 8,
                payload: vec![0x01]
            }
        );
    }

    #[test]
    fn a_truncated_frame_asks_for_more_never_errors() {
        // A 6-byte payload declared, only 2 present: every prefix is Ok(None).
        let full: Vec<u8> = encode_frame_vec(16, &[1, 2, 3, 4, 5, 6]);
        for cut in 0..full.len() {
            assert_eq!(
                decode_frame(&full[..cut]),
                Ok(None),
                "prefix of length {cut} must be incomplete, not an error"
            );
        }
        assert!(decode_frame(&full).unwrap().is_some());
    }

    #[test]
    fn the_encrypted_preamble_is_rejected_cleanly() {
        // Preamble 0x01 belongs to the Noise bead; we must not try to parse it.
        assert_eq!(
            decode_frame(&[ENCRYPTED_PREAMBLE, 0x00, 0x01]),
            Err(FrameError::RequiresEncryption)
        );
    }

    #[test]
    fn a_foreign_preamble_is_a_typed_error() {
        assert_eq!(
            decode_frame(&[0x42, 0x00, 0x01]),
            Err(FrameError::InvalidPreamble(0x42))
        );
    }

    #[test]
    fn an_over_long_length_varuint_is_rejected() {
        // Five continuation-flagged bytes after the preamble: a 5th varuint byte.
        let buf: Vec<u8> = vec![0x00, 0x80, 0x80, 0x80, 0x80, 0x80];
        assert_eq!(decode_frame(&buf), Err(FrameError::VarintTooLong));
    }

    #[test]
    fn an_oversized_length_is_rejected_before_allocating() {
        // A well-formed 4-byte varuint for 70000 (> MAX_FRAME_SIZE), then a type.
        let mut buf: Vec<u8> = vec![0x00];
        write_varuint(70_000, &mut buf);
        write_varuint(16, &mut buf);
        assert_eq!(
            decode_frame(&buf),
            Err(FrameError::FrameTooLarge { length: 70_000 })
        );
    }

    #[test]
    fn a_length_at_the_ceiling_is_accepted() {
        // Exactly MAX_FRAME_SIZE is legal; the check rejects only *above* it.
        let payload: Vec<u8> = vec![0xAB; MAX_FRAME_SIZE];
        let bytes: Vec<u8> = encode_frame_vec(16, &payload);
        let (frame, _): (Frame, usize) = decode_frame(&bytes).unwrap().expect("a whole frame");
        assert_eq!(frame.payload.len(), MAX_FRAME_SIZE);
    }

    #[test]
    fn varuint_length_boundaries_encode_as_expected() {
        // 127 fits in one byte; 128 needs two — the boundary the frame's length
        // field crosses for payloads at 128 bytes.
        let mut one: Vec<u8> = Vec::new();
        write_varuint(127, &mut one);
        assert_eq!(one, vec![0x7f]);

        let mut two: Vec<u8> = Vec::new();
        write_varuint(128, &mut two);
        assert_eq!(two, vec![0x80, 0x01]);
    }

    proptest! {
        /// The core property: any (type, payload) encodes to a frame that
        /// decodes back to exactly itself, consuming the whole buffer. Sizes
        /// span the empty, single-byte, sub-128, and past-128 (two-byte length)
        /// ranges, plus the type-varuint boundaries.
        #[test]
        fn encode_then_decode_is_identity(
            msg_type in 0u32..=300,
            payload in prop::collection::vec(any::<u8>(), 0..512),
        ) {
            let bytes: Vec<u8> = encode_frame_vec(msg_type, &payload);
            let (frame, consumed): (Frame, usize) =
                decode_frame(&bytes).unwrap().expect("a whole frame");
            prop_assert_eq!(consumed, bytes.len());
            prop_assert_eq!(frame.msg_type, msg_type);
            prop_assert_eq!(frame.payload, payload);
        }

        /// Any strict prefix of a valid frame is incomplete, never an error and
        /// never a panic — the guarantee the blocking reader leans on to reassemble
        /// across short reads.
        #[test]
        fn every_prefix_is_incomplete(
            msg_type in 0u32..=300,
            payload in prop::collection::vec(any::<u8>(), 0..300),
        ) {
            let bytes: Vec<u8> = encode_frame_vec(msg_type, &payload);
            for cut in 0..bytes.len() {
                prop_assert_eq!(decode_frame(&bytes[..cut]), Ok(None));
            }
        }

        /// A varuint survives its own round-trip for every u32 the protocol can
        /// carry (values below 2^28, the 4-byte cap).
        #[test]
        fn varuint_round_trips(value in 0u32..(1 << 28)) {
            let mut buf: Vec<u8> = Vec::new();
            write_varuint(value, &mut buf);
            prop_assert!(buf.len() <= MAX_VARUINT_BYTES);
            let (decoded, n): (u32, usize) =
                read_varuint(&buf).unwrap().expect("a whole varuint");
            prop_assert_eq!(decoded, value);
            prop_assert_eq!(n, buf.len());
        }

        /// Decoding never panics on arbitrary bytes: it returns a frame, asks
        /// for more, or reports a typed protocol error — total over all input.
        #[test]
        fn decode_is_total_over_arbitrary_bytes(
            buf in prop::collection::vec(any::<u8>(), 0..64),
        ) {
            let _ = decode_frame(&buf);
        }
    }
}
