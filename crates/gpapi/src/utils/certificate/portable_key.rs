use crate::utils::request::RequestIdentityError;
use pkcs8::der::{Decode, Encode};
use rustls::{
  SignatureAlgorithm, SignatureScheme,
  pki_types::{PrivateKeyDer, SubjectPublicKeyInfoDer},
  sign::{Signer, SigningKey},
};
use std::sync::Arc;

#[derive(Debug)]
pub(in crate::utils) struct ClientKey {
  key: Arc<dyn SigningKey>,
}

impl ClientKey {
  pub(in crate::utils) fn load(key: &PrivateKeyDer<'_>) -> anyhow::Result<Self> {
    super::validate_private_key(key)?;
    let key = rustls::crypto::ring::default_provider()
      .key_provider
      .load_private_key(key.clone_key())
      .map_err(|_| RequestIdentityError::InvalidKey)?;
    Ok(Self { key })
  }
  pub(in crate::utils) fn matches_certificate(&self, certificate: &[u8]) -> anyhow::Result<bool> {
    let certificate =
      x509_cert::Certificate::from_der(certificate).map_err(|_| RequestIdentityError::InvalidCertificate)?;
    let key = self.key.public_key().ok_or(RequestIdentityError::InvalidKey)?;
    Ok(certificate.tbs_certificate().subject_public_key_info().to_der()? == key.as_ref())
  }
}
impl SigningKey for ClientKey {
  fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
    self.key.choose_scheme(offered)
  }
  fn public_key(&self) -> Option<SubjectPublicKeyInfoDer<'_>> {
    self.key.public_key()
  }
  fn algorithm(&self) -> SignatureAlgorithm {
    self.key.algorithm()
  }
}
