use pkcs8::SecretDocument;

use super::{identity_encoding::TraditionalKeyFormat, request::RequestIdentityError};

pub(super) fn normalize(
  _bytes: &[u8],
  format: TraditionalKeyFormat,
  _passphrase: Option<&str>,
) -> anyhow::Result<SecretDocument> {
  Err(
    match format {
      TraditionalKeyFormat::EncryptedPem => RequestIdentityError::UnsupportedEncryption,
      TraditionalKeyFormat::PlainPem | TraditionalKeyFormat::Sec1Der => RequestIdentityError::UnsupportedKeyFormat,
    }
    .into(),
  )
}
