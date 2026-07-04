//! The blocking `std::io` **pump** that turns a byte stream into a stream of
//! plaintext frames, and back.
//!
//! This is the thin, effectful shell around the pure [`crate::frame`] codec:
//! [`FrameReader`] pulls bytes from any [`Read`] and reassembles whole
//! [`Frame`]s across short reads; [`FrameWriter`] serialises a message onto any
//! [`Write`]. All the protocol logic lives in `frame`; this module only moves
//! bytes and maps a [`FrameError`] onto [`io::Error`], so the effect boundary
//! between "understands the wire" and "touches the socket" stays crisp.
//!
//! It is deliberately transport-agnostic — a `TcpStream`, a byte pair in a test,
//! or a loopback pipe all work — because the accept loop that owns real sockets
//! is a separate firmware-infra bead.

use std::io::{self, Read, Write};

use crate::frame::{decode_frame, encode_frame, Frame, FrameError};

/// The chunk size [`FrameReader`] pulls from the underlying reader per syscall.
/// Frames are small (a `SensorStateResponse` is a dozen bytes); this only needs
/// to be large enough that a typical frame arrives in one read.
const READ_CHUNK: usize = 1024;

/// Turn a [`FrameError`] (a pure protocol fault) into an [`io::Error`] the
/// blocking pump can return. All map to [`io::ErrorKind::InvalidData`]: the
/// bytes on the wire were not a frame this codec accepts.
fn protocol_error(err: FrameError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}

/// A blocking reader of plaintext frames over any [`Read`].
///
/// Holds a small buffer so a frame split across several `read` calls (a slow
/// socket handing over one byte at a time) is reassembled transparently. Bytes
/// that belong to the *next* frame are retained, so back-to-back frames in a
/// single read are each returned in turn.
pub struct FrameReader<R> {
    inner: R,
    buf: Vec<u8>,
}

impl<R: Read> FrameReader<R> {
    /// Wrap a reader.
    pub fn new(inner: R) -> Self {
        Self { inner, buf: Vec::new() }
    }

    /// Read the next whole frame, blocking until one arrives.
    ///
    /// Returns `Ok(Some(frame))` for each frame, or `Ok(None)` on a **clean**
    /// end of stream — the peer closed the connection on a frame boundary, with
    /// no partial frame buffered. A close *mid-frame* is truncation, reported as
    /// [`io::ErrorKind::UnexpectedEof`]; a protocol fault surfaces as
    /// [`io::ErrorKind::InvalidData`].
    pub fn read_frame(&mut self) -> io::Result<Option<Frame>> {
        loop {
            match decode_frame(&self.buf) {
                Ok(Some((frame, consumed))) => {
                    self.buf.drain(..consumed);
                    return Ok(Some(frame));
                }
                Ok(None) => {} // need more bytes — fall through to read
                Err(err) => return Err(protocol_error(err)),
            }

            let mut chunk: [u8; READ_CHUNK] = [0u8; READ_CHUNK];
            let n: usize = self.inner.read(&mut chunk)?;
            if n == 0 {
                if self.buf.is_empty() {
                    return Ok(None); // clean EOF on a frame boundary
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "stream ended mid-frame",
                ));
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }
}

/// A blocking writer of plaintext frames over any [`Write`].
pub struct FrameWriter<W> {
    inner: W,
}

impl<W: Write> FrameWriter<W> {
    /// Wrap a writer.
    pub fn new(inner: W) -> Self {
        Self { inner }
    }

    /// Serialise one frame for `msg_type` carrying `payload` and flush it.
    ///
    /// The whole frame is built in one buffer and written with a single
    /// [`Write::write_all`], so a frame is never interleaved with another on a
    /// shared writer.
    pub fn write_frame(&mut self, msg_type: u32, payload: &[u8]) -> io::Result<()> {
        let mut out: Vec<u8> = Vec::with_capacity(1 + 8 + payload.len());
        encode_frame(msg_type, payload, &mut out);
        self.inner.write_all(&out)?;
        self.inner.flush()
    }

