//! The Nomad wire protocol (Design doc 20 §5, D-087).
//!
//! - **Messages:** one versioned enum per direction, [`ClientMessage`] and
//!   [`ServerMessage`], with the payloads of doc 20 §5. The commander's view is a
//!   [`ViewSnapshot`] followed by [`ViewChanges`], split by information class (doc 16 §2,
//!   §6). Commands are the [`nomad_types::Command`] shape, shared with the simulation's
//!   validator.
//! - **Version:** client and server must speak exactly [`PROTOCOL_VERSION`].
//!   [`check_version`] and [`hello_protocol`] detect a mismatch, even with a peer whose
//!   message layout differs ([`version`] lists the frozen parts).
//! - **Framing:** a `u32` little-endian body length, then the `postcard` body
//!   ([`encode_frame`], [`decode_frame`], [`FrameDecoder`]). Each direction has a maximum
//!   frame length.
//! - **Robustness:** no input can make decoding panic. Oversized, truncated and malformed
//!   frames and unknown versions are typed [`ProtocolError`]s (doc 20 §9,
//!   `CODING_STANDARDS.md` §5). Decoding checks the encoding only, not meaning: a decoded
//!   command can still be nonsense, which is the validator's to reject (D-077).
//! - **Time:** every time on the wire is a [`Tick`](nomad_types::Tick) deadline, never a
//!   countdown (doc 11 §3). Positions are metres in the system frame (D-099).
//!
//! No networking: the QUIC transport belongs to the server and the client (doc 20 §5).

mod error;
mod frame;
mod message;
pub mod version;
mod view;

pub use error::{MalformedBody, ProtocolError};
pub use frame::{
    FrameDecoder, HEADER_LEN, MAX_CLIENT_FRAME_LEN, MAX_FRAME_LEN, MAX_SERVER_FRAME_LEN,
    WireMessage, decode_body, decode_frame, encode_frame,
};
pub use message::{
    Announcement, AuthToken, ByeReason, CatchUpProgress, ClientMessage, CommandOutcome,
    Notification, NotificationKind, RejectReason, ServerMessage,
};
/// A place in a system, used by the view and by orders; shared with the simulation, so it
/// lives in `nomad-types`.
pub use nomad_types::Location;
pub use version::{PROTOCOL_VERSION, check_version, hello_protocol};
pub use view::{Entities, EntityChanges, EntityEntry, EntityKind, ViewChanges, ViewSnapshot};
