//! Length-prefixed framing (Design doc 20 §5, §9; D-087).
//!
//! A frame is a `u32` little-endian body length followed by the body: one message encoded
//! with `postcard`. The length counts the body only. Frames longer than the limit for their
//! direction are rejected as soon as the header arrives, before the body is buffered.
//!
//! Decoding works on byte buffers, so it is independent of the transport (QUIC streams
//! arrive with the server and the client, doc 20 §5). [`FrameDecoder`] accepts input in
//! pieces of any size, as a stream delivers it.

use std::marker::PhantomData;
use std::ops::Range;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::{MalformedBody, ProtocolError};

/// Bytes in a frame header: the body length as `u32` little-endian.
pub const HEADER_LEN: usize = 4;

/// The largest body any frame may have, in bytes: the server-to-client limit.
///
/// *Proposed (doc 20 §9 asks for a maximum per message type):* 16 MiB. A full view of a
/// stage-2 Universe is far smaller; the cap only bounds what a broken or hostile peer can
/// make the other side buffer.
pub const MAX_FRAME_LEN: u32 = MAX_SERVER_FRAME_LEN;

/// The largest body of a server-to-client frame: 16 MiB. *Proposed* (doc 20 §9).
pub const MAX_SERVER_FRAME_LEN: u32 = 16 * 1024 * 1024;

/// The largest body of a client-to-server frame: 64 KiB. Client messages are handshakes and
/// single commands, so they stay small. *Proposed* (doc 20 §9).
pub const MAX_CLIENT_FRAME_LEN: u32 = 64 * 1024;

/// A message type that travels in frames: one enum per direction (doc 20 §5).
pub trait WireMessage: Serialize + DeserializeOwned {
    /// The largest body a frame of this direction may have, in bytes. At most
    /// [`MAX_FRAME_LEN`].
    const MAX_FRAME_LEN: u32;
}

/// Appends `message` to `out` as one frame.
///
/// Fails with [`ProtocolError::Oversized`] if the body would exceed the limit of the
/// message's direction; `out` is unchanged then.
pub fn encode_frame<M: WireMessage>(message: &M, out: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let body = postcard::to_stdvec(message).map_err(ProtocolError::Encode)?;
    let len = u32::try_from(body.len())
        .ok()
        .filter(|&len| len <= M::MAX_FRAME_LEN)
        .ok_or(ProtocolError::Oversized {
            len: u64::try_from(body.len()).unwrap_or(u64::MAX),
            max: M::MAX_FRAME_LEN,
        })?;
    out.reserve(HEADER_LEN + body.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&body);
    Ok(())
}

/// Decodes the frame at the start of `bytes`. Returns the message and the number of bytes
/// the frame took, header included.
///
/// Fails with [`ProtocolError::Truncated`] if `bytes` ends inside the frame. For input that
/// arrives in pieces, use [`FrameDecoder`].
pub fn decode_frame<M: WireMessage>(bytes: &[u8]) -> Result<(M, usize), ProtocolError> {
    let truncated = |needed| ProtocolError::Truncated {
        needed,
        available: bytes.len(),
    };
    let (header, rest) = bytes
        .split_first_chunk::<HEADER_LEN>()
        .ok_or_else(|| truncated(HEADER_LEN))?;
    let len = body_len(*header, M::MAX_FRAME_LEN)?;
    let body = rest.get(..len).ok_or_else(|| truncated(HEADER_LEN + len))?;
    Ok((decode_body(body)?, HEADER_LEN + len))
}

/// Decodes a frame body that must hold exactly one message.
pub fn decode_body<M: DeserializeOwned>(body: &[u8]) -> Result<M, ProtocolError> {
    let (message, rest) = postcard::take_from_bytes(body).map_err(ProtocolError::decode)?;
    if rest.is_empty() {
        Ok(message)
    } else {
        Err(ProtocolError::Malformed(MalformedBody::TrailingBytes(
            rest.len(),
        )))
    }
}

