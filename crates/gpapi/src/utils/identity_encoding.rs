//! PEM/DER normalization; usable key and certificate imports stay with native TLS.
use pkcs8::{
  AlgorithmIdentifierRef, EncryptedPrivateKeyInfoRef, ObjectIdentifier, PrivateKeyInfoRef, SecretDocument,
  der::{
    Decode, Reader, SliceReader, Tag, Tagged,
    asn1::{AnyRef, OctetStringRef, SequenceRef},
  },
};
use zeroize::Zeroizing;

use super::{request::RequestIdentityError, traditional_key};

pub(super) enum TraditionalKeyFormat {
  Sec1Der,
  PlainPem,
  EncryptedPem,
}

struct PemBlock {
  tag: String,
  headers: pem::HeaderMap,
  contents: Zeroizing<Vec<u8>>,
}

impl PemBlock {
  fn encoded(&self) -> Zeroizing<Vec<u8>> {
    let mut block = pem::Pem::new(&self.tag, self.contents.as_slice());
    *block.headers_mut() = self.headers.clone();
    let encoded = Zeroizing::new(pem::encode(&block).into_bytes());
    // The PEM crate owns a copy of the decoded private key as well.
    drop(Zeroizing::new(block.into_contents()));
    encoded
  }
}

fn is_pem(bytes: &[u8]) -> bool {
  bytes.windows(b"-----BEGIN".len()).any(|part| part == b"-----BEGIN")
}

// Adopt decoded blocks immediately, including ignored keys in combined files.
fn blocks(bytes: &[u8]) -> Result<Vec<PemBlock>, pem::PemError> {
  Ok(
    pem::parse_many(bytes)?
      .into_iter()
      .map(|block| PemBlock {
        tag: block.tag().to_owned(),
        headers: block.headers().clone(),
        contents: Zeroizing::new(block.into_contents()),
      })
      .collect(),
  )
}

pub(super) fn private_key(bytes: &[u8], passphrase: Option<&str>) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  let pem_input = is_pem(bytes);
  let block = if pem_input {
    blocks(bytes)
      .map_err(|_| RequestIdentityError::InvalidKey)?
      .into_iter()
      .find(|block| block.tag.ends_with("PRIVATE KEY"))
      .ok_or(RequestIdentityError::NoKey)?
  } else {
    // Unlike combined PEM, a standalone DER certificate cannot include a key.
    if x509_cert::Certificate::from_der(bytes).is_ok() {
      return Err(RequestIdentityError::NoKey.into());
    }
    PemBlock {
      tag: der_key_tag(bytes)
        .map_err(|_| RequestIdentityError::InvalidKey)?
        .to_owned(),
      headers: pem::HeaderMap::default(),
      contents: Zeroizing::new(bytes.to_vec()),
    }
  };
  let der = &block.contents;
  let document = if traditional_encryption(&block)? {
    if !matches!(block.tag.as_str(), "RSA PRIVATE KEY" | "EC PRIVATE KEY") {
      return Err(RequestIdentityError::InvalidKey.into());
    }
    traditional_key::normalize(&block.encoded(), TraditionalKeyFormat::EncryptedPem, passphrase)?
  } else {
    match block.tag.as_str() {
      "PRIVATE KEY" => SecretDocument::try_from(der.as_slice()).map_err(|_| RequestIdentityError::InvalidKey)?,
      "RSA PRIVATE KEY" => {
        // PKCS#8 stores the original PKCS#1 bytes inside an RSA algorithm envelope.
        // Native TLS validates the RSA key, as it does for existing PKCS#8 input.
        let algorithm = AlgorithmIdentifierRef {
          oid: ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1"),
          parameters: Some(AnyRef::NULL),
        };
        let private_key = OctetStringRef::new(der).map_err(|_| RequestIdentityError::InvalidKey)?;
        let info = PrivateKeyInfoRef::new(algorithm, private_key);
        SecretDocument::encode_msg(&info).map_err(|_| RequestIdentityError::InvalidKey)?
      }
      "ENCRYPTED PRIVATE KEY" => decrypt(der, passphrase)?,
      "EC PRIVATE KEY" => {
        if pem_input {
          traditional_key::normalize(&block.encoded(), TraditionalKeyFormat::PlainPem, passphrase)?
        } else {
          traditional_key::normalize(der, TraditionalKeyFormat::Sec1Der, passphrase)?
        }
      }
      _ => return Err(RequestIdentityError::UnsupportedKeyFormat.into()),
    }
  };
  PrivateKeyInfoRef::from_der(document.as_bytes()).map_err(|_| RequestIdentityError::InvalidKey)?;
  let pem = document.to_pem("PRIVATE KEY", pkcs8::LineEnding::LF)?;
  Ok(Zeroizing::new(pem.as_bytes().to_vec()))
}

