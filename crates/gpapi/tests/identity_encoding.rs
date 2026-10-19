//! Container compatibility through the public identity loader, using public fixtures.
use std::{
  fs,
  path::{Path, PathBuf},
};

use gpapi::utils::request::{ClientIdentity, RequestIdentityError};

const PASSWORD: &str = "fixture-password";

fn fixture(name: &str) -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("tests/files/client-identity")
    .join(name)
}

fn bytes(name: &str) -> Vec<u8> {
  fs::read(fixture(name)).unwrap()
}

fn load(certificate: &Path, key: &Path, password: Option<&str>) -> anyhow::Result<ClientIdentity> {
  ClientIdentity::load(certificate.to_str().unwrap(), Some(key.to_str().unwrap()), password)
}

#[test]
fn der_certificates_and_rsa_keys_normalize_to_the_existing_pem_identity() {
  let directory = tempfile::tempdir().unwrap();
  let certificate = directory.path().join("client.cer");
  let key = directory.path().join("client.key");
  fs::write(&certificate, pem::parse(bytes("rsa-cert.pem")).unwrap().contents()).unwrap();
  let missing = ClientIdentity::load(certificate.to_str().unwrap(), None, None).unwrap_err();
  assert!(matches!(
    missing.downcast_ref::<RequestIdentityError>(),
    Some(RequestIdentityError::NoKey)
  ));
  for name in ["rsa-pkcs8.pem", "rsa-pkcs1.pem", "rsa-pbes2-sha256.pem"] {
    fs::write(&key, pem::parse(bytes(name)).unwrap().contents()).unwrap();
    let identity = load(&certificate, &key, Some(PASSWORD)).unwrap_or_else(|error| panic!("{name}: {error}"));
    assert_eq!(identity.certificate_data(), bytes("rsa-cert.pem"));
    assert_eq!(identity.key_data().unwrap(), bytes("rsa-pkcs8.pem"));
    assert!(identity.key_password().is_none());
    let snapshot = ClientIdentity::from_data(
      zeroize::Zeroizing::new(identity.certificate_data().to_vec()),
      Some(zeroize::Zeroizing::new(identity.key_data().unwrap().to_vec())),
      None,
    )
    .unwrap();
    assert_eq!(snapshot.key_data(), identity.key_data());
  }
}

#[test]
fn encrypted_der_preserves_password_errors() {
  let directory = tempfile::tempdir().unwrap();
  let key = directory.path().join("key.der");
  fs::write(&key, pem::parse(bytes("rsa-pbes2-sha256.pem")).unwrap().contents()).unwrap();
  for (password, missing) in [(None, true), (Some("incorrect"), false)] {
    let error = load(&fixture("rsa-cert.pem"), &key, password).unwrap_err();
    match error.downcast_ref::<RequestIdentityError>() {
      Some(RequestIdentityError::NoPassphrase("PEM")) => assert!(missing),
      Some(RequestIdentityError::DecryptError("PEM")) => assert!(!missing),
      other => panic!("unexpected DER password error: {other:?}"),
    }
  }
}

#[test]
fn ec_sec1_normalizes_on_linux_and_bsd_and_is_terminal_on_macos() {
  let directory = tempfile::tempdir().unwrap();
  let key = directory.path().join("ec.der");
  fs::write(&key, pem::parse(bytes("ec-sec1.pem")).unwrap().contents()).unwrap();
  for source in [fixture("ec-sec1.pem"), key] {
    for password in [None, Some(PASSWORD)] {
      let result = load(&fixture("ec-cert.pem"), &source, password);
      if cfg!(target_os = "macos") {
        assert!(matches!(
          result.unwrap_err().downcast_ref::<RequestIdentityError>(),
          Some(RequestIdentityError::UnsupportedKeyFormat)
        ));
      } else {
        assert_eq!(result.unwrap().key_data().unwrap(), bytes("ec-pkcs8.pem"));
      }
    }
  }
}

