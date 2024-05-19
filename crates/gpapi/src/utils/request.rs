use std::fs;

use anyhow::bail;
use log::warn;
use openssl::pkey::PKey;
use pem::parse_many;
use reqwest::Identity;

#[derive(Debug, thiserror::Error)]
pub enum RequestIdentityError {
  #[error("Failed to find the private key")]
  NoKey,
  #[error("No passphrase provided")]
  NoPassphrase(&'static str),
  #[error("Failed to decrypt private key")]
  DecryptError(&'static str),
}

/// Create an identity object from a certificate and key
pub fn create_identity_from_pem(cert: &str, key: Option<&str>, passphrase: Option<&str>) -> anyhow::Result<Identity> {
  let cert_pem = fs::read(cert).map_err(|err| anyhow::anyhow!("Failed to read certificate file: {}", err))?;

  // Get the private key pem
  let key_pem = match key {
    Some(key) => {
      let pem_file = fs::read(key).map_err(|err| anyhow::anyhow!("Failed to read key file: {}", err))?;
      pem::parse(pem_file)?
    }
    None => {
      // If key is not provided, find the private key in the cert pem
      parse_many(&cert_pem)?
        .into_iter()
        .find(|pem| pem.tag().ends_with("PRIVATE KEY"))
        .ok_or(RequestIdentityError::NoKey)?
    }
  };

  // The key pem could be encrypted, so we need to decrypt it
  let decrypted_key_pem = if key_pem.tag().ends_with("ENCRYPTED PRIVATE KEY") {
    let passphrase = passphrase.ok_or_else(|| {
      warn!("Key is encrypted but no passphrase provided");
      RequestIdentityError::NoPassphrase("PEM")
    })?;
    let pem_content = pem::encode(&key_pem);
    let key = PKey::private_key_from_pem_passphrase(pem_content.as_bytes(), passphrase.as_bytes()).map_err(|err| {
      warn!("Failed to decrypt key: {}", err);
      RequestIdentityError::DecryptError("PEM")
    })?;

    key.private_key_to_pem_pkcs8()?
  } else {
    pem::encode(&key_pem).into()
  };

  let identity = Identity::from_pkcs8_pem(&cert_pem, &decrypted_key_pem)?;
  Ok(identity)
}

pub fn create_identity_from_pkcs12(pkcs12: &str, passphrase: Option<&str>) -> anyhow::Result<Identity> {
  let pkcs12 = fs::read(pkcs12)?;

  let Some(passphrase) = passphrase else {
    bail!(RequestIdentityError::NoPassphrase("PKCS#12"));
  };

  let identity = Identity::from_pkcs12_der(&pkcs12, passphrase)?;
  Ok(identity)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn create_identity_from_pem_requires_passphrase() {
    let cert = "tests/files/badssl.com-client.pem";
    let identity = create_identity_from_pem(cert, None, None);

    assert!(identity.is_err());
    assert!(identity.unwrap_err().to_string().contains("No passphrase provided"));
  }

  #[test]
  fn create_identity_from_pem_with_passphrase() {
    let cert = "tests/files/badssl.com-client.pem";
    let passphrase = "badssl.com";

    let identity = create_identity_from_pem(cert, None, Some(passphrase));

    assert!(identity.is_ok());
  }
}
