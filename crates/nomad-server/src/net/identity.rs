//! The server's identity: its self-signed certificate, the certificate's private key and the
//! session token (Design doc 20 §5, §9; D-087, D-151, D-122).
//!
//! **Created once, kept** (D-151, superseding D-121): the identity is made when the
//! Universe is created (or at the first `run` of a Universe created before N3) and stored in
//! `identity.json` in the Universe's directory. It survives restarts and image upgrades, so
//! a client imports the connection details once. `nomad-server rotate-token` replaces the
//! token, and with `--certificate` the certificate too, while the server is stopped.
//!
//! **Privacy** (D-122): the file holds the private key and the token. On Unix it is created
//! with mode 0600. On Windows it relies on the per-user ACL of the data directory under
//! `%LOCALAPPDATA%`; the server warns when the Universe lies outside the user's profile.
//! Writes go to a temporary file first, so a reader never sees half a file.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
#[cfg(any(windows, test))]
use std::path::PathBuf;

use nomad_protocol::connect::{hex, unhex};
use nomad_protocol::{AuthToken, Fingerprint};
use ring::digest::{SHA256, digest};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

use crate::error::ServerError;

/// The identity file's name, in the Universe's directory (D-151).
pub const IDENTITY_FILE: &str = "identity.json";

/// The identity file's layout version.
const IDENTITY_FORMAT: u32 = 1;

/// Random bytes in a token; it is written as twice as many hexadecimal digits.
const TOKEN_BYTES: usize = 32;

/// The names the certificate is valid for. The client pins the fingerprint and does not
/// check names (D-087); these only make the certificate conventional.
const CERTIFICATE_NAMES: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// The server's certificate, private key and token.
#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    certificate: Vec<u8>,
    private_key: Vec<u8>,
    token: String,
}

/// `identity.json`.
#[derive(Serialize, Deserialize)]
struct IdentityFile {
    format: u32,
    /// The certificate, DER, in hexadecimal.
    certificate: String,
    /// The private key, PKCS #8 DER, in hexadecimal.
    private_key: String,
    /// The session token.
    token: String,
}

impl Identity {
    /// A new certificate, key and token.
    pub fn generate() -> Result<Self, String> {
        let names: Vec<String> = CERTIFICATE_NAMES.iter().map(|&n| n.to_owned()).collect();
        let certified =
            rcgen::generate_simple_self_signed(names).map_err(|e| format!("certificate: {e}"))?;
        Ok(Self {
            certificate: certified.cert.der().to_vec(),
            private_key: certified.signing_key.serialize_der(),
            token: new_token()?,
        })
    }

    /// The certificate, DER.
    pub fn certificate_der(&self) -> &[u8] {
        &self.certificate
    }

    /// The private key, PKCS #8 DER.
    pub fn private_key_der(&self) -> &[u8] {
        &self.private_key
    }

    /// The certificate's SHA-256 fingerprint, which clients pin.
    pub fn fingerprint(&self) -> Fingerprint {
        let hash = digest(&SHA256, &self.certificate);
        let mut bytes = [0; 32];
        bytes.copy_from_slice(hash.as_ref());
        Fingerprint(bytes)
    }

    /// The session token.
    pub fn token(&self) -> AuthToken {
        AuthToken::new(self.token.clone())
    }

    /// Whether `presented` is the token, compared in constant time (doc 20 §9). The token's
    /// length is not secret.
    pub fn token_matches(&self, presented: &str) -> bool {
        let (a, b) = (self.token.as_bytes(), presented.as_bytes());
        a.len() == b.len() && a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }

    /// Reads the identity of the Universe in `dir`, or `None` if it has none yet.
    pub fn load(dir: &Path) -> Result<Option<Self>, ServerError> {
        let path = dir.join(IDENTITY_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(ServerError::io(
                    format!("cannot read {}", path.display()),
                    e,
                ));
            }
        };
        let damaged = |detail: String| ServerError::Identity {
            path: path.clone(),
            detail,
        };
        let file: IdentityFile =
            serde_json::from_str(&text).map_err(|e| damaged(format!("not readable: {e}")))?;
        if file.format != IDENTITY_FORMAT {
            return Err(damaged(format!("unknown format {}", file.format)));
        }
        let certificate =
            unhex(&file.certificate).ok_or_else(|| damaged("bad certificate".to_owned()))?;
        let private_key =
            unhex(&file.private_key).ok_or_else(|| damaged("bad private key".to_owned()))?;
        if file.token.len() != TOKEN_BYTES * 2 || unhex(&file.token).is_none() {
            return Err(damaged("bad token".to_owned()));
        }
        Ok(Some(Self {
            certificate,
            private_key,
            token: file.token.to_ascii_lowercase(),
        }))
    }

    /// Reads the identity of the Universe in `dir`, creating and storing one if it has none
    /// (D-151: a Universe created before N3 gets its identity at its first run). Safe against
    /// a concurrent creator: the first file written wins and both read it.
    pub fn load_or_create(dir: &Path) -> Result<Self, ServerError> {
        if let Some(identity) = Self::load(dir)? {
            return Ok(identity);
        }
        let identity = Self::generate().map_err(|detail| ServerError::Identity {
            path: dir.join(IDENTITY_FILE),
            detail,
        })?;
        identity.store(dir, false)?;
        Self::load(dir)?.ok_or_else(|| ServerError::Identity {
            path: dir.join(IDENTITY_FILE),
            detail: "vanished right after it was written".to_owned(),
        })
    }

    /// Replaces the token, and with `certificate` the certificate and key too, and stores the
    /// result. The caller holds the Universe's lock, so no server is using the old one.
    pub fn rotate(dir: &Path, certificate: bool) -> Result<Self, ServerError> {
        let path = dir.join(IDENTITY_FILE);
        let current = Self::load_or_create(dir)?;
        let fresh = Self::generate().map_err(|detail| ServerError::Identity {
            path: path.clone(),
            detail,
        })?;
        let rotated = if certificate {
            fresh
        } else {
            Self {
                token: fresh.token,
                ..current
            }
        };
        rotated.store(dir, true)?;
        Ok(rotated)
    }

    fn store(&self, dir: &Path, overwrite: bool) -> Result<(), ServerError> {
        let file = IdentityFile {
            format: IDENTITY_FORMAT,
            certificate: hex(&self.certificate),
            private_key: hex(&self.private_key),
            token: self.token.clone(),
        };
        let text = serde_json::to_string_pretty(&file)
            .map_err(|e| ServerError::Encode(format!("identity: {e}")))?;
        write_private(&dir.join(IDENTITY_FILE), text.as_bytes(), overwrite).map(|_| ())
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("fingerprint", &self.fingerprint())
            .field("private_key", &"<redacted>")
            .field("token", &"<redacted>")
            .finish()
    }
}