#[test]
fn traditional_encrypted_pem_has_platform_specific_support_and_password_errors() {
  for algorithm in ["rsa", "ec"] {
    for cipher in ["aes128", "aes192", "aes256", "des3"] {
      let certificate = fixture(&format!("{algorithm}-cert.pem"));
      let key = fixture(&format!("{algorithm}-traditional-{cipher}.pem"));
      for password in [None, Some("incorrect"), Some(PASSWORD)] {
        let result = load(&certificate, &key, password);
        if cfg!(target_os = "macos") {
          assert!(matches!(
            result.unwrap_err().downcast_ref::<RequestIdentityError>(),
            Some(RequestIdentityError::UnsupportedEncryption)
          ));
        } else if password == Some(PASSWORD) {
          let identity = result.unwrap_or_else(|error| panic!("{key:?}: {error}"));
          assert_eq!(identity.key_data().unwrap(), bytes(&format!("{algorithm}-pkcs8.pem")));
          assert!(identity.key_password().is_none());
        } else {
          let error = result.unwrap_err();
          match error.downcast_ref::<RequestIdentityError>() {
            Some(RequestIdentityError::NoPassphrase("PEM")) => assert!(password.is_none()),
            Some(RequestIdentityError::DecryptError("PEM")) => assert!(password.is_some()),
            other => panic!("{key:?}: unexpected password error {other:?}"),
          }
        }
      }
    }
  }
}

#[test]
#[cfg(not(target_os = "macos"))]
fn traditional_passphrase_with_nul_returns_a_redacted_terminal_error() {
  let error = load(
    &fixture("rsa-cert.pem"),
    &fixture("rsa-traditional-aes256.pem"),
    Some("secret-password\0do-not-log"),
  )
  .unwrap_err();
  assert!(!matches!(
    error.downcast_ref::<RequestIdentityError>(),
    Some(RequestIdentityError::NoPassphrase(_) | RequestIdentityError::DecryptError(_))
  ));
  for diagnostic in [error.to_string(), format!("{error:?}")] {
    assert!(!diagnostic.contains("secret-password"));
    assert!(!diagnostic.contains("do-not-log"));
    assert!(!diagnostic.contains("115, 101, 99, 114, 101, 116"));
  }
}

#[test]
fn first_traditional_encrypted_key_controls_combined_pem_loading() {
  let directory = tempfile::tempdir().unwrap();
  let source = directory.path().join("combined.pem");
  let mut chain = bytes("rsa-cert.pem");
  chain.extend(bytes("root.pem"));
  let mut combined = chain.clone();
  combined.extend(bytes("rsa-traditional-aes256.pem"));
  combined.extend(bytes("rsa-pkcs8.pem"));
  fs::write(&source, &combined).unwrap();
  for password in [None, Some(PASSWORD)] {
    let result = ClientIdentity::load(source.to_str().unwrap(), None, password);
    if cfg!(target_os = "macos") {
      assert!(matches!(
        result.unwrap_err().downcast_ref::<RequestIdentityError>(),
        Some(RequestIdentityError::UnsupportedEncryption)
      ));
    } else if password.is_none() {
      assert!(matches!(
        result.unwrap_err().downcast_ref::<RequestIdentityError>(),
        Some(RequestIdentityError::NoPassphrase("PEM"))
      ));
    } else {
      let identity = result.unwrap();
      assert_eq!(identity.certificate_data(), chain);
      assert_eq!(identity.key_data().unwrap(), bytes("rsa-pkcs8.pem"));
    }
  }
  assert_eq!(fs::read(source).unwrap(), combined);
}

