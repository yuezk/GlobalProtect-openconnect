//! Portable certificate verification; native legacy import is unavailable here.
use crate::utils::request::RequestIdentityError;
use pkcs8::der::{Decode, Encode, asn1::AnyRef};

pub(in crate::utils) fn decrypt(
  _bytes: &[u8],
  _pem: bool,
  _passphrase: Option<&str>,
) -> anyhow::Result<pkcs8::SecretDocument> {
  Err(RequestIdentityError::UnsupportedEncryption.into())
}

pub(in crate::utils) fn archive(
  _bytes: &[u8],
  _passphrase: Option<&str>,
) -> anyhow::Result<(zeroize::Zeroizing<Vec<u8>>, pkcs8::SecretDocument)> {
  Err(RequestIdentityError::UnsupportedEncryption.into())
}

pub(super) fn verify_certificate_signature(
  child: &x509_cert::Certificate,
  parent: &x509_cert::Certificate,
  message: &[u8],
  signature: &[u8],
) -> anyhow::Result<bool> {
  let public_key = parent.tbs_certificate().subject_public_key_info();
  let key_algorithm = public_key.algorithm.to_der()?;
  let signature_algorithm = child.signature_algorithm().to_der()?;
  let key_algorithm = AnyRef::from_der(&key_algorithm)?.value();
  let signature_algorithm = AnyRef::from_der(&signature_algorithm)?.value();
  let key = public_key
    .subject_public_key
    .as_bytes()
    .ok_or(RequestIdentityError::InvalidCertificate)?;
  Ok(
    rustls::crypto::ring::default_provider()
      .signature_verification_algorithms
      .all
      .iter()
      .any(|algorithm| {
        algorithm.public_key_alg_id().as_ref() == key_algorithm
          && algorithm.signature_alg_id().as_ref() == signature_algorithm
          && algorithm.verify_signature(key, message, signature).is_ok()
      }),
  )
}