fn new_token() -> Result<String, String> {
    let mut bytes = [0; TOKEN_BYTES];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "the system random generator failed".to_owned())?;
    Ok(hex(&bytes))
}

/// Writes `bytes` to `path` readable by the user only (D-122), through a temporary file in
/// the same directory. With `overwrite` an existing file is replaced; without it an existing
/// file is kept and `Ok(false)` returned.
pub fn write_private(path: &Path, bytes: &[u8], overwrite: bool) -> Result<bool, ServerError> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let fail = |what: &str, e| ServerError::io(format!("cannot {what} {}", path.display()), e);
    {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(|e| fail("write", e))?;
        file.write_all(bytes).map_err(|e| fail("write", e))?;
        file.sync_all().map_err(|e| fail("write", e))?;
    }
    if overwrite {
        fs::rename(&temp, path).map_err(|e| {
            let _ = fs::remove_file(&temp);
            fail("replace", e)
        })?;
        return Ok(true);
    }
    // A hard link never replaces an existing file, so the first writer wins.
    let linked = fs::hard_link(&temp, path);
    let _ = fs::remove_file(&temp);
    match linked {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(fail("create", e)),
    }
}

/// Warns when the Universe's secrets may be readable by other users (D-122): on Windows,
/// when the Universe lies outside the user's profile, whose ACL protects it. Unix files are
/// written with mode 0600 instead, so there is nothing to warn about.
pub fn warn_if_exposed(dir: &Path) {
    #[cfg(windows)]
    {
        let canonical = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_owned());
        let inside = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .is_some_and(|profile| canonical(dir).starts_with(canonical(&profile)));
        if !inside {
            tracing::warn!(
                dir = %dir.display(),
                "the Universe lies outside your user profile, so its token and key may be \
                 readable by other users of this computer (D-122)"
            );
        }
    }
    #[cfg(not(windows))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nomad-identity-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn created_once_then_kept_and_rotated() {
        let dir = temp_dir("rotate");
        let first = Identity::load_or_create(&dir).unwrap();
        assert_eq!(Identity::load_or_create(&dir).unwrap(), first);
        assert_eq!(first.token.len(), 64);
        assert!(first.token_matches(first.token().as_str()));
        assert!(!first.token_matches("00"));

        let token_only = Identity::rotate(&dir, false).unwrap();
        assert_ne!(token_only.token, first.token);
        assert_eq!(token_only.fingerprint(), first.fingerprint());
        assert_eq!(Identity::load(&dir).unwrap(), Some(token_only.clone()));

        let both = Identity::rotate(&dir, true).unwrap();
        assert_ne!(both.fingerprint(), first.fingerprint());
        assert_ne!(both.token, token_only.token);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_file_is_an_error_not_a_panic() {
        let dir = temp_dir("damaged");
        fs::write(dir.join(IDENTITY_FILE), "{not json").unwrap();
        assert!(matches!(
            Identity::load(&dir),
            Err(ServerError::Identity { .. })
        ));
        fs::write(
            dir.join(IDENTITY_FILE),
            r#"{"format":1,"certificate":"zz","private_key":"00","token":"00"}"#,
        )
        .unwrap();
        assert!(Identity::load(&dir).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn debug_output_hides_the_secrets() {
        let identity = Identity::generate().unwrap();
        let text = format!("{identity:?}");
        assert!(!text.contains(&identity.token), "{text}");
        assert!(text.contains(&identity.fingerprint().to_hex()));
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("mode");
        Identity::load_or_create(&dir).unwrap();
        let mode = fs::metadata(dir.join(IDENTITY_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = fs::remove_dir_all(&dir);
    }
}
