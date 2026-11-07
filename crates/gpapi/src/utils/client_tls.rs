//! Local identity matching is separate from the peer's certificate validation.
use std::{io::Cursor, sync::Arc};

use pkcs8::der::Decode;
use rustls::{
  ClientConfig, DigitallySignedStruct, SignatureScheme,
  client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
  pki_types::{CertificateDer, ServerName, UnixTime},
  sign::{CertifiedKey, SingleCertAndKey},
};
use rustls_platform_verifier::BuilderVerifierExt;

use super::{
  certificate::{ClientKey, validate_certificate},
  request::RequestIdentityError,
};

pub(super) fn pem_certified_key(certificate: &[u8], key: &[u8]) -> anyhow::Result<Arc<CertifiedKey>> {
  let certificates = rustls_pemfile::certs(&mut Cursor::new(certificate))
    .collect::<Result<Vec<_>, _>>()
    .map_err(|_| RequestIdentityError::InvalidCertificate)?;
  let private_key = rustls_pemfile::private_key(&mut Cursor::new(key))
    .map_err(|_| RequestIdentityError::InvalidKey)?
    .ok_or(RequestIdentityError::NoKey)?;
  let signing_key = Arc::new(ClientKey::load(&private_key)?);
  let leaf = certificates.first().ok_or(RequestIdentityError::InvalidCertificate)?;
  validate_certificate(leaf)?;
  if !signing_key.matches_certificate(leaf)? {
    return Err(RequestIdentityError::KeyMismatch.into());
  }
  for certificate in certificates.iter().skip(1) {
    x509_cert::Certificate::from_der(certificate).map_err(|_| RequestIdentityError::InvalidCertificate)?;
  }
  // The local key has been matched without applying peer certificate policy.
  Ok(Arc::new(CertifiedKey::new(certificates, signing_key)))
}

pub(super) fn identity_client_builder(
  identity: Arc<CertifiedKey>,
  ignore_server_certificate_errors: bool,
) -> anyhow::Result<reqwest::ClientBuilder> {
  let provider = Arc::new(rustls::crypto::ring::default_provider());
  let config = ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions()?;
  let config = if ignore_server_certificate_errors {
    config
      .dangerous()
      .with_custom_certificate_verifier(Arc::new(IgnoreServerCertificateErrors {
        schemes: provider.signature_verification_algorithms.supported_schemes(),
      }))
  } else {
    // Match Reqwest's default system trust store, including private enterprise CAs.
    config.with_platform_verifier()?
  };
  let mut config = config.with_client_cert_resolver(Arc::new(SingleCertAndKey::from(identity)));
  config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
  Ok(super::request::client_builder().tls_backend_preconfigured(config))
}

/// Equivalent to Reqwest's danger_accept_invalid_certs(true), used only when
/// the user explicitly enables the existing ignore-certificate-errors setting.
#[derive(Debug)]
struct IgnoreServerCertificateErrors {
  schemes: Vec<SignatureScheme>,
}

impl ServerCertVerifier for IgnoreServerCertificateErrors {
  fn verify_server_cert(
    &self,
    _: &CertificateDer<'_>,
    _: &[CertificateDer<'_>],
    _: &ServerName<'_>,
    _: &[u8],
    _: UnixTime,
  ) -> Result<ServerCertVerified, rustls::Error> {
    Ok(ServerCertVerified::assertion())
  }

  fn verify_tls12_signature(
    &self,
    _: &[u8],
    _: &CertificateDer<'_>,
    _: &DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    Ok(HandshakeSignatureValid::assertion())
  }

  fn verify_tls13_signature(
    &self,
    _: &[u8],
    _: &CertificateDer<'_>,
    _: &DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    Ok(HandshakeSignatureValid::assertion())
  }

  fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
    self.schemes.clone()
  }
}
