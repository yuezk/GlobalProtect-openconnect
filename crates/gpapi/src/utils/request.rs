use std::{fmt, fs, io::Write, path::Path, sync::Arc};

use anyhow::{Context, ensure};
use reqwest::Identity;
use zeroize::Zeroizing;

use super::{identity_encoding, pkcs12_identity};

#[derive(Debug, thiserror::Error)]
pub enum RequestIdentityError {
  #[error("Failed to find the private key")]
  NoKey,
  #[error("No passphrase provided")]
  NoPassphrase(&'static str),
  #[error("Failed to decrypt private key")]
  DecryptError(&'static str),
  #[error(
    "Unsupported private key encryption. Convert the key to modern PKCS#8 using PBES2 with AES before importing it"
  )]
  UnsupportedEncryption,
  #[error(
    "Could not convert the legacy private key with OpenSSL: {0}. Convert the key to modern PKCS#8 using PBES2 with AES before importing it"
  )]
  LegacyKeyConversion(&'static str),
  #[error("Unsupported private key format. Convert the key to PKCS#8 before importing it")]
  UnsupportedKeyFormat,
  #[error("Invalid private key")]
  InvalidKey,
  #[error("Invalid certificate or certificate chain")]
  InvalidCertificate,
  #[error("The PKCS#12 file must contain both a client certificate and its private key")]
  IncompletePkcs12,
  #[error("Could not open the PKCS#12 certificate. Check the password and certificate file")]
  InvalidPkcs12(#[source] reqwest::Error),
}

/// Loaded once by the authentication owner. PEM keys are decrypted once; PKCS#12
/// retains its original bytes and passphrase so each native backend imports the
/// same identity and chain. Debug never exposes retained material.
#[derive(Clone)]
pub struct ClientIdentity {
  material: Arc<IdentityMaterial>,
  identity: Identity,
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
    let identity = pem_request_identity(&certificate, &key)?;
    Ok(Self {
      material: Arc::new(IdentityMaterial {
        certificate,
        key: Some(key),
        passphrase: None,
      }),
      identity,
    })
  }

  /// A separate key denotes decrypted PEM; no separate key denotes original
  /// PKCS#12 bytes. Input-size limits belong to the service transport boundary.
  pub fn from_data(
    certificate: Zeroizing<Vec<u8>>,
    key: Option<Zeroizing<Vec<u8>>>,
    passphrase: Option<&str>,
  ) -> anyhow::Result<Self> {
    let (identity, passphrase) = match &key {
      Some(key) => (pem_request_identity(&certificate, key)?, None),
      None => {
        pkcs12_identity::ensure_complete(&certificate, passphrase.unwrap_or_default())?;
        // A blank settings field is stored as None. Try the empty password so
        // passwordless archives work, but keep prompting for protected ones.
        let identity = Identity::from_pkcs12_der(&certificate, passphrase.unwrap_or_default()).map_err(|error| {
          if passphrase.is_none() {
            RequestIdentityError::NoPassphrase("PKCS#12")
          } else {
            RequestIdentityError::InvalidPkcs12(error)
          }
        })?;
        (
          identity,
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
      identity,
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
  pub(crate) fn request_identity(&self) -> Identity {
    self.identity.clone()
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

fn pem_request_identity(certificate: &[u8], key: &[u8]) -> anyhow::Result<Identity> {
  let error_context =
    "Client certificate and private key cannot be used together. Check that the private key matches the certificate";
  let identity = Identity::from_pkcs8_pem(certificate, key).context(error_context)?;
  // Native TLS may defer the certificate/key match check until connector creation.
  // Build the same client used for requests without making a network connection.
  reqwest::Client::builder()
    .no_proxy()
    .identity(identity.clone())
    .build()
    .context(error_context)?;
  Ok(identity)
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

/// General request clients may load an identity directly. Session owners call
/// ClientIdentity::load once and retain that snapshot across their lifecycle.
pub fn create_identity(cert: &str, key: Option<&str>, passphrase: Option<&str>) -> anyhow::Result<Identity> {
  Ok(ClientIdentity::load(cert, key, passphrase)?.request_identity())
}

#[cfg(test)]
mod tests {
  use super::*;

  const CERTIFICATE: &[u8] = include_bytes!("../../tests/files/client-identity/rsa-cert.pem");
  const KEY: &[u8] = include_bytes!("../../tests/files/client-identity/rsa-pkcs8.pem");
  const ENCRYPTED_KEY: &[u8] = include_bytes!("../../tests/files/client-identity/rsa-pbes2-sha256.pem");

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
  fn create_identity_preserves_password_errors_and_builds_a_native_request_client() {
    let mut source = tempfile::NamedTempFile::new().unwrap();
    source.write_all(CERTIFICATE).unwrap();
    source.write_all(ENCRYPTED_KEY).unwrap();
    for (password, missing) in [(None, true), (Some("wrong"), false)] {
      let error = create_identity(source.path().to_str().unwrap(), None, password).unwrap_err();
      match error.downcast_ref::<RequestIdentityError>() {
        Some(RequestIdentityError::NoPassphrase("PEM")) => assert!(missing),
        Some(RequestIdentityError::DecryptError("PEM")) => assert!(!missing),
        other => panic!("Unexpected identity error: {other:?}"),
      }
    }
    let identity = create_identity(source.path().to_str().unwrap(), None, Some("fixture-password")).unwrap();
    drop(source);
    reqwest::Client::builder().identity(identity).build().unwrap();
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
    reqwest::Client::builder()
      .identity(identity.request_identity())
      .build()
      .unwrap();
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
