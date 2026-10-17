//! Shared PEM processing; usable key and certificate imports stay with native TLS.
use pkcs8::{
  EncryptedPrivateKeyInfoRef, PrivateKeyInfoRef, SecretDocument,
  der::{Decode, Reader},
};
use zeroize::Zeroizing;

use super::request::RequestIdentityError;

// Adopt decoded blocks immediately, including ignored keys in combined files.
fn blocks(bytes: &[u8]) -> Result<Vec<(String, Zeroizing<Vec<u8>>)>, pem::PemError> {
  Ok(
    pem::parse_many(bytes)?
      .into_iter()
      .map(|block| (block.tag().to_owned(), Zeroizing::new(block.into_contents())))
      .collect(),
  )
}

pub(super) fn private_key(bytes: &[u8], passphrase: Option<&str>) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  let (tag, der) = blocks(bytes)
    .map_err(|_| RequestIdentityError::InvalidKey)?
    .into_iter()
    .find(|(tag, _)| tag.ends_with("PRIVATE KEY"))
    .ok_or(RequestIdentityError::NoKey)?;
  let document = match tag.as_str() {
    "PRIVATE KEY" => SecretDocument::try_from(der.as_slice()).map_err(|_| RequestIdentityError::InvalidKey)?,
    "ENCRYPTED PRIVATE KEY" => decrypt(&der, passphrase)?,
    _ => return Err(RequestIdentityError::UnsupportedKeyFormat.into()),
  };
  PrivateKeyInfoRef::from_der(document.as_bytes()).map_err(|_| RequestIdentityError::InvalidKey)?;
  let pem = document.to_pem("PRIVATE KEY", pkcs8::LineEnding::LF)?;
  Ok(Zeroizing::new(pem.as_bytes().to_vec()))
}

fn decrypt(der: &[u8], passphrase: Option<&str>) -> anyhow::Result<SecretDocument> {
  // pkcs5 collapses unknown outer PBE OIDs into a generic DER value error.
  // Inspect the typed envelope first so legacy PBE is not called malformed.
  let algorithm = pkcs8::der::asn1::AnyRef::from_der(der)
    .and_then(|value| {
      value.sequence(|reader| {
        let algorithm = pkcs8::AlgorithmIdentifierRef::decode(reader)?;
        let _: &pkcs8::der::asn1::OctetStringRef = reader.decode()?;
        Ok(algorithm.oid)
      })
    })
    .map_err(|_| RequestIdentityError::InvalidKey)?;
  if algorithm != pkcs8::pkcs5::pbes2::PBES2_OID {
    return Err(RequestIdentityError::UnsupportedEncryption.into());
  }
  // Decode the full encryption scheme, including its KDF/PRF/cipher, before
  // asking for a password. Unsupported algorithms must not cause prompt loops.
  let encrypted = EncryptedPrivateKeyInfoRef::from_der(der).map_err(|error| match error.kind() {
    pkcs8::der::ErrorKind::OidUnknown { .. } => RequestIdentityError::UnsupportedEncryption,
    _ => RequestIdentityError::InvalidKey,
  })?;
  let passphrase = passphrase.ok_or(RequestIdentityError::NoPassphrase("PEM"))?;
  let mut plaintext = Zeroizing::new(encrypted.encrypted_data.as_bytes().to_vec());
  let decrypted = encrypted
    .encryption_algorithm
    .decrypt_in_place(passphrase, &mut plaintext)
    .map_err(|_| RequestIdentityError::DecryptError("PEM"))?;
  SecretDocument::try_from(decrypted).map_err(|_| RequestIdentityError::DecryptError("PEM").into())
}

pub(super) fn certificate_chain(bytes: &[u8]) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  let mut chain = Zeroizing::new(Vec::new());
  for (tag, der) in blocks(bytes).map_err(|_| RequestIdentityError::InvalidCertificate)? {
    if tag == "CERTIFICATE" {
      x509_cert::Certificate::from_der(&der).map_err(|_| RequestIdentityError::InvalidCertificate)?;
      let encoded = pem::encode_config(
        &pem::Pem::new("CERTIFICATE", der.as_slice()),
        pem::EncodeConfig::new().set_line_ending(pem::LineEnding::LF),
      );
      chain.extend_from_slice(encoded.as_bytes());
    }
  }
  if chain.is_empty() {
    return Err(RequestIdentityError::InvalidCertificate.into());
  }
  Ok(chain)
}
