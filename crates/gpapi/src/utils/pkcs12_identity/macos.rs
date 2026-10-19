use security_framework::{import_export::Pkcs12ImportOptions, os::macos::keychain::CreateOptions};

use super::request::RequestIdentityError;

pub(super) fn ensure_complete(bytes: &[u8], password: &str) -> anyhow::Result<()> {
  // native-tls assumes a successful import has a last item with an identity.
  // Incomplete archives violate that assumption and panic. Inspect them first
  // in a private temporary keychain, never the user's default keychain.
  let directory = tempfile::tempdir()?;
  let keychain = CreateOptions::new()
    .password(&uuid::Uuid::new_v4().to_string())
    .create(directory.path().join("identity.keychain"))?;
  if let Ok(imports) = Pkcs12ImportOptions::new()
    .keychain(keychain)
    .passphrase(password)
    .import(bytes)
    && !imports.last().is_some_and(|item| item.identity.is_some())
  {
    return Err(RequestIdentityError::IncompletePkcs12.into());
  }
  // Leave failed imports to reqwest so password errors retain their existing
  // classification and native error chain.
  Ok(())
}
