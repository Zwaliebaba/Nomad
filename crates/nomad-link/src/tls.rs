//! TLS for the client: TLS 1.3 with `ring`, ALPN `nomad`, and the server certificate pinned
//! by its SHA-256 fingerprint (D-087). There is no certificate authority and no system trust
//! store; the handshake signature is still checked, so only the holder of the pinned
//! certificate's key passes.

use std::sync::Arc;
use std::time::Duration;

use nomad_protocol::Fingerprint;
use quinn::crypto::rustls::QuicClientConfig;
use ring::digest::{SHA256, digest};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, SignatureScheme};

/// The ALPN protocol name; the server's `nomad_server::net::ALPN`.
pub const ALPN: &[u8] = b"nomad";

/// The name the client asks for; the server's certificate covers it. The pin, not the
/// name, is what is checked.
pub const SERVER_NAME: &str = "localhost";

/// QUIC idle timeout and keep-alive, as on the server (N3 plan §4.6).
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const KEEP_ALIVE: Duration = Duration::from_secs(5);

/// Accepts exactly the certificate with this fingerprint.
#[derive(Debug)]
struct Pinned {
    fingerprint: Fingerprint,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if digest(&SHA256, end_entity.as_ref()).as_ref() == self.fingerprint.0 {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        // TLS 1.2 is never negotiated.
        Err(rustls::Error::General(
            "TLS 1.2 is not supported".to_owned(),
        ))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// The QUIC client configuration for a server with `fingerprint`.
pub fn client_config(fingerprint: Fingerprint) -> Result<quinn::ClientConfig, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(Pinned {
        fingerprint,
        algorithms: provider.signature_verification_algorithms,
    });
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| format!("TLS versions: {e}"))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = QuicClientConfig::try_from(tls).map_err(|e| format!("QUIC TLS: {e}"))?;
    let mut config = quinn::ClientConfig::new(Arc::new(crypto));
    let mut transport = quinn::TransportConfig::default();
    transport
        .max_idle_timeout(Some(
            IDLE_TIMEOUT
                .try_into()
                .map_err(|e| format!("idle timeout: {e}"))?,
        ))
        .keep_alive_interval(Some(KEEP_ALIVE));
    config.transport_config(Arc::new(transport));
    Ok(config)
}