// Only choose a container here. The selected decoder/native importer validates
// the key. Decoding the complete outer sequence rejects concatenated DER input.
fn der_key_tag(bytes: &[u8]) -> Result<&'static str, pkcs8::der::Error> {
  let sequence = <&SequenceRef>::from_der(bytes)?;
  let mut reader = SliceReader::new(sequence.as_bytes())?;
  let first: AnyRef = reader.decode()?;
  Ok(match (first.tag(), Tag::peek(&reader)?) {
    (Tag::Integer, Tag::Sequence) => "PRIVATE KEY",
    (Tag::Integer, Tag::Integer) => "RSA PRIVATE KEY",
    (Tag::Integer, Tag::OctetString) => "EC PRIVATE KEY",
    (Tag::Sequence, Tag::OctetString) => "ENCRYPTED PRIVATE KEY",
    _ => return Err(pkcs8::der::ErrorKind::Failed.into()),
  })
}

fn traditional_encryption(block: &PemBlock) -> Result<bool, RequestIdentityError> {
  let mut proc_type = None;
  let mut dek_info = None;
  for (name, value) in block.headers.iter() {
    let slot = match name {
      "Proc-Type" => &mut proc_type,
      "DEK-Info" => &mut dek_info,
      _ => continue,
    };
    if slot.replace(value).is_some() {
      return Err(RequestIdentityError::InvalidKey);
    }
  }
  if proc_type.is_none() && dek_info.is_none() {
    return Ok(false);
  }
  let (Some("4,ENCRYPTED"), Some(dek_info)) = (proc_type, dek_info) else {
    return Err(RequestIdentityError::InvalidKey);
  };
  let (cipher, iv) = dek_info.split_once(',').ok_or(RequestIdentityError::InvalidKey)?;
  let block_size = match cipher {
    "AES-128-CBC" | "AES-192-CBC" | "AES-256-CBC" => 16,
    "DES-EDE3-CBC" => 8,
    _ => return Err(RequestIdentityError::UnsupportedEncryption),
  };
  if iv.len() != block_size * 2
    || !iv.bytes().all(|byte| byte.is_ascii_hexdigit())
    || block.contents.is_empty()
    || block.contents.len() % block_size != 0
  {
    return Err(RequestIdentityError::InvalidKey);
  }
  Ok(true)
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
    // PKCS#12 PBE-SHA1-3DES inside standalone PKCS#8 is not implemented by
    // RustCrypto. Only this known legacy scheme uses the optional executable.
    if algorithm == pkcs8::ObjectIdentifier::new_unwrap("1.2.840.113549.1.12.1.3") {
      return super::legacy_pkcs8::decrypt(der, passphrase);
    }
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
  if !is_pem(bytes) {
    return certificate_pem(bytes).map(|pem| Zeroizing::new(pem.into_bytes()));
  }
  let mut chain = Zeroizing::new(Vec::new());
  for block in blocks(bytes).map_err(|_| RequestIdentityError::InvalidCertificate)? {
    if block.tag == "CERTIFICATE" {
      let encoded = certificate_pem(&block.contents)?;
      chain.extend_from_slice(encoded.as_bytes());
    }
  }
  if chain.is_empty() {
    return Err(RequestIdentityError::InvalidCertificate.into());
  }
  Ok(chain)
}

fn certificate_pem(der: &[u8]) -> anyhow::Result<String> {
  x509_cert::Certificate::from_der(der).map_err(|_| RequestIdentityError::InvalidCertificate)?;
  Ok(pem::encode_config(
    &pem::Pem::new("CERTIFICATE", der),
    pem::EncodeConfig::new().set_line_ending(pem::LineEnding::LF),
  ))
}
