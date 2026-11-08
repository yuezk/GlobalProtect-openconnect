use std::{fmt, fs, io::Write, path::Path, sync::Arc};

use anyhow::{Context, ensure};
use rustls::sign::CertifiedKey;
use zeroize::Zeroizing;

use super::{certificate::pkcs12, client_tls, identity_encoding};

#[derive(Debug, thiserror::Error)]
pub enum RequestIdentityError {
  #[error("No private key was found. Choose the matching key file")]
  NoKey,
  #[error("This certificate or key is password-protected. Enter its password")]
  NoPassphrase(&'static str),
  #[error("Could not unlock the certificate or key. Check the password and file")]
  DecryptError(&'static str),
  #[error("This certificate or key uses unsupported encryption. Re-export it with AES encryption")]
  UnsupportedEncryption,
  #[error("This private key format is not supported. Choose a PEM key or a PKCS#12 certificate file")]
  UnsupportedKeyFormat,
  #[error(
    "This client certificate uses an unsupported key type ({0}). Ask your VPN administrator for an RSA or P-256 certificate"
  )]
  UnsupportedKeyAlgorithm(&'static str),
  #[error("The private key does not match this certificate. Choose the matching key file")]
  KeyMismatch,
  #[error("The private key file is invalid. Choose a different key file")]
  InvalidKey,
  #[error("The certificate file is invalid. Choose a different certificate file")]
  InvalidCertificate,
  #[error("The PKCS#12 file must contain both a client certificate and its private key")]
  IncompletePkcs12,
  #[error("Could not open the PKCS#12 file. Check the password or choose another file")]
  InvalidPkcs12(#[source] p12_keystore::error::Error),
  #[error(
    "The certificate file contains more than one matching identity. Choose a file with one client certificate and key"
  )]
  AmbiguousPkcs12,
}

/// Loaded once by the authentication owner. PEM keys are decrypted once; PKCS#12
/// retains its original bytes and passphrase so each native backend imports the
/// same identity and chain. Debug never exposes retained material.
#[derive(Clone)]
pub struct ClientIdentity {
  material: Arc<IdentityMaterial>,
  certified_key: Arc<CertifiedKey>,
}

struct IdentityMaterial {
  certificate: Zeroizing<Vec<u8>>,
  key: Option<Zeroizing<Vec<u8>>>,
  passphrase: Option<Zeroizing<String>>,
}

impl fmt::Debug for ClientIdentity {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("ClientIdentity")
      .field("material", &"<redacted>")
      .finish()
  }
}

impl ClientIdentity {
  pub fn load(certificate: &str, key: Option<&str>, passphrase: Option<&str>) -> anyhow::Result<Self> {
    let certificate_bytes = read_identity_file(certificate)?;
    let extension = Path::new(certificate).extension().and_then(|value| value.to_str());
    if extension.is_some_and(|value| value.eq_ignore_ascii_case("p12") || value.eq_ignore_ascii_case("pfx")) {
      return Self::from_data(certificate_bytes, None, passphrase);
    }
    let key_bytes = match key {
      Some(path) => read_identity_file(path)?,
      None => certificate_bytes.clone(),
    };
    let key = identity_encoding::private_key(&key_bytes, passphrase)?;
    let certificate = identity_encoding::certificate_chain(&certificate_bytes)?;
    let certified_key = client_tls::pem_certified_key(&certificate, &key)?;
    Ok(Self {
      material: Arc::new(IdentityMaterial {
        certificate,
        key: Some(key),
        passphrase: None,
      }),
      certified_key,
    })
  }

  /// A separate key denotes decrypted PEM; no separate key denotes original
  /// PKCS#12 bytes. Input-size limits belong to the service transport boundary.
  pub fn from_data(
    certificate: Zeroizing<Vec<u8>>,
    key: Option<Zeroizing<Vec<u8>>>,
    passphrase: Option<&str>,
  ) -> anyhow::Result<Self> {
    let (certified_key, passphrase) = match &key {
      Some(key) => (client_tls::pem_certified_key(&certificate, key)?, None),
      None => {
        let certified_key = pkcs12::certified_key(&certificate, passphrase)?;
        (
          certified_key,
          Some(Zeroizing::new(passphrase.unwrap_or_default().to_owned())),
        )
      }
    };
    Ok(Self {
      material: Arc::new(IdentityMaterial {
        certificate,
        key,
        passphrase,
      }),
      certified_key,
    })
  }

