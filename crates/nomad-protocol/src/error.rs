//! Protocol errors (Design doc 20 §9; `CODING_STANDARDS.md` §5).

use thiserror::Error;

/// Why bytes could not be turned into a message, or a message into a frame.
///
/// Decoding never panics, whatever the input. Rejecting and logging the input, and deciding
/// whether to close the connection, is the caller's job (doc 20 §9).
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ProtocolError {
    /// A frame is longer than the limit for its direction. Decoding stops for good, because
    /// the stream cannot be resynchronised after an untrusted length.
    #[error("frame of {len} bytes exceeds the limit of {max} bytes")]
    Oversized {
        /// The length declared in the header, or the length of the encoded message.
        len: u64,
        /// The limit for this direction.
        max: u32,
    },
    /// The input ends inside a frame.
    #[error("input ends inside a frame: {available} of {needed} bytes")]
    Truncated {
        /// Bytes needed to complete the frame (header and body), as far as known.
        needed: usize,
        /// Bytes available.
        available: usize,
    },
    /// The frame body is not exactly one valid message. The frame is consumed, so the stream
    /// stays in sync.
    #[error("malformed message body: {0}")]
    Malformed(MalformedBody),
    /// The peer speaks a different protocol version. Client and server must match exactly
    /// (D-087).
    #[error("protocol version {found} is not supported (this build speaks {expected})")]
    UnknownVersion {
        /// The peer's version.
        found: u32,
        /// This build's version.
        expected: u32,
    },
    /// A message could not be encoded.
    #[error("message encoding failed: {0}")]
    Encode(postcard::Error),
}

/// What is wrong with a frame body.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum MalformedBody {
    /// The bytes do not decode as a message of this direction.
    #[error("{0}")]
    Decode(postcard::Error),
    /// The message is followed by more bytes inside the same frame.
    #[error("{0} bytes after the message")]
    TrailingBytes(usize),
    /// A handshake body that does not start with `Hello`.
    #[error("the message is not Hello")]
    NotHello,
}

impl ProtocolError {
    pub(crate) const fn decode(error: postcard::Error) -> Self {
        Self::Malformed(MalformedBody::Decode(error))
    }
}
