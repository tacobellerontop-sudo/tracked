//! TLS configuration for the Riot Client's *local* API.
//!
//! The local server presents a self-signed certificate, so normal
//! verification fails. Rather than turning verification off globally, this
//! verifier accepts any certificate **only** when the connection target is a
//! loopback IP address and rejects everything else. The resulting config is
//! used exclusively by the local REST client and the local websocket.

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

#[derive(Debug)]
pub struct LoopbackOnlyVerifier {
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for LoopbackOnlyVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if is_loopback(server_name) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "self-signed certs are only accepted for 127.0.0.1".into(),
            ))
        }
    }

    // The certificate itself is untrusted (self-signed), so checking the
    // handshake signature against it proves nothing. Skipping it matches
    // reqwest's `danger_accept_invalid_certs` and avoids failures on unusual
    // self-signed certificates. Non-loopback targets were already rejected
    // in `verify_server_cert`, so this never applies to internet hosts.
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

pub fn is_loopback(name: &ServerName<'_>) -> bool {
    match name {
        ServerName::IpAddress(ip) => std::net::IpAddr::from(*ip).is_loopback(),
        _ => false,
    }
}

/// A rustls client config that only trusts loopback self-signed certs.
pub fn local_client_config() -> Arc<rustls::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("ring provider supports default TLS versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(LoopbackOnlyVerifier { provider }))
        .with_no_client_auth();
    Arc::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_is_trusted() {
        assert!(is_loopback(&ServerName::try_from("127.0.0.1").unwrap()));
        assert!(!is_loopback(&ServerName::try_from("10.0.0.1").unwrap()));
        assert!(!is_loopback(&ServerName::try_from("pd.na.a.pvp.net").unwrap()));
        assert!(!is_loopback(&ServerName::try_from("localhost").unwrap()));
    }
}