  pub fn certificate_data(&self) -> &[u8] {
    &self.material.certificate
  }
  pub fn key_data(&self) -> Option<&[u8]> {
    self.material.key.as_ref().map(|key| key.as_slice())
  }
  pub fn key_password(&self) -> Option<&str> {
    self.material.passphrase.as_ref().map(|value| value.as_str())
  }
  pub(crate) fn client_builder(&self, ignore_tls_errors: bool) -> anyhow::Result<reqwest::ClientBuilder> {
    client_tls::identity_client_builder(self.certified_key.clone(), ignore_tls_errors)
  }

  pub fn write_files(&self, directory: Option<&Path>) -> anyhow::Result<ClientIdentityFiles> {
    let suffix = if self.material.key.is_some() { ".pem" } else { ".p12" };
    let certificate = write_identity_file(self.certificate_data(), directory, suffix)?;
    let key = self
      .key_data()
      .map(|key| write_identity_file(key, directory, ".pem"))
      .transpose()?;
    Ok(ClientIdentityFiles { certificate, key })
  }
}

/// Named files are private and remain owned until native workers and logout
/// finish. The original configured files are never reopened by these workers.
pub struct ClientIdentityFiles {
  certificate: tempfile::NamedTempFile,
  key: Option<tempfile::NamedTempFile>,
}

impl ClientIdentityFiles {
  pub fn certificate(&self) -> &str {
    self.certificate.path().to_str().expect("identity path is UTF-8")
  }
  pub fn key(&self) -> Option<&str> {
    self
      .key
      .as_ref()
      .map(|key| key.path().to_str().expect("identity path is UTF-8"))
  }
}

fn read_identity_file(path: &str) -> anyhow::Result<Zeroizing<Vec<u8>>> {
  Ok(Zeroizing::new(
    fs::read(path).context("Failed to read client identity")?,
  ))
}

fn write_identity_file(
  bytes: &[u8],
  directory: Option<&Path>,
  suffix: &str,
) -> anyhow::Result<tempfile::NamedTempFile> {
  let mut builder = tempfile::Builder::new();
  builder.prefix("gp-identity-").suffix(suffix);
  let mut file = match directory {
    Some(directory) => builder.tempfile_in(directory)?,
    None => builder.tempfile()?,
  };
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    file.as_file().set_permissions(fs::Permissions::from_mode(0o600))?;
  }
  ensure!(file.path().to_str().is_some(), "Client identity path is not UTF-8");
  file.write_all(bytes)?;
  file.flush()?;
  Ok(file)
}

/// Ordinary Reqwest construction with the shared rustls provider initialized.
pub fn client_builder() -> reqwest::ClientBuilder {
  if rustls::crypto::CryptoProvider::get_default().is_none() {
    let _ = rustls::crypto::ring::default_provider().install_default();
  }
  reqwest::Client::builder().tls_backend_rustls()
}

#[cfg(test)]
mod tests {
  use super::*;

  const CERTIFICATE: &[u8] = include_bytes!("../../tests/files/client-identity/rsa-cert.pem");
  const KEY: &[u8] = include_bytes!("../../tests/files/client-identity/rsa-pkcs8.pem");
  const ENCRYPTED_KEY: &[u8] = include_bytes!("../../tests/files/client-identity/rsa-pbes2-sha256.pem");

  #[test]
  fn unsupported_identity_errors_explain_the_action_without_blaming_passwords_or_key_matching() {
    for error in [
      RequestIdentityError::UnsupportedKeyAlgorithm("RSA-PSS"),
      RequestIdentityError::UnsupportedEncryption,
      RequestIdentityError::UnsupportedKeyFormat,
    ] {
      let message = error.to_string();
      assert!(
        message.contains("unsupported") || message.contains("not supported"),
        "{message}"
      );
      assert!(
        message.contains("Ask ") || message.contains("Re-export ") || message.contains("Choose "),
        "{message}"
      );
      assert!(
        !message.contains("password") && !message.contains("does not match"),
        "{message}"
      );
    }
  }

