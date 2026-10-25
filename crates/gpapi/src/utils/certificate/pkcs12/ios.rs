use crate::utils::request::RequestIdentityError;
use security_framework::import_export::Pkcs12ImportOptions;
pub(in crate::utils) fn ensure_complete(bytes: &[u8], password: &str) -> anyhow::Result<()> {
  if let Ok(imports) = Pkcs12ImportOptions::new().passphrase(password).import(bytes)
    && !imports.last().is_some_and(|item| item.identity.is_some())
  {
    return Err(RequestIdentityError::IncompletePkcs12.into());
  }
  Ok(())
}
