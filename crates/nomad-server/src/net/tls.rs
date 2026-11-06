//! The QUIC endpoint's TLS and transport settings (N3 plan §4.2, §4.6; D-087).
//!
//! TLS 1.3 only, with `rustls` and the `ring` provider; ALPN `nomad` without a version,
//! because the version check belongs to `Hello` (D-087). 0-RTT stays off: it allows replay.
//! The client may open one bidirectional stream (the control stream) and no unidirectional
//! ones; the server opens the state stream (N4 fills it).

use std::sync::Arc;

use quinn::crypto::rustls::QuicServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use super::identity::Identity;
use super::{ALPN, Limits};

/// The endpoint configuration for `identity` under `limits`.
pub fn server_config(identity: &Identity, limits: &Limits) -> Result<quinn::ServerConfig, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let certificate = CertificateDer::from(identity.certificate_der().to_vec());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        identity.private_key_der().to_vec(),
    ));
    let mut tls = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| format!("TLS versions: {e}"))?
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)
        .map_err(|e| format!("certificate: {e}"))?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    tls.max_early_data_size = 0;
    let crypto = QuicServerConfig::try_from(tls).map_err(|e| format!("QUIC TLS: {e}"))?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
    config.transport_config(Arc::new(transport(limits)?));
    Ok(config)
}

/// Idle timeout and keep-alive (§4.6), one control stream per connection.
fn transport(limits: &Limits) -> Result<quinn::TransportConfig, String> {
    let mut transport = quinn::TransportConfig::default();
    transport
        .max_idle_timeout(Some(
            limits
                .idle_timeout
                .try_into()
                .map_err(|e| format!("idle timeout: {e}"))?,
        ))
        .keep_alive_interval(Some(limits.keep_alive))
        .max_concurrent_bidi_streams(1_u8.into())
        .max_concurrent_uni_streams(0_u8.into());
    Ok(transport)
}