#[test]
fn malformed_traditional_headers_are_terminal_and_do_not_expose_material() {
  let directory = tempfile::tempdir().unwrap();
  let key = directory.path().join("invalid.pem");
  for (headers, unsupported) in [
    (vec![("Proc-Type", "4,ENCRYPTED")], false),
    (
      vec![("DEK-Info", "AES-256-CBC,00000000000000000000000000000000")],
      false,
    ),
    (
      vec![
        ("Proc-Type", "4,ENCRYPTED"),
        ("DEK-Info", "AES-256-CBC,invalid-secret-iv"),
      ],
      false,
    ),
    (
      vec![("Proc-Type", "4,ENCRYPTED"), ("DEK-Info", "AES-256-CBC,00")],
      false,
    ),
    (
      vec![
        ("Proc-Type", "4,ENCRYPTED"),
        ("Proc-Type", "4,ENCRYPTED"),
        ("DEK-Info", "AES-256-CBC,00000000000000000000000000000000"),
      ],
      false,
    ),
    (
      vec![
        ("Proc-Type", "4,ENCRYPTED"),
        ("DEK-Info", "UNKNOWN-CIPHER,00000000000000000000000000000000"),
      ],
      true,
    ),
  ] {
    let mut block = pem::Pem::new("RSA PRIVATE KEY", b"secret-invalid-key-material");
    for (name, value) in headers {
      block.headers_mut().add(name, value).unwrap();
    }
    fs::write(&key, pem::encode(&block)).unwrap();
    for password in [None, Some("secret-passphrase")] {
      let error = load(&fixture("rsa-cert.pem"), &key, password).unwrap_err();
      match error.downcast_ref::<RequestIdentityError>() {
        Some(RequestIdentityError::UnsupportedEncryption) => assert!(unsupported),
        Some(RequestIdentityError::InvalidKey) => assert!(!unsupported),
        other => panic!("unexpected traditional header error: {other:?}"),
      }
      let diagnostic = format!("{error:?}");
      for secret in ["secret-invalid-key-material", "secret-passphrase", "invalid-secret-iv"] {
        assert!(!diagnostic.contains(secret));
      }
    }
  }
}

#[test]
fn der_with_trailing_data_is_rejected_for_certificates_and_keys() {
  let directory = tempfile::tempdir().unwrap();
  let source = directory.path().join("trailing.der");
  for name in [
    "rsa-cert.pem",
    "rsa-pkcs1.pem",
    "rsa-pkcs8.pem",
    "rsa-pbes2-sha256.pem",
    "ec-sec1.pem",
  ] {
    let mut der = pem::parse(bytes(name)).unwrap().into_contents();
    der.push(0);
    fs::write(&source, der).unwrap();
    let result = if name == "rsa-cert.pem" {
      load(&source, &fixture("rsa-pkcs8.pem"), Some(PASSWORD))
    } else {
      load(&fixture("rsa-cert.pem"), &source, Some(PASSWORD))
    };
    let error = result.unwrap_err();
    assert!(
      matches!(
        error.downcast_ref::<RequestIdentityError>(),
        Some(RequestIdentityError::InvalidKey | RequestIdentityError::InvalidCertificate)
      ),
      "{name}: {error}"
    );
  }
}

#[test]
fn pkcs12_extensions_are_ascii_case_insensitive() {
  let directory = tempfile::tempdir().unwrap();
  let archive = bytes("modern.p12");
  for extension in ["P12", "p12", "PFX", "pFx"] {
    let path = directory.path().join(format!("identity.{extension}"));
    fs::write(&path, &archive).unwrap();
    let identity = ClientIdentity::load(path.to_str().unwrap(), None, Some(PASSWORD)).unwrap();
    assert_eq!(identity.certificate_data(), archive);
    assert!(identity.key_data().is_none());
    for password in [None, Some("incorrect")] {
      let error = ClientIdentity::load(path.to_str().unwrap(), None, password).unwrap_err();
      match error.downcast_ref::<RequestIdentityError>() {
        Some(RequestIdentityError::NoPassphrase("PKCS#12")) => assert!(password.is_none()),
        Some(RequestIdentityError::InvalidPkcs12(_)) => assert!(password.is_some()),
        other => panic!("unexpected archive error: {other:?}"),
      }
    }
  }
}
