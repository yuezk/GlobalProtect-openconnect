pub(in crate::utils) fn decrypt(
  _bytes: &[u8],
  _pem: bool,
  _passphrase: Option<&str>,
) -> anyhow::Result<pkcs8::SecretDocument> {
  Err(crate::utils::request::RequestIdentityError::UnsupportedEncryption.into())
}

pub(in crate::utils) fn archive(
  _bytes: &[u8],
  _passphrase: Option<&str>,
) -> anyhow::Result<(zeroize::Zeroizing<Vec<u8>>, pkcs8::SecretDocument)> {
  Err(crate::utils::request::RequestIdentityError::UnsupportedEncryption.into())
}
