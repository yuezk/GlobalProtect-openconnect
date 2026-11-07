use p12_keystore::Pkcs12Archive;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

use super::{
  ClientKey,
  pkcs12_format::{self, ArchiveBackend},
};
use crate::utils::{client_tls, request::RequestIdentityError};

pub(in crate::utils) fn certified_key(
  bytes: &[u8],
  passphrase: Option<&str>,
) -> anyhow::Result<std::sync::Arc<rustls::sign::CertifiedKey>> {
  anyhow::ensure!(
    passphrase.is_none_or(|password| !password.as_bytes().contains(&0)),
    "PKCS#12 passphrases containing NUL are not supported"
  );
  if pkcs12_format::backend(bytes)? == ArchiveBackend::NativeRc4 {
    let (certificate, key) = super::archive(bytes, passphrase)?;
    let key = key.to_pem("PRIVATE KEY", pkcs8::LineEnding::LF)?;
    return client_tls::pem_certified_key(&certificate, key.as_bytes());
  }
  let archive = Pkcs12Archive::from_pkcs12(bytes, passphrase.unwrap_or_default()).map_err(|error| match error {
    p12_keystore::error::Error::UnsupportedEncryptionScheme | p12_keystore::error::Error::UnsupportedMacAlgorithm => {
      RequestIdentityError::UnsupportedEncryption
    }
    p12_keystore::error::Error::MacError(_)
    | p12_keystore::error::Error::UnpadError
    | p12_keystore::error::Error::Pkcs5Error(_)
      if passphrase.is_none() =>
    {
      RequestIdentityError::NoPassphrase("PKCS#12")
    }
    error => RequestIdentityError::InvalidPkcs12(error),
  })?;
  if archive.keys.is_empty() || archive.certs.is_empty() {
    return Err(RequestIdentityError::IncompletePkcs12.into());
  }
  if archive.keys.len() != 1 {
    return Err(RequestIdentityError::AmbiguousPkcs12.into());
  }
  let key = &archive.keys[0].key;
  let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.as_der().to_vec()));
  let signing_key = ClientKey::load(&private_key)?;
  let mut certs = Vec::new();
  for bag in &archive.certs {
    if !certs
      .iter()
      .any(|cert: &&p12_keystore::Certificate| cert.as_der() == bag.cert.as_der())
    {
      certs.push(&bag.cert);
    }
  }
  let mut matches = Vec::new();
  for (index, cert) in certs.iter().enumerate() {
    if signing_key.matches_certificate(cert.as_der())? {
      matches.push(index);
    }
  }
  let leaf = match matches.first() {
    Some(leaf) => *leaf,
    None => return Err(RequestIdentityError::KeyMismatch.into()),
  };
  let mut current = certs.remove(leaf);
  let mut chain = Vec::new();
  loop {
    chain.extend_from_slice(pem::encode(&pem::Pem::new("CERTIFICATE", current.as_der())).as_bytes());
    if current.subject() == current.issuer() {
      break;
    }
    let Some(parent) = certs.iter().position(|cert| cert.subject() == current.issuer()) else {
      break;
    };
    current = certs.remove(parent);
  }
  let key = pkcs8::SecretDocument::try_from(key.as_der()).map_err(|_| RequestIdentityError::InvalidKey)?;
  let key = key.to_pem("PRIVATE KEY", pkcs8::LineEnding::LF)?;
  client_tls::pem_certified_key(&chain, key.as_bytes())
}
