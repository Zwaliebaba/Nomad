//! What a client needs to reach a server: its address, its certificate fingerprint and the
//! session token (Design doc 20 §5, §9; D-120, D-122, D-150, D-151).
//!
//! The details reach a client in one of two forms:
//!
//! - **The connection file** `server.json` in the Universe's directory ([`ConnectionFile`],
//!   [`CONNECTION_FILE`]). The running server writes it, so a client on the same machine
//!   and user account reads it directly (D-120). Its privacy is the data directory's
//!   (D-122).
//! - **The import string**, a `nomad://` URI that `nomad-server connect-info` prints for a
//!   client that cannot read the server's files, such as a server in a container
//!   ([`ConnectInfo::to_uri`], [`ConnectInfo::parse`]; D-151, D-152). The client imports it
//!   once; it stays valid until the operator rotates the token.
//!
//! Both carry the same [`ConnectInfo`]. This module only shapes and parses the details;
//! opening the connection belongs to the client link and the server.

use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::message::AuthToken;

/// The connection file's name, in the Universe's directory (D-120).
pub const CONNECTION_FILE: &str = "server.json";

/// The version of the [`ConnectionFile`] layout.
pub const CONNECTION_FILE_FORMAT: u32 = 1;

/// The import string's scheme.
pub const URI_SCHEME: &str = "nomad://";

/// The SHA-256 fingerprint of the server's certificate, which the client pins (D-087).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    /// The fingerprint as 64 lower-case hexadecimal digits.
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({})", self.to_hex())
    }
}

impl FromStr for Fingerprint {
    type Err = ConnectInfoError;

    /// Parses 64 hexadecimal digits, in either case.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bytes = unhex(text).ok_or(ConnectInfoError::BadFingerprint)?;
        let array = <[u8; 32]>::try_from(bytes).map_err(|_| ConnectInfoError::BadFingerprint)?;
        Ok(Self(array))
    }
}

/// Where a server listens and how to prove the client may use it.
#[derive(Clone, PartialEq, Eq)]
pub struct ConnectInfo {
    /// The address and UDP port to connect to.
    pub address: SocketAddr,
    /// The certificate fingerprint to pin.
    pub fingerprint: Fingerprint,
    /// The session token (doc 20 §9).
    pub token: AuthToken,
    /// The Universe's 8-character ID (D-107), if known; informative only.
    pub universe: Option<String>,
}

impl ConnectInfo {
    /// The import string: `nomad://ADDRESS:PORT/?fingerprint=HEX&token=TOKEN[&universe=ID]`.
    pub fn to_uri(&self) -> String {
        let mut uri = format!(
            "{URI_SCHEME}{}/?fingerprint={}&token={}",
            self.address,
            self.fingerprint,
            self.token.as_str()
        );
        if let Some(universe) = &self.universe {
            uri.push_str("&universe=");
            uri.push_str(universe);
        }
        uri
    }

    /// Parses an import string. Surrounding text is ignored, so the whole output of
    /// `connect-info` can be pasted: the first word starting with `nomad://` is used.
    pub fn parse(text: &str) -> Result<Self, ConnectInfoError> {
        let uri = text
            .split_whitespace()
            .find(|word| word.starts_with(URI_SCHEME))
            .ok_or(ConnectInfoError::NoUri)?;
        let rest = &uri[URI_SCHEME.len()..];
        let (address, query) = rest.split_once('/').ok_or(ConnectInfoError::BadUri)?;
        let address = SocketAddr::from_str(address).map_err(|_| ConnectInfoError::BadAddress)?;
        let query = query.strip_prefix('?').ok_or(ConnectInfoError::BadUri)?;
        let (mut fingerprint, mut token, mut universe) = (None, None, None);
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=').ok_or(ConnectInfoError::BadUri)?;
            match key {
                "fingerprint" => fingerprint = Some(value.parse::<Fingerprint>()?),
                "token" => token = Some(parse_token(value)?),
                "universe" => universe = Some(value.to_owned()),
                // Unknown keys are ignored, so a later build can add some.
                _ => {}
            }
        }
        Ok(Self {
            address,
            fingerprint: fingerprint.ok_or(ConnectInfoError::Missing("fingerprint"))?,
            token: token.ok_or(ConnectInfoError::Missing("token"))?,
            universe,
        })
    }
}

impl fmt::Debug for ConnectInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectInfo")
            .field("address", &self.address)
            .field("fingerprint", &self.fingerprint)
            .field("token", &self.token)
            .field("universe", &self.universe)
            .finish()
    }
}

/// The connection file `server.json` (D-120, D-150): written by the running server, removed
/// on a clean shutdown. A file left by a crash is recognised as stale because the
/// Universe's lock is free.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionFile {
    /// Always [`CONNECTION_FILE_FORMAT`].
    pub format: u32,
    /// Where a client on this machine connects: the bound address, or loopback when the
    /// server binds to all addresses.
    pub address: SocketAddr,
    /// The certificate fingerprint, 64 hexadecimal digits.
    pub fingerprint: String,
    /// The session token.
    pub token: String,
    /// The Universe's ID.
    pub universe: String,
    /// The server's build.
    pub build: String,
    /// The server's protocol version.
    pub protocol: u32,
    /// The server's process ID, for diagnostics.
    pub pid: u32,
}