    /// The underlying writer, for a caller that needs it back (e.g. to close).
    pub fn into_inner(self) -> W {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader that hands out its bytes one at a time, forcing [`FrameReader`]
    /// to reassemble a frame across many short reads — the worst case a real
    /// slow socket produces.
    struct TrickleReader {
        bytes: Vec<u8>,
        pos: usize,
    }

    impl TrickleReader {
        fn new(bytes: Vec<u8>) -> Self {
            Self { bytes, pos: 0 }
        }
    }

    impl Read for TrickleReader {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.bytes.len() || out.is_empty() {
                return Ok(0);
            }
            out[0] = self.bytes[self.pos];
            self.pos += 1;
            Ok(1)
        }
    }

    #[test]
    fn a_frame_split_into_single_bytes_is_reassembled() {
        let frame_bytes: Vec<u8> = encode_frame_bytes(16, &[1, 2, 3, 4, 5, 6]);
        let mut reader: FrameReader<TrickleReader> =
            FrameReader::new(TrickleReader::new(frame_bytes));

        let frame: Frame = reader.read_frame().unwrap().expect("a frame");
        assert_eq!(frame.msg_type, 16);
        assert_eq!(frame.payload, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn back_to_back_frames_are_each_returned() {
        let mut bytes: Vec<u8> = encode_frame_bytes(7, &[]);
        bytes.extend(encode_frame_bytes(25, &[0xAA, 0xBB]));
        let mut reader: FrameReader<&[u8]> = FrameReader::new(bytes.as_slice());

        let first: Frame = reader.read_frame().unwrap().expect("first");
        assert_eq!(first.msg_type, 7);
        let second: Frame = reader.read_frame().unwrap().expect("second");
        assert_eq!(second, Frame { msg_type: 25, payload: vec![0xAA, 0xBB] });
    }

    #[test]
    fn a_clean_close_on_a_boundary_is_none_not_an_error() {
        let bytes: Vec<u8> = encode_frame_bytes(8, &[]);
        let mut reader: FrameReader<&[u8]> = FrameReader::new(bytes.as_slice());
        assert!(reader.read_frame().unwrap().is_some());
        // Stream is now exhausted exactly on a frame boundary.
        assert_eq!(reader.read_frame().unwrap(), None);
    }

    #[test]
    fn a_close_mid_frame_is_unexpected_eof() {
        // A frame that claims 6 payload bytes but the stream cuts off after 2.
        let full: Vec<u8> = encode_frame_bytes(16, &[1, 2, 3, 4, 5, 6]);
        let truncated: Vec<u8> = full[..full.len() - 4].to_vec();
        let mut reader: FrameReader<&[u8]> = FrameReader::new(truncated.as_slice());
        let err: io::Error = reader.read_frame().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn an_encrypted_frame_surfaces_as_invalid_data() {
        let bytes: Vec<u8> = vec![0x01, 0x00, 0x01];
        let mut reader: FrameReader<&[u8]> = FrameReader::new(bytes.as_slice());
        let err: io::Error = reader.read_frame().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn write_then_read_over_a_byte_buffer_round_trips() {
        let mut sink: Vec<u8> = Vec::new();
        {
            let mut writer: FrameWriter<&mut Vec<u8>> = FrameWriter::new(&mut sink);
            writer.write_frame(25, &[0xDE, 0xAD]).unwrap();
            writer.write_frame(7, &[]).unwrap();
        }
        let mut reader: FrameReader<&[u8]> = FrameReader::new(sink.as_slice());
        assert_eq!(
            reader.read_frame().unwrap(),
            Some(Frame { msg_type: 25, payload: vec![0xDE, 0xAD] })
        );
        assert_eq!(
            reader.read_frame().unwrap(),
            Some(Frame { msg_type: 7, payload: vec![] })
        );
        assert_eq!(reader.read_frame().unwrap(), None);
    }

    /// Local helper mirroring `frame::encode_frame_vec`, kept here so the pump's
    /// tests do not reach across module privacy.
    fn encode_frame_bytes(msg_type: u32, payload: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        encode_frame(msg_type, payload, &mut out);
        out
    }
}
