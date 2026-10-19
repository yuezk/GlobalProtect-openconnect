use openssl::pkcs12::Pkcs12;

use super::request::RequestIdentityError;

pub(super) fn ensure_complete(bytes: &[u8], password: &str) -> anyhow::Result<()> {
  // Parse errors belong to the existing native TLS importer. A successful parse
  // lets us report incomplete archives without suggesting a password change.
  if let Ok(archive) = Pkcs12::from_der(bytes).and_then(|archive| archive.parse2(password))
    && (archive.pkey.is_none() || archive.cert.is_none())
  {
    return Err(RequestIdentityError::IncompletePkcs12.into());
  }
  Ok(())
}
