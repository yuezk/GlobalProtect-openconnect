use openssl::{
  error::ErrorStack,
  pkey::{Id, PKey},
};
use pkcs8::SecretDocument;
use zeroize::Zeroizing;

use super::{identity_encoding::TraditionalKeyFormat, request::RequestIdentityError};

pub(super) fn normalize(
  bytes: &[u8],
  format: TraditionalKeyFormat,
  passphrase: Option<&str>,
) -> anyhow::Result<SecretDocument> {
  let key = match format {
    TraditionalKeyFormat::Sec1Der => PKey::private_key_from_der(bytes),
    TraditionalKeyFormat::PlainPem => PKey::private_key_from_pem_passphrase(bytes, b""),
    TraditionalKeyFormat::EncryptedPem => {
      let passphrase = passphrase.ok_or(RequestIdentityError::NoPassphrase("PEM"))?;
      anyhow::ensure!(
        !passphrase.as_bytes().contains(&0),
        "Private key passphrases containing NUL are not supported"
      );
      PKey::private_key_from_pem_passphrase(bytes, passphrase.as_bytes())
    }
  }
  .map_err(import_error)?;
  if !matches!(key.id(), Id::RSA | Id::EC) {
    return Err(RequestIdentityError::UnsupportedKeyFormat.into());
  }
  let der = Zeroizing::new(
    key
      .private_key_to_pkcs8()
      .map_err(|_| RequestIdentityError::InvalidKey)?,
  );
  Ok(SecretDocument::try_from(der.as_slice()).map_err(|_| RequestIdentityError::InvalidKey)?)
}

fn import_error(error: ErrorStack) -> RequestIdentityError {
  // Inspect only known backend reasons. Never expose the native error stack or
  // turn an unclassified decoder failure into a password retry loop.
  if error.errors().iter().any(|error| error.reason() == Some("bad decrypt")) {
    RequestIdentityError::DecryptError("PEM")
  } else if error.errors().iter().any(|error| error.reason() == Some("unsupported")) {
    RequestIdentityError::UnsupportedEncryption
  } else {
    RequestIdentityError::InvalidKey
  }
}
