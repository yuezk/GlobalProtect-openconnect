use p12_keystore::Pkcs12Archive;
use rustls::{
  pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
  sign::CertifiedKey,
};

use crate::utils::request::{RequestIdentityError, pem_request_identity};

pub(in crate::utils) fn identity(bytes: &[u8], passphrase: Option<&str>) -> anyhow::Result<reqwest::Identity> {
  anyhow::ensure!(
    passphrase.is_none_or(|password| !password.as_bytes().contains(&0)),
    "PKCS#12 passphrases containing NUL are not supported"
  );
  let archive = Pkcs12Archive::from_pkcs12(bytes, passphrase.unwrap_or_default()).map_err(|error| {
    if passphrase.is_none() {
      RequestIdentityError::NoPassphrase("PKCS#12")
    } else {
      RequestIdentityError::InvalidPkcs12(error)
    }
  })?;
  if archive.keys.is_empty() || archive.certs.is_empty() {
    return Err(RequestIdentityError::IncompletePkcs12.into());
  }
  if archive.keys.len() != 1 {
    return Err(RequestIdentityError::AmbiguousPkcs12.into());
  }
  let key = &archive.keys[0].key;
  let provider = rustls::crypto::ring::default_provider();
  let mut certs = Vec::new();
  for bag in &archive.certs {
    if !certs
      .iter()
      .any(|cert: &&p12_keystore::Certificate| cert.as_der() == bag.cert.as_der())
    {
      certs.push(&bag.cert);
    }
  }
  let matches: Vec<_> = certs
    .iter()
    .enumerate()
    .filter_map(|(index, cert)| {
      let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.as_der().to_vec()));
      CertifiedKey::from_der(
        vec![CertificateDer::from(cert.as_der().to_vec())],
        private_key,
        &provider,
      )
      .is_ok()
      .then_some(index)
    })
    .collect();
  let leaf = match matches.as_slice() {
    [leaf] => *leaf,
    [] => return Err(RequestIdentityError::InvalidCertificate.into()),
    _ => return Err(RequestIdentityError::AmbiguousPkcs12.into()),
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
  pem_request_identity(&chain, key.as_bytes())
}
