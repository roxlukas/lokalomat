//! TLS for the LAN gateway: a self-signed certificate on the computer, pinned
//! by its SHA-256 fingerprint on the phone (no certificate authority). Uses
//! the `ring` provider of rustls only.

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

use crate::pairing::{ct_eq, sha256};

/// Name inside the self-signed certificate (the phone connects by IP and
/// checks only the pinned fingerprint).
pub const CERT_NAME: &str = "lokalomat.local";

pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// SHA-256 of a DER certificate – what the QR code carries.
pub fn fingerprint(cert_der: &[u8]) -> [u8; 32] {
    sha256(cert_der)
}

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("TLS setup failed: {0}")]
    Rustls(#[from] rustls::Error),
    #[error("certificate generation failed: {0}")]
    Generate(String),
}

pub fn server_config(
    cert_der: Vec<u8>,
    key_pkcs8_der: Vec<u8>,
) -> Result<Arc<rustls::ServerConfig>, TlsError> {
    let config = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert_der)],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pkcs8_der)),
        )?;
    Ok(Arc::new(config))
}

/// A new self-signed certificate: `(certificate DER, PKCS#8 private key DER)`.
#[cfg(feature = "server")]
pub fn generate() -> Result<(Vec<u8>, Vec<u8>), TlsError> {
    let key = rcgen::KeyPair::generate().map_err(|e| TlsError::Generate(e.to_string()))?;
    let mut params = rcgen::CertificateParams::new(vec![CERT_NAME.to_owned()])
        .map_err(|e| TlsError::Generate(e.to_string()))?;
    params.not_before = rcgen::date_time_ymd(2025, 1, 1);
    params.not_after = rcgen::date_time_ymd(2045, 1, 1);
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Lokalomat AI Studio");
    let cert = params
        .self_signed(&key)
        .map_err(|e| TlsError::Generate(e.to_string()))?;
    Ok((cert.der().to_vec(), key.serialize_der()))
}

/// Accepts exactly one certificate: the one whose fingerprint was in the QR code.
#[derive(Debug)]
pub struct PinnedVerifier {
    fingerprint: [u8; 32],
    provider: Arc<CryptoProvider>,
}

impl PinnedVerifier {
    pub fn new(fingerprint: [u8; 32]) -> Arc<Self> {
        Arc::new(PinnedVerifier {
            fingerprint,
            provider: provider(),
        })
    }
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if ct_eq(&fingerprint(end_entity.as_ref()), &self.fingerprint) {
            Ok(ServerCertVerified::assertion())
        } else {
            tracing::warn!("server certificate does not match the paired computer");
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub fn client_config(fingerprint: [u8; 32]) -> Result<Arc<rustls::ClientConfig>, TlsError> {
    let config = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(PinnedVerifier::new(fingerprint))
        .with_no_client_auth();
    Ok(Arc::new(config))
}
