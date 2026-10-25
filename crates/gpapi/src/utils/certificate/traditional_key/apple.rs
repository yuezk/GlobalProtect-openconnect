use pkcs8::SecretDocument;

use crate::utils::{identity_encoding::TraditionalKeyFormat, request::RequestIdentityError};

pub(in crate::utils) fn normalize(
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
