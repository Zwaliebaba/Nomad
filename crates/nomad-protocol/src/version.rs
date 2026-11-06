//! The protocol version and the handshake check (Design doc 20 §5, §13 item 3; D-087).
//!
//! Client and server must speak exactly the same version. A mismatch is real even in the
//! MVP: the server keeps running after the client exits (D-026), so an updated client can
//! meet a server of the previous build.
//!
//! For the check to work across versions, a small part of the encoding is **frozen** and
//! never changes in any version:
//!
//! - the frame header (`u32` little-endian body length);
//! - [`ClientMessage::Hello`](crate::ClientMessage::Hello) is variant 0 of `ClientMessage`,
//!   and `protocol` is its first field;
//! - [`ServerMessage::Bye`](crate::ServerMessage::Bye) is variant 0 of `ServerMessage`, and
//!   [`ByeReason::VersionMismatch`](crate::ByeReason::VersionMismatch) is variant 0 of
//!   `ByeReason`, with the server's version as its only field.
//!
//! So a server reads any client's version with [`hello_protocol`] before decoding the rest,
//! and any client can decode the `Bye` that tells it the server's version.

use crate::error::{MalformedBody, ProtocolError};

/// The protocol version this build speaks. Any change to a message type or its encoding
/// increments it.
///
/// - 1: the N1 messages.
/// - 2: `Ping` and `Pong` carry a nonce; rejections carry the validator's reason (D-123) or
///   `SeasonOver` (D-116); `ByeReason` gains `HandshakeTimeout`, `Resync` and `Replaced`
///   (D-124). N3 plan §4.7.
pub const PROTOCOL_VERSION: u32 = 2;

/// The variant index of `ClientMessage::Hello` (frozen).
const HELLO_VARIANT: u32 = 0;

/// Accepts the peer's version if it is exactly [`PROTOCOL_VERSION`] (D-087).
pub fn check_version(found: u32) -> Result<(), ProtocolError> {
    if found == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ProtocolError::UnknownVersion {
            found,
            expected: PROTOCOL_VERSION,
        })
    }
}

/// The protocol version in the body of a client's `Hello`, read from the frozen prefix
/// without decoding the rest. Works on a `Hello` from any protocol version.
///
/// Fails with [`MalformedBody::NotHello`] if the body is another message.
pub fn hello_protocol(body: &[u8]) -> Result<u32, ProtocolError> {
    // postcard writes an enum variant index and a u32 the same way, as a varint.
    let (variant, rest) = postcard::take_from_bytes::<u32>(body).map_err(ProtocolError::decode)?;
    if variant != HELLO_VARIANT {
        return Err(ProtocolError::Malformed(MalformedBody::NotHello));
    }
    let (protocol, _) = postcard::take_from_bytes::<u32>(rest).map_err(ProtocolError::decode)?;
    Ok(protocol)
}
