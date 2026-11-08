//! Select an archive importer from decoded encryption metadata, before passwords.
use cms::encrypted_data::EncryptedData;
use pkcs8::{
  ObjectIdentifier,
  der::{
    Decode, Encode,
    asn1::{ContextSpecific, OctetString},
  },
};
use pkcs12::{
  AuthenticatedSafe,
  pbe_params::{EncryptedPrivateKeyInfo, Pkcs12PbeParams},
  pfx::Pfx,
  safe_bag::SafeContents,
};

use crate::utils::request::RequestIdentityError;

const DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");
const ENCRYPTED_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.6");
const ENCRYPTED_KEY: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.12.10.1.2");
const PBES2: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.5.13");
const RC4_128: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.12.1.1");
const DES3: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.12.1.3");
const RC2_40: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.12.1.6");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArchiveBackend {
  Rust,
  NativeRc4,
}

fn invalid(_: pkcs8::der::Error) -> RequestIdentityError {
  RequestIdentityError::InvalidCertificate
}

/// Inspect only visible metadata. Encrypted safe contents remain opaque until
/// the selected importer verifies the MAC and decrypts them.
pub(super) fn backend(bytes: &[u8]) -> Result<ArchiveBackend, RequestIdentityError> {
  let pfx = Pfx::from_der(bytes).map_err(invalid)?;
  if pfx.auth_safe.content_type != DATA {
    return Err(RequestIdentityError::UnsupportedEncryption);
  }
  let contents = pfx.auth_safe.content.decode_as::<OctetString>().map_err(invalid)?;
  let safes = AuthenticatedSafe::from_der(contents.as_bytes()).map_err(invalid)?;
  let mut selected = ArchiveBackend::Rust;
  for safe in safes {
    if safe.content_type == ENCRYPTED_DATA {
      let encrypted = safe.content.decode_as::<EncryptedData>().map_err(invalid)?;
      let algorithm = encrypted.enc_content_info.content_enc_alg;
      inspect_algorithm(algorithm.oid, algorithm.parameters.as_ref(), &mut selected)?;
    } else if safe.content_type == DATA {
      let contents = safe.content.decode_as::<OctetString>().map_err(invalid)?;
      let bags = SafeContents::from_der(contents.as_bytes()).map_err(invalid)?;
      for bag in bags {
        if bag.bag_id == ENCRYPTED_KEY {
          let encrypted = ContextSpecific::<EncryptedPrivateKeyInfo>::from_der(&bag.bag_value).map_err(invalid)?;
          let algorithm = encrypted.value.encryption_algorithm;
          inspect_algorithm(algorithm.oid, algorithm.parameters.as_ref(), &mut selected)?;
        }
      }
    } else {
      return Err(RequestIdentityError::UnsupportedEncryption);
    }
  }
  Ok(selected)
}

fn inspect_algorithm(
  oid: ObjectIdentifier,
  parameters: Option<&pkcs8::der::Any>,
  selected: &mut ArchiveBackend,
) -> Result<(), RequestIdentityError> {
  let parameters = parameters
    .ok_or(RequestIdentityError::InvalidCertificate)?
    .to_der()
    .map_err(invalid)?;
  if oid == PBES2 {
    pkcs8::pkcs5::pbes2::Parameters::from_der(&parameters).map_err(|error| match error.kind() {
      pkcs8::der::ErrorKind::OidUnknown { .. } => RequestIdentityError::UnsupportedEncryption,
      _ => RequestIdentityError::InvalidCertificate,
    })?;
  } else if matches!(oid, RC4_128 | DES3 | RC2_40) {
    Pkcs12PbeParams::from_der(&parameters).map_err(invalid)?;
    if oid == RC4_128 {
      *selected = ArchiveBackend::NativeRc4;
    }
  } else {
    return Err(RequestIdentityError::UnsupportedEncryption);
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn published_archive_formats_choose_their_importer_from_metadata() {
    for name in [
      "modern.p12",
      "legacy-rc2.p12",
      "empty-password.p12",
      "unicode-password.p12",
      "no-bag-ids.p12",
    ] {
      let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/files/client-identity")
        .join(name);
      assert_eq!(
        backend(&std::fs::read(path).unwrap()).unwrap(),
        ArchiveBackend::Rust,
        "{name}"
      );
    }
    assert_eq!(
      backend(include_bytes!(
        "../../../tests/files/client-identity/legacy-RC4-128.p12"
      ))
      .unwrap(),
      ArchiveBackend::NativeRc4
    );
  }

  #[test]
  fn unsupported_archive_encryption_is_terminal_before_password_handling() {
    for bytes in [
      include_bytes!("../../../tests/files/client-identity/legacy-RC2-128.p12").as_slice(),
      include_bytes!("../../../tests/files/client-identity/legacy-2DES.p12").as_slice(),
    ] {
      assert!(matches!(
        backend(bytes),
        Err(RequestIdentityError::UnsupportedEncryption)
      ));
    }
  }
}