/// Reads a frame header and checks the body length against `max`.
fn body_len(header: [u8; HEADER_LEN], max: u32) -> Result<usize, ProtocolError> {
    let len = u32::from_le_bytes(header);
    let oversized = ProtocolError::Oversized {
        len: u64::from(len),
        max,
    };
    if len > max {
        return Err(oversized);
    }
    // Infallible on every supported target (usize has at least 32 bits).
    usize::try_from(len).map_err(|_| oversized)
}

/// Splits a byte stream into messages of type `M`, accepting input in pieces of any size.
///
/// Push what the transport delivers with [`push`](Self::push), then call
/// [`next_message`](Self::next_message) until it returns `Ok(None)` before pushing more.
/// Used like that, the decoder buffers at most one incomplete frame, which is bounded by
/// `M::MAX_FRAME_LEN`, plus the last piece pushed.
///
/// After [`ProtocolError::Oversized`] the decoder has lost the framing: it drops its buffer,
/// ignores further input and returns that error from every call. After
/// [`ProtocolError::Malformed`] the bad frame is consumed and decoding can continue.
#[derive(Debug)]
pub struct FrameDecoder<M> {
    buffer: Vec<u8>,
    /// Bytes at the front of `buffer` that were already decoded.
    consumed: usize,
    /// Set once the framing is lost.
    failed: Option<ProtocolError>,
    message: PhantomData<fn() -> M>,
}

impl<M: WireMessage> Default for FrameDecoder<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: WireMessage> FrameDecoder<M> {
    /// An empty decoder.
    pub const fn new() -> Self {
        Self {
            buffer: Vec::new(),
            consumed: 0,
            failed: None,
            message: PhantomData,
        }
    }

    /// Appends input.
    pub fn push(&mut self, bytes: &[u8]) {
        if self.failed.is_some() {
            return;
        }
        if self.consumed > 0 {
            self.buffer.drain(..self.consumed);
            self.consumed = 0;
        }
        self.buffer.extend_from_slice(bytes);
    }

    /// The next complete message, or `None` until more input arrives.
    pub fn next_message(&mut self) -> Result<Option<M>, ProtocolError> {
        match self.take_frame()? {
            Some(range) => decode_body(self.body(range)).map(Some),
            None => Ok(None),
        }
    }

    /// The body of the next complete frame without decoding it, or `None` until more input
    /// arrives. For the handshake, which reads the version first
    /// ([`hello_protocol`](crate::hello_protocol)).
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, ProtocolError> {
        Ok(self.take_frame()?.map(|range| self.body(range).to_vec()))
    }

    /// Bytes pushed but not yet decoded.
    pub fn buffered_len(&self) -> usize {
        self.buffer.len().saturating_sub(self.consumed)
    }

    /// Checks that the input ended on a frame boundary, for when the stream closes.
    pub fn finish(&self) -> Result<(), ProtocolError> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        let pending = self.pending();
        if pending.is_empty() {
            return Ok(());
        }
        let needed = match pending.split_first_chunk::<HEADER_LEN>() {
            Some((header, _)) => HEADER_LEN
                .saturating_add(usize::try_from(u32::from_le_bytes(*header)).unwrap_or(usize::MAX)),
            None => HEADER_LEN,
        };
        Err(ProtocolError::Truncated {
            needed,
            available: pending.len(),
        })
    }

    fn pending(&self) -> &[u8] {
        self.buffer.get(self.consumed..).unwrap_or_default()
    }

    fn body(&self, range: Range<usize>) -> &[u8] {
        self.buffer
            .get(range)
            .expect("invariant: a taken frame lies inside the buffer")
    }

    /// Marks the next complete frame as consumed and returns the range of its body.
    fn take_frame(&mut self) -> Result<Option<Range<usize>>, ProtocolError> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        let Some((header, rest)) = self.pending().split_first_chunk::<HEADER_LEN>() else {
            return Ok(None);
        };
        let available = rest.len();
        let len = match body_len(*header, M::MAX_FRAME_LEN) {
            Ok(len) => len,
            Err(error) => {
                self.failed = Some(error.clone());
                self.buffer = Vec::new();
                self.consumed = 0;
                return Err(error);
            }
        };
        if available < len {
            return Ok(None);
        }
        let start = self.consumed + HEADER_LEN;
        self.consumed = start + len;
        Ok(Some(start..self.consumed))
    }
}
