use pkcs8::der::{Decode, Encode};

use crate::utils::request::RequestIdentityError;

/// Identify the signing issuer, without applying trust, expiry or extension policy
/// to a local identity. The VPN server remains responsible for those checks.
pub(super) fn is_issuer(certificate: &[u8], issuer: &[u8]) -> anyhow::Result<bool> {
  let child = x509_cert::Certificate::from_der(certificate).map_err(|_| RequestIdentityError::InvalidCertificate)?;
  let parent = x509_cert::Certificate::from_der(issuer).map_err(|_| RequestIdentityError::InvalidCertificate)?;
  if child.tbs_certificate().issuer() != parent.tbs_certificate().subject() {
    return Ok(false);
  }
  let signature = child
    .signature()
    .as_bytes()
    .ok_or(RequestIdentityError::InvalidCertificate)?;
  super::platform_import::verify_certificate_signature(&child, &parent, &child.tbs_certificate().to_der()?, signature)
}