  #[test]
  fn pem_without_key_reports_missing_key_even_with_passphrase() {
    let mut source = tempfile::NamedTempFile::new().unwrap();
    source.write_all(CERTIFICATE).unwrap();
    for passphrase in [None, Some("unused")] {
      let error = ClientIdentity::load(source.path().to_str().unwrap(), None, passphrase).unwrap_err();
      assert!(matches!(
        error.downcast_ref::<RequestIdentityError>(),
        Some(RequestIdentityError::NoKey)
      ));
    }
  }

  #[test]
  fn local_identity_loading_is_not_limited_by_service_message_size() {
    let mut source = tempfile::NamedTempFile::new().unwrap();
    source.write_all(CERTIFICATE).unwrap();
    source.write_all(KEY).unwrap();
    source.write_all(&vec![b'\n'; 33 * 1024]).unwrap();
    let identity = ClientIdentity::load(source.path().to_str().unwrap(), None, None).unwrap();
    assert_eq!(identity.certificate_data(), CERTIFICATE);
    assert_eq!(identity.key_data().unwrap(), KEY);
  }

  #[test]
  fn identity_loading_preserves_password_errors_and_builds_a_request_client() {
    let mut source = tempfile::NamedTempFile::new().unwrap();
    source.write_all(CERTIFICATE).unwrap();
    source.write_all(ENCRYPTED_KEY).unwrap();
    for (password, missing) in [(None, true), (Some("wrong"), false)] {
      let error = ClientIdentity::load(source.path().to_str().unwrap(), None, password).unwrap_err();
      match error.downcast_ref::<RequestIdentityError>() {
        Some(RequestIdentityError::NoPassphrase("PEM")) => assert!(missing),
        Some(RequestIdentityError::DecryptError("PEM")) => assert!(!missing),
        other => panic!("Unexpected identity error: {other:?}"),
      }
    }
    let identity = ClientIdentity::load(source.path().to_str().unwrap(), None, Some("fixture-password")).unwrap();
    drop(source);
    identity.client_builder(false).unwrap().build().unwrap();
  }

  #[test]
  fn unencrypted_key_ignores_unused_password_and_archive_owned_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let identity = ClientIdentity::from_data(
      Zeroizing::new(CERTIFICATE.to_vec()),
      Some(Zeroizing::new(KEY.to_vec())),
      Some("unused password"),
    )
    .unwrap();
    assert!(identity.key_password().is_none());
    let archive = include_bytes!("../../tests/files/client-identity/modern.p12").to_vec();
    let identity = ClientIdentity::from_data(Zeroizing::new(archive), None, Some("fixture-password")).unwrap();
    let files = identity.write_files(None).unwrap();
    let path = files.certificate().to_owned();
    assert!(path.ends_with(".p12"));
    assert!(files.key().is_none());
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    drop(files);
    assert!(!Path::new(&path).exists());
    identity.client_builder(false).unwrap().build().unwrap();
  }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ResponseBodyError {
  #[error("Response read failed: {0}")]
  Transport(#[from] reqwest::Error),
  #[error("Response exceeds its size limit")]
  TooLarge,
}

pub(crate) async fn read_bounded_response(
  mut response: reqwest::Response,
  limit: usize,
) -> Result<Vec<u8>, ResponseBodyError> {
  if response.content_length().is_some_and(|length| length > limit as u64) {
    return Err(ResponseBodyError::TooLarge);
  }
  let mut body = Vec::new();
  while let Some(chunk) = response.chunk().await? {
    if chunk.len() > limit.saturating_sub(body.len()) {
      return Err(ResponseBodyError::TooLarge);
    }
    body.extend_from_slice(&chunk);
  }
  Ok(body)
}