impl ConnectionFile {
    /// The connection details it holds.
    pub fn connect_info(&self) -> Result<ConnectInfo, ConnectInfoError> {
        if self.format != CONNECTION_FILE_FORMAT {
            return Err(ConnectInfoError::UnknownFormat(self.format));
        }
        Ok(ConnectInfo {
            address: self.address,
            fingerprint: self.fingerprint.parse()?,
            token: parse_token(&self.token)?,
            universe: Some(self.universe.clone()),
        })
    }
}

impl fmt::Debug for ConnectionFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionFile")
            .field("format", &self.format)
            .field("address", &self.address)
            .field("fingerprint", &self.fingerprint)
            .field("token", &"<redacted>")
            .field("universe", &self.universe)
            .field("build", &self.build)
            .field("protocol", &self.protocol)
            .field("pid", &self.pid)
            .finish()
    }
}

/// Why connection details could not be read.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ConnectInfoError {
    /// The text holds no `nomad://` import string.
    #[error("no nomad:// import string found")]
    NoUri,
    /// The import string is not `nomad://ADDRESS:PORT/?key=value&...`.
    #[error("the import string is malformed")]
    BadUri,
    /// The address is not `IP:PORT`.
    #[error("the address is not IP:PORT")]
    BadAddress,
    /// The fingerprint is not 64 hexadecimal digits.
    #[error("the fingerprint is not 64 hexadecimal digits")]
    BadFingerprint,
    /// The token is empty or holds characters other than hexadecimal digits.
    #[error("the token is malformed")]
    BadToken,
    /// A required field is missing.
    #[error("the {0} is missing")]
    Missing(&'static str),
    /// The connection file has a layout this build does not know.
    #[error("connection file format {0} is not supported")]
    UnknownFormat(u32),
}

/// Tokens are hexadecimal, so they need no escaping in an import string.
fn parse_token(text: &str) -> Result<AuthToken, ConnectInfoError> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ConnectInfoError::BadToken);
    }
    Ok(AuthToken::new(text.to_ascii_lowercase()))
}

/// Lower-case hexadecimal digits of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0F)]));
    }
    text
}

/// The bytes of a string of hexadecimal digits, or `None` if it is not one.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    let digit = |c: u8| {
        char::from(c)
            .to_digit(16)
            .and_then(|d| u8::try_from(d).ok())
    };
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    bytes
        .chunks_exact(2)
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ConnectInfo {
        ConnectInfo {
            address: "127.0.0.1:31790".parse().unwrap(),
            fingerprint: Fingerprint([0xAB; 32]),
            token: AuthToken::new("00ff".repeat(16)),
            universe: Some("k7q2m9xd".to_owned()),
        }
    }

    #[test]
    fn import_string_round_trips() {
        let info = info();
        let uri = info.to_uri();
        assert!(
            uri.starts_with("nomad://127.0.0.1:31790/?fingerprint=abab"),
            "{uri}"
        );
        assert_eq!(ConnectInfo::parse(&uri).unwrap(), info);
        // Pasted with surrounding text, as `connect-info` prints it.
        let pasted = format!("Address 127.0.0.1:31790\nImport  {uri}\n");
        assert_eq!(ConnectInfo::parse(&pasted).unwrap(), info);
        // IPv6 and no Universe.
        let v6 = ConnectInfo {
            address: "[::1]:5".parse().unwrap(),
            universe: None,
            ..info
        };
        assert_eq!(ConnectInfo::parse(&v6.to_uri()).unwrap(), v6);
    }

    #[test]
    fn bad_import_strings_are_rejected() {
        let good = info().to_uri();
        for (text, error) in [
            ("hello", ConnectInfoError::NoUri),
            ("nomad://127.0.0.1:1", ConnectInfoError::BadUri),
            (
                "nomad://localhost:1/?token=ab",
                ConnectInfoError::BadAddress,
            ),
            (
                "nomad://127.0.0.1:1/?fingerprint=ab&token=ab",
                ConnectInfoError::BadFingerprint,
            ),
            (
                "nomad://127.0.0.1:1/?token=ab",
                ConnectInfoError::Missing("fingerprint"),
            ),
        ] {
            assert_eq!(ConnectInfo::parse(text).unwrap_err(), error, "{text}");
        }
        let bad_token = good.replace("token=00ff", "token=zz");
        assert_eq!(
            ConnectInfo::parse(&bad_token).unwrap_err(),
            ConnectInfoError::BadToken
        );
    }

    #[test]
    fn secrets_stay_out_of_debug_output() {
        let info = info();
        let file = ConnectionFile {
            format: CONNECTION_FILE_FORMAT,
            address: info.address,
            fingerprint: info.fingerprint.to_hex(),
            token: info.token.as_str().to_owned(),
            universe: "k7q2m9xd".to_owned(),
            build: "b".to_owned(),
            protocol: 2,
            pid: 1,
        };
        for text in [format!("{info:?}"), format!("{file:?}")] {
            assert!(!text.contains("00ff00ff"), "{text}");
        }
        assert_eq!(file.connect_info().unwrap(), info);
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(hex(&[0, 1, 0xAB, 0xFF]), "0001abff");
        assert_eq!(unhex("0001ABff"), Some(vec![0, 1, 0xAB, 0xFF]));
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
    }
}
