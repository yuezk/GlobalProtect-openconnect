use std::{fmt, fs, io::Write, path::Path, sync::Arc};

use anyhow::{Context, ensure};
use openssl::{pkey::PKey, x509::X509};
use reqwest::Identity;
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub enum RequestIdentityError {
  #[error("Failed to find the private key")]
  NoKey,
  #[error("No passphrase provided")]
  NoPassphrase(&'static str),
  #[error("Failed to decrypt private key")]
  DecryptError(&'static str),
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
    if certificate.ends_with(".p12") || certificate.ends_with(".pfx") {
      let passphrase = passphrase.ok_or(RequestIdentityError::NoPassphrase("PKCS#12"))?;
      return Self::from_data(certificate_bytes, None, Some(passphrase));
    }
    let key_bytes = match key {
      Some(path) => read_identity_file(path)?,
      None => certificate_bytes.clone(),
    };
    // Keep the previous first-private-key rule: unrelated later blocks must not
    // change which key is selected or whether the caller is prompted.
    let key = pem::parse_many(&*key_bytes)?
      .into_iter()
      .find(|pem| pem.tag().ends_with("PRIVATE KEY"))
      .ok_or(RequestIdentityError::NoKey)?;
    let key_pem = Zeroizing::new(pem::encode(&key));
    let key = if key.tag().ends_with("ENCRYPTED PRIVATE KEY") {
      let passphrase = passphrase.ok_or(RequestIdentityError::NoPassphrase("PEM"))?;
      let key = PKey::private_key_from_pem_passphrase(key_pem.as_bytes(), passphrase.as_bytes())
        .map_err(|_| RequestIdentityError::DecryptError("PEM"))?;
      Zeroizing::new(key.private_key_to_pem_pkcs8()?)
    } else {
      Zeroizing::new(key_pem.as_bytes().to_vec())
    };
    // Strip private-key blocks before importing the certificate chain: macOS
    // otherwise imports the same key twice for combined PEM files.
    let mut certificate = Zeroizing::new(Vec::new());
    for cert in X509::stack_from_pem(&certificate_bytes)? {
      certificate.extend(cert.to_pem()?);
    }
    let identity = Identity::from_pkcs8_pem(&certificate, &key)?;
    let key = Zeroizing::new(PKey::private_key_from_pem(&key)?.private_key_to_pem_pkcs8()?);
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
      Some(key) => (Identity::from_pkcs8_pem(&certificate, key)?, None),
      None => {
        let passphrase = passphrase.ok_or(RequestIdentityError::NoPassphrase("PKCS#12"))?;
        (
          Identity::from_pkcs12_der(&certificate, passphrase)?,
          Some(Zeroizing::new(passphrase.to_owned())),
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
  use openssl::pkcs12::Pkcs12;

  fn generated_identity() -> (Vec<u8>, Vec<u8>) {
    use openssl::{asn1::Asn1Time, hash::MessageDigest, rsa::Rsa, x509::X509NameBuilder};
    let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "identity-fixture").unwrap();
    let name = name.build();
    let mut certificate = X509::builder().unwrap();
    certificate.set_version(2).unwrap();
    certificate.set_subject_name(&name).unwrap();
    certificate.set_issuer_name(&name).unwrap();
    certificate.set_pubkey(&key).unwrap();
    certificate
      .set_not_before(&Asn1Time::days_from_now(0).unwrap())
      .unwrap();
    certificate.set_not_after(&Asn1Time::days_from_now(1).unwrap()).unwrap();
    certificate.sign(&key, MessageDigest::sha256()).unwrap();
    (
      certificate.build().to_pem().unwrap(),
      key.private_key_to_pem_pkcs8().unwrap(),
    )
  }

  #[test]
  fn pem_uses_first_private_key_without_prompting_for_later_encrypted_keys() {
    let (certificate, key) = generated_identity();
    let encrypted = PKey::private_key_from_pem(&key)
      .unwrap()
      .private_key_to_pem_pkcs8_passphrase(openssl::symm::Cipher::aes_256_cbc(), b"unused")
      .unwrap();
    let mut source = tempfile::NamedTempFile::new().unwrap();
    source.write_all(&certificate).unwrap();
    source.write_all(&key).unwrap();
    source.write_all(&encrypted).unwrap();
    let identity = ClientIdentity::load(source.path().to_str().unwrap(), None, None).unwrap();
    assert_eq!(identity.key_data().unwrap(), key);
  }

  #[test]
  fn pem_without_key_reports_missing_key_even_with_passphrase() {
    let (certificate, _) = generated_identity();
    let mut source = tempfile::NamedTempFile::new().unwrap();
    source.write_all(&certificate).unwrap();
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
    let (certificate, key) = generated_identity();
    let mut source = tempfile::NamedTempFile::new().unwrap();
    source.write_all(&certificate).unwrap();
    source.write_all(&key).unwrap();
    source.write_all(&vec![b'\n'; 33 * 1024]).unwrap();
    let identity = ClientIdentity::load(source.path().to_str().unwrap(), None, None).unwrap();
    assert_eq!(identity.certificate_data(), certificate);
    assert_eq!(identity.key_data().unwrap(), key);
  }

  #[test]
  fn pem_passphrases_preserve_prompt_errors_and_accept_separate_keys() {
    let (certificate, key) = generated_identity();
    let mut cert_file = tempfile::NamedTempFile::new().unwrap();
    cert_file.write_all(&certificate).unwrap();
    let mut key_file = tempfile::NamedTempFile::new().unwrap();
    let encrypted = PKey::private_key_from_pem(&key)
      .unwrap()
      .private_key_to_pem_pkcs8_passphrase(openssl::symm::Cipher::aes_256_cbc(), b"correct")
      .unwrap();
    key_file.write_all(&encrypted).unwrap();
    let cert = cert_file.path().to_str().unwrap();
    let key_path = Some(key_file.path().to_str().unwrap());
    for (passphrase, missing) in [(None, true), (Some("wrong"), false)] {
      let error = ClientIdentity::load(cert, key_path, passphrase).unwrap_err();
      match error.downcast_ref::<RequestIdentityError>().unwrap() {
        RequestIdentityError::NoPassphrase("PEM") => assert!(missing),
        RequestIdentityError::DecryptError("PEM") => assert!(!missing),
        other => panic!("Unexpected identity error: {other}"),
      }
    }
    let loaded = ClientIdentity::load(cert, key_path, Some("correct")).unwrap();
    assert_eq!(loaded.key_data().unwrap(), key);
    fs::write(key_file.path(), &key).unwrap();
    for passphrase in [None, Some("ignored for unencrypted key")] {
      assert_eq!(
        ClientIdentity::load(cert, key_path, passphrase)
          .unwrap()
          .key_data()
          .unwrap(),
        key
      );
    }
  }

  #[test]
  fn loaded_identity_survives_original_file_replacement_and_owns_private_files() {
    use std::os::unix::fs::PermissionsExt;
    let (certificate, key) = generated_identity();
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("combined.pem");
    let mut combined = certificate.clone();
    combined.extend(&key);
    fs::write(&source, &combined).unwrap();
    let identity = ClientIdentity::load(source.to_str().unwrap(), None, None).unwrap();
    fs::write(&source, "changed after authentication").unwrap();
    assert_eq!(identity.certificate_data(), certificate);
    assert_eq!(identity.key_data().unwrap(), key);
    assert!(!format!("{identity:?}").contains("PRIVATE KEY"));
    let files = identity.write_files(Some(directory.path())).unwrap();
    let certificate_path = files.certificate().to_owned();
    let key_path = files.key().unwrap().to_owned();
    assert_eq!(fs::read(&certificate_path).unwrap(), certificate);
    assert_eq!(fs::read(&key_path).unwrap(), key);
    assert_eq!(fs::metadata(&key_path).unwrap().permissions().mode() & 0o777, 0o600);
    drop(files);
    assert!(!Path::new(&certificate_path).exists());
    assert!(!Path::new(&key_path).exists());
    // The request identity remains valid after its original files disappear.
    fs::remove_file(&source).unwrap();
    reqwest::Client::builder()
      .identity(identity.request_identity())
      .build()
      .unwrap();
  }

  #[test]
  fn pkcs12_preserves_original_data_password_and_owned_native_files() {
    let (certificate, key) = generated_identity();
    let certificate = X509::from_pem(&certificate).unwrap();
    let key = PKey::private_key_from_pem(&key).unwrap();
    let mut builder = Pkcs12::builder();
    builder.name("identity-fixture").pkey(&key).cert(&certificate);
    let encoded = builder.build2("fixture-password").unwrap().to_der().unwrap();
    for suffix in [".p12", ".pfx"] {
      let mut source = tempfile::Builder::new().suffix(suffix).tempfile().unwrap();
      source.write_all(&encoded).unwrap();
      let path = source.path().to_str().unwrap();
      assert!(ClientIdentity::load(path, None, None).is_err());
      assert!(ClientIdentity::load(path, None, Some("wrong")).is_err());
      // PKCS#12 has always ignored a separately configured key path.
      let identity = ClientIdentity::load(path, Some("missing-key-file"), Some("fixture-password")).unwrap();
      fs::write(source.path(), b"replaced after login").unwrap();
      assert_eq!(identity.certificate_data(), encoded);
      assert!(identity.key_data().is_none());
      assert_eq!(identity.key_password(), Some("fixture-password"));
      assert!(!format!("{identity:?}").contains("fixture-password"));
      let files = identity.write_files(None).unwrap();
      assert!(files.certificate().ends_with(".p12"));
      assert!(files.key().is_none());
      ClientIdentity::load(files.certificate(), files.key(), identity.key_password()).unwrap();
      let owned_path = files.certificate().to_owned();
      drop(files);
      assert!(!Path::new(&owned_path).exists());
    }
    let empty_password = builder.build2("").unwrap().to_der().unwrap();
    // Empty-password support varies by native backend and PKCS#12 algorithms.
    // Match the previous native import instead of introducing another parser.
    let previously_accepted = Identity::from_pkcs12_der(&empty_password, "").is_ok();
    assert_eq!(
      ClientIdentity::from_data(Zeroizing::new(empty_password), None, Some("")).is_ok(),
      previously_accepted
    );
  }

  #[test]
  fn pkcs12_retains_chain_without_reordering_or_reencoding() {
    use openssl::{
      asn1::Asn1Time,
      hash::MessageDigest,
      rsa::Rsa,
      stack::Stack,
      x509::{X509NameBuilder, extension::BasicConstraints},
    };
    fn certificate(
      name: &str,
      parent: Option<(&X509, &PKey<openssl::pkey::Private>)>,
      ca: bool,
    ) -> (X509, PKey<openssl::pkey::Private>) {
      let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
      let mut subject = X509NameBuilder::new().unwrap();
      subject.append_entry_by_text("CN", name).unwrap();
      let subject = subject.build();
      let mut cert = X509::builder().unwrap();
      cert.set_version(2).unwrap();
      let serial = openssl::bn::BigNum::from_u32(name.len() as u32)
        .unwrap()
        .to_asn1_integer()
        .unwrap();
      cert.set_serial_number(&serial).unwrap();
      cert.set_subject_name(&subject).unwrap();
      cert
        .set_issuer_name(parent.map(|(cert, _)| cert.subject_name()).unwrap_or(&subject))
        .unwrap();
      cert.set_pubkey(&key).unwrap();
      cert.set_not_before(&Asn1Time::days_from_now(0).unwrap()).unwrap();
      cert.set_not_after(&Asn1Time::days_from_now(1).unwrap()).unwrap();
      if ca {
        cert
          .append_extension(BasicConstraints::new().critical().ca().build().unwrap())
          .unwrap();
      }
      cert
        .sign(parent.map(|(_, key)| key).unwrap_or(&key), MessageDigest::sha256())
        .unwrap();
      (cert.build(), key)
    }
    let (root, root_key) = certificate("root", None, true);
    let (intermediate, intermediate_key) = certificate("intermediate", Some((&root, &root_key)), true);
    let (leaf, key) = certificate("client", Some((&intermediate, &intermediate_key)), false);
    // This is the reverse CA stack consumed by the previous native-tls path.
    let mut ca = Stack::new().unwrap();
    ca.push(root.clone()).unwrap();
    ca.push(intermediate.clone()).unwrap();
    let mut builder = Pkcs12::builder();
    builder.name("chain-fixture").pkey(&key).cert(&leaf).ca(ca);
    let encoded = builder.build2("fixture-password").unwrap().to_der().unwrap();
    let mut source = tempfile::Builder::new().suffix(".p12").tempfile().unwrap();
    source.write_all(&encoded).unwrap();
    let identity = ClientIdentity::load(source.path().to_str().unwrap(), None, Some("fixture-password")).unwrap();
    assert_eq!(identity.certificate_data(), encoded);
    assert!(identity.key_data().is_none());
    ClientIdentity::from_data(
      Zeroizing::new(identity.certificate_data().to_vec()),
      None,
      identity.key_password(),
    )
    .unwrap();
  }

  #[test]
  fn create_identity_from_pem_requires_passphrase() {
    let cert = "tests/files/badssl.com-client.pem";
    let identity = create_identity(cert, None, None);

    assert!(identity.is_err());
    assert!(identity.unwrap_err().to_string().contains("No passphrase provided"));
  }

  #[test]
  #[cfg(not(target_os = "macos"))]
  fn create_identity_from_pem_with_passphrase() {
    let cert = "tests/files/badssl.com-client.pem";
    let passphrase = "badssl.com";

    let identity = create_identity(cert, None, Some(passphrase));

    assert!(identity.is_ok());
  }

  #[test]
  #[cfg(not(target_os = "macos"))]
  fn create_identity_from_pem_unencrypted_key() {
    let cert = "tests/files/badssl.com-client-unencrypted.pem";
    let identity = create_identity(cert, None, None);
    println!("{:?}", identity);

    assert!(identity.is_ok());
  }

  #[test]
  #[cfg(not(target_os = "macos"))]
  fn create_identity_from_pem_cert_and_encrypted_key() {
    let cert = "tests/files/badssl.com-client.pem";
    let key = "tests/files/badssl.com-client.pem";
    let passphrase = "badssl.com";

    let identity = create_identity(cert, Some(key), Some(passphrase));

    assert!(identity.is_ok());
  }

  #[test]
  fn create_identity_from_pem_cert_and_encrypted_key_no_passphrase() {
    let cert = "tests/files/badssl.com-client.pem";
    let key = "tests/files/badssl.com-client.pem";

    let identity = create_identity(cert, Some(key), None);

    assert!(identity.is_err());
    assert!(identity.unwrap_err().to_string().contains("No passphrase provided"));
  }

  #[test]
  #[cfg(not(target_os = "macos"))]
  fn create_identity_from_pem_cert_and_unencrypted_key() {
    let cert = "tests/files/badssl.com-client.pem";
    let key = "tests/files/badssl.com-client-unencrypted.pem";

    let identity = create_identity(cert, Some(key), None);

    assert!(identity.is_ok());
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
