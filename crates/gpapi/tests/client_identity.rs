//! Public identity compatibility and intended parser migration behavior.
use std::{fs, path::PathBuf};

use gpapi::utils::request::{ClientIdentity, RequestIdentityError};

const PASSWORD: &str = "fixture-password";

fn fixture(name: &str) -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("tests/files/client-identity")
    .join(name)
}

fn load(certificate: &str, key: &str, password: Option<&str>) -> anyhow::Result<ClientIdentity> {
  ClientIdentity::load(
    fixture(certificate).to_str().unwrap(),
    Some(fixture(key).to_str().unwrap()),
    password,
  )
}

#[test]
fn rsa_pkcs8_loads_without_a_password() {
  let identity = load("rsa-cert.pem", "rsa-pkcs8.pem", None).unwrap();
  assert_eq!(identity.certificate_data(), fs::read(fixture("rsa-cert.pem")).unwrap());
  assert_eq!(
    identity.key_data().unwrap(),
    fs::read(fixture("rsa-pkcs8.pem")).unwrap()
  );
  assert!(identity.key_password().is_none());
}

#[test]
fn ec_pkcs8_records_native_import_compatibility() {
  let result = load("ec-cert.pem", "ec-pkcs8.pem", None);
  if cfg!(target_os = "macos") {
    // Apple native-tls import currently rejects this P-256 PKCS#8 fixture.
    assert!(
      result
        .unwrap_err()
        .downcast_ref::<reqwest::Error>()
        .unwrap()
        .is_builder()
    );
  } else {
    assert_eq!(
      result.unwrap().key_data().unwrap(),
      fs::read(fixture("ec-pkcs8.pem")).unwrap()
    );
  }
}

#[test]
fn encrypted_pkcs8_keys_normalize_to_the_same_private_key() {
  for (key, expected, certificate) in [
    ("rsa-pbes2-sha1.pem", "rsa-pkcs8.pem", "rsa-cert.pem"),
    ("rsa-pbes2-sha256.pem", "rsa-pkcs8.pem", "rsa-cert.pem"),
    ("rsa-pbes2-aes-128-cbc-sha1.pem", "rsa-pkcs8.pem", "rsa-cert.pem"),
    ("rsa-pbes2-aes-128-cbc-sha256.pem", "rsa-pkcs8.pem", "rsa-cert.pem"),
    ("rsa-pbes2-aes-192-cbc-sha1.pem", "rsa-pkcs8.pem", "rsa-cert.pem"),
    ("rsa-pbes2-aes-192-cbc-sha256.pem", "rsa-pkcs8.pem", "rsa-cert.pem"),
    ("rsa-pbes2-scrypt.pem", "rsa-pkcs8.pem", "rsa-cert.pem"),
    ("ec-pbes2.pem", "ec-pkcs8.pem", "ec-cert.pem"),
  ] {
    let result = load(certificate, key, Some(PASSWORD));
    if cfg!(target_os = "macos") && certificate == "ec-cert.pem" {
      assert!(
        result
          .unwrap_err()
          .downcast_ref::<reqwest::Error>()
          .unwrap()
          .is_builder()
      );
    } else {
      let identity = result.unwrap_or_else(|error| panic!("{key}: {error}"));
      assert_eq!(
        identity.key_data().unwrap(),
        fs::read(fixture(expected)).unwrap(),
        "{key}"
      );
      assert!(identity.key_password().is_none());
    }
    for (password, missing) in [(None, true), (Some("incorrect"), false)] {
      let error = load(certificate, key, password).unwrap_err();
      match error.downcast_ref::<RequestIdentityError>() {
        Some(RequestIdentityError::NoPassphrase("PEM")) => assert!(missing),
        Some(RequestIdentityError::DecryptError("PEM")) => assert!(!missing),
        other => panic!("{key}: unexpected error {other:?}"),
      }
    }
  }
}

#[test]
fn combined_pem_preserves_the_certificate_chain_and_removes_key_blocks() {
  let mut chain = fs::read(fixture("rsa-cert.pem")).unwrap();
  chain.extend(fs::read(fixture("root.pem")).unwrap());
  let mut combined = chain.clone();
  let key = fs::read(fixture("rsa-pkcs8.pem")).unwrap();
  combined.extend(&key);
  let directory = tempfile::tempdir().unwrap();
  let source = directory.path().join("combined.pem");
  fs::write(&source, combined).unwrap();
  let identity = ClientIdentity::load(source.to_str().unwrap(), None, None).unwrap();
  assert_eq!(identity.certificate_data(), chain);
  assert_eq!(identity.key_data().unwrap(), key);
}

#[test]
fn traditional_unencrypted_keys_are_rejected_before_normalization() {
  for (certificate, key) in [("rsa-cert.pem", "rsa-pkcs1.pem"), ("ec-cert.pem", "ec-sec1.pem")] {
    // Container support is deliberately unchanged; the parser may reject it
    // before the native backend, so do not couple this to a reqwest error.
    assert!(load(certificate, key, None).is_err());
  }
}

fn check_archive(name: &str, password: &str) {
  let source = fixture(name);
  let path = source.to_str().unwrap();
  let identity = ClientIdentity::load(path, None, Some(password)).unwrap();
  assert_eq!(identity.certificate_data(), fs::read(&source).unwrap());
  assert!(identity.key_data().is_none());
  assert_eq!(identity.key_password(), Some(password));
  let files = identity.write_files(None).unwrap();
  ClientIdentity::load(files.certificate(), files.key(), identity.key_password()).unwrap();
  assert!(ClientIdentity::load(path, None, Some("incorrect")).is_err());
  let error = ClientIdentity::load(path, None, None).unwrap_err();
  assert!(matches!(
    error.downcast_ref::<RequestIdentityError>(),
    Some(RequestIdentityError::NoPassphrase("PKCS#12"))
  ));
  // The MAC is at the end of these fixtures. Change authenticated archive data
  // (a byte in the friendly name), leaving valid ASN.1 and ciphertext intact.
  let mut corrupted = fs::read(&source).unwrap();
  let label = [0, b'f', 0, b'i', 0, b'x', 0, b't', 0, b'u', 0, b'r', 0, b'e'];
  let offset = corrupted
    .windows(label.len())
    .position(|window| window == label)
    .unwrap();
  corrupted[offset + 1] = b'F';
  let directory = tempfile::tempdir().unwrap();
  let damaged = directory.path().join("damaged.p12");
  fs::write(&damaged, corrupted).unwrap();
  assert!(ClientIdentity::load(damaged.to_str().unwrap(), None, Some(password)).is_err());
}

#[test]
fn modern_pkcs12_checks_password_and_archive_integrity() {
  check_archive("modern.p12", PASSWORD);
}

#[test]
fn pkcs12_accepts_non_ascii_passwords() {
  check_archive("unicode-password.p12", "pässword-密码");
}

#[test]
fn legacy_rc2_pkcs12_checks_password_and_archive_integrity() {
  #[cfg(not(target_os = "macos"))]
  if std::env::var_os("GP_IDENTITY_LEGACY_TEST_CHILD").is_none() {
    // OpenSSL providers are process-global. Exercise the existing legacy-mode
    // configuration in an isolated process, without affecting parallel tests.
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("openssl.cnf");
    fs::write(&config, gpapi::utils::openssl::openssl_conf()).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
      .args([
        "--exact",
        "legacy_rc2_pkcs12_checks_password_and_archive_integrity",
        "--nocapture",
      ])
      .env("OPENSSL_CONF", config)
      .env("GP_IDENTITY_LEGACY_TEST_CHILD", "1")
      .status()
      .unwrap();
    assert!(status.success());
    return;
  }
  check_archive("legacy-rc2.p12", PASSWORD);
}

#[test]
fn invalid_certificate_and_private_key_data_are_rejected() {
  let directory = tempfile::tempdir().unwrap();
  let invalid = directory.path().join("invalid.pem");
  fs::write(&invalid, b"not a certificate or private key").unwrap();
  assert!(
    ClientIdentity::load(
      invalid.to_str().unwrap(),
      Some(fixture("rsa-pkcs8.pem").to_str().unwrap()),
      None,
    )
    .is_err()
  );
  let error = ClientIdentity::load(
    fixture("rsa-cert.pem").to_str().unwrap(),
    Some(invalid.to_str().unwrap()),
    None,
  )
  .unwrap_err();
  assert!(matches!(
    error.downcast_ref::<RequestIdentityError>(),
    Some(RequestIdentityError::NoKey)
  ));
}

#[test]
fn pbes2_legacy_ciphers_remain_supported() {
  for key in ["rsa-pbes2-des3-sha1.pem", "rsa-pbes2-des3-sha256.pem"] {
    let identity = load("rsa-cert.pem", key, Some(PASSWORD)).unwrap();
    assert_eq!(
      identity.key_data().unwrap(),
      fs::read(fixture("rsa-pkcs8.pem")).unwrap()
    );
    for (password, missing) in [(None, true), (Some("incorrect"), false)] {
      let error = load("rsa-cert.pem", key, password).unwrap_err();
      match error.downcast_ref::<RequestIdentityError>() {
        Some(RequestIdentityError::NoPassphrase("PEM")) => assert!(missing),
        Some(RequestIdentityError::DecryptError("PEM")) => assert!(!missing),
        other => panic!("{key}: unexpected error {other:?}"),
      }
    }
  }
}

fn assert_unsupported_legacy_pkcs8(password: Option<&str>) {
  // Deliberate compatibility change: this standalone PKCS#12 PBE scheme
  // differs from PBES2-3DES and from the supported legacy PKCS#12 archives.
  let error = load("rsa-cert.pem", "rsa-pbes1-3des.pem", password)
    .expect_err("legacy standalone encryption should require one-time conversion");
  assert!(matches!(
    error.downcast_ref::<RequestIdentityError>(),
    Some(RequestIdentityError::UnsupportedEncryption)
  ));
  let message = error.to_string().to_lowercase();
  assert!(
    message.contains("unsupported") && message.contains("encryption"),
    "{message}"
  );
  assert!(message.contains("pkcs#8") && message.contains("convert"), "{message}");
}

#[test]
fn unsupported_legacy_pkcs8_without_password_does_not_request_one() {
  assert_unsupported_legacy_pkcs8(None);
}

#[test]
fn unsupported_legacy_pkcs8_with_correct_password_requires_conversion() {
  assert_unsupported_legacy_pkcs8(Some(PASSWORD));
}

#[test]
fn unsupported_legacy_pkcs8_with_wrong_password_does_not_retry_password() {
  assert_unsupported_legacy_pkcs8(Some("incorrect"));
}

#[test]
fn first_key_selection_controls_password_prompt_and_ignores_later_keys() {
  let directory = tempfile::tempdir().unwrap();
  let source = directory.path().join("combined.pem");
  let certificate = fs::read(fixture("rsa-cert.pem")).unwrap();
  let plain = fs::read(fixture("rsa-pkcs8.pem")).unwrap();
  let encrypted = fs::read(fixture("rsa-pbes2-sha256.pem")).unwrap();
  for (first, second, needs_password) in [(&plain, &encrypted, false), (&encrypted, &plain, true)] {
    let mut combined = certificate.clone();
    combined.extend(first);
    combined.extend(second);
    fs::write(&source, combined).unwrap();
    if needs_password {
      let error = ClientIdentity::load(source.to_str().unwrap(), None, None).unwrap_err();
      assert!(matches!(
        error.downcast_ref::<RequestIdentityError>(),
        Some(RequestIdentityError::NoPassphrase("PEM"))
      ));
    }
    let identity = ClientIdentity::load(
      source.to_str().unwrap(),
      None,
      if needs_password { Some(PASSWORD) } else { None },
    )
    .unwrap();
    assert_eq!(identity.key_data().unwrap(), plain);
  }
}

#[test]
fn prepared_pem_round_trip_retains_snapshot_and_private_file_lifecycle() {
  use std::os::unix::fs::PermissionsExt;
  use zeroize::Zeroizing;
  let directory = tempfile::tempdir().unwrap();
  let source = directory.path().join("identity.pem");
  let mut combined = fs::read(fixture("rsa-cert.pem")).unwrap();
  combined.extend(fs::read(fixture("rsa-pkcs8.pem")).unwrap());
  fs::write(&source, combined).unwrap();
  let loaded = ClientIdentity::load(source.to_str().unwrap(), None, None).unwrap();
  let snapshot = ClientIdentity::from_data(
    Zeroizing::new(loaded.certificate_data().to_vec()),
    Some(Zeroizing::new(loaded.key_data().unwrap().to_vec())),
    Some("unused-pem-password"),
  )
  .unwrap();
  fs::write(&source, b"replaced after authentication").unwrap();
  drop(loaded);
  let clone = snapshot.clone();
  drop(snapshot);
  assert!(clone.key_password().is_none());
  let debug = format!("{clone:?}");
  assert!(debug.contains("<redacted>"));
  assert!(!debug.contains("PRIVATE KEY"));
  assert!(!debug.contains("unused-pem-password"));
  let files = clone.write_files(Some(directory.path())).unwrap();
  let certificate = PathBuf::from(files.certificate());
  let key = PathBuf::from(files.key().unwrap());
  for path in [&certificate, &key] {
    assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
  }
  assert_eq!(fs::read(&certificate).unwrap(), clone.certificate_data());
  assert_eq!(fs::read(&key).unwrap(), clone.key_data().unwrap());
  ClientIdentity::load(files.certificate(), files.key(), None).unwrap();
  drop(files);
  assert!(!certificate.exists() && !key.exists());
}

#[test]
fn archive_snapshot_and_pfx_preserve_native_import_contract() {
  use zeroize::Zeroizing;
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("identity.pfx");
  let data = fs::read(fixture("modern.p12")).unwrap();
  fs::write(&path, &data).unwrap();
  let identity = ClientIdentity::load(path.to_str().unwrap(), Some("nonexistent-key"), Some(PASSWORD)).unwrap();
  fs::remove_file(path).unwrap();
  let restored = ClientIdentity::from_data(
    Zeroizing::new(identity.certificate_data().to_vec()),
    None,
    identity.key_password(),
  )
  .unwrap();
  assert_eq!(restored.certificate_data(), data);
  assert_eq!(restored.key_password(), Some(PASSWORD));
  assert!(restored.key_data().is_none());
  assert!(!format!("{restored:?}").contains(PASSWORD));
  let empty = fs::read(fixture("empty-password.p12")).unwrap();
  // Preserve the native backend's existing empty-password decision.
  let expected = reqwest::Identity::from_pkcs12_der(&empty, "").is_ok();
  assert_eq!(
    ClientIdentity::from_data(Zeroizing::new(empty), None, Some("")).is_ok(),
    expected
  );
}

#[test]
fn malformed_certificate_der_and_private_key_der_are_rejected_without_secret_output() {
  let directory = tempfile::tempdir().unwrap();
  let invalid = directory.path().join("invalid.pem");
  for (tag, invalid_certificate) in [("CERTIFICATE", true), ("PRIVATE KEY", false)] {
    let invalid_der = b"secret-invalid-der-material";
    fs::write(&invalid, pem::encode(&pem::Pem::new(tag, invalid_der))).unwrap();
    let result = if invalid_certificate {
      ClientIdentity::load(
        invalid.to_str().unwrap(),
        Some(fixture("rsa-pkcs8.pem").to_str().unwrap()),
        None,
      )
    } else {
      ClientIdentity::load(
        fixture("rsa-cert.pem").to_str().unwrap(),
        Some(invalid.to_str().unwrap()),
        None,
      )
    };
    let error = result.unwrap_err();
    for message in [error.to_string(), format!("{error:?}")] {
      assert!(!message.contains("secret-invalid-der-material"));
      assert!(!message.contains("c2VjcmV0LWludmFsaWQtZGVyLW1hdGVyaWFs"));
    }
  }
}

#[test]
fn pbes2_des_remains_supported_without_native_legacy_provider() {
  let identity = load("rsa-cert.pem", "rsa-pbes2-des.pem", Some(PASSWORD)).unwrap();
  assert_eq!(
    identity.key_data().unwrap(),
    fs::read(fixture("rsa-pkcs8.pem")).unwrap()
  );
  for (password, missing) in [(None, true), (Some("incorrect"), false)] {
    let error = load("rsa-cert.pem", "rsa-pbes2-des.pem", password).unwrap_err();
    match error.downcast_ref::<RequestIdentityError>() {
      Some(RequestIdentityError::NoPassphrase("PEM")) => assert!(missing),
      Some(RequestIdentityError::DecryptError("PEM")) => assert!(!missing),
      other => panic!("unexpected error {other:?}"),
    }
  }
}

fn assert_unsupported_pbes2_algorithm(oid_der: &[u8]) {
  let mut encrypted = pem::parse(fs::read(fixture("rsa-pbes2-sha256.pem")).unwrap()).unwrap();
  let mut der = encrypted.contents().to_vec();
  let matches: Vec<_> = der
    .windows(oid_der.len())
    .enumerate()
    .filter_map(|(offset, window)| (window == oid_der).then_some(offset))
    .collect();
  assert_eq!(
    matches.len(),
    1,
    "the fixture must contain exactly the intended algorithm OID"
  );
  // An unknown OID in the same arc preserves ASN.1 structure and algorithm
  // parameters, isolating algorithm recognition from malformed DER handling.
  der[matches[0] + oid_der.len() - 1] = 0x7f;
  encrypted = pem::Pem::new(encrypted.tag(), der);
  let directory = tempfile::tempdir().unwrap();
  let source = directory.path().join("unsupported.pem");
  fs::write(&source, pem::encode(&encrypted)).unwrap();
  for password in [None, Some(PASSWORD), Some("incorrect")] {
    let error = ClientIdentity::load(
      fixture("rsa-cert.pem").to_str().unwrap(),
      Some(source.to_str().unwrap()),
      password,
    )
    .unwrap_err();
    assert!(matches!(
      error.downcast_ref::<RequestIdentityError>(),
      Some(RequestIdentityError::UnsupportedEncryption)
    ));
    let message = error.to_string().to_lowercase();
    assert!(
      message.contains("unsupported") && message.contains("encryption"),
      "{message}"
    );
  }
}

#[test]
fn unsupported_pbes2_kdf_is_identified_before_password_prompt() {
  // PBKDF2, 1.2.840.113549.1.5.12.
  assert_unsupported_pbes2_algorithm(&[0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x05, 0x0c]);
}

#[test]
fn unsupported_pbes2_cipher_is_identified_before_password_prompt() {
  // AES256-CBC, 2.16.840.1.101.3.4.1.42.
  assert_unsupported_pbes2_algorithm(&[0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x01, 0x2a]);
}

#[test]
fn unsupported_pbes2_prf_is_identified_before_password_prompt() {
  // HMAC-SHA256, 1.2.840.113549.2.9.
  assert_unsupported_pbes2_algorithm(&[0x06, 0x08, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x02, 0x09]);
}

#[test]
fn malformed_encrypted_pkcs8_does_not_request_a_password_or_expose_material() {
  let directory = tempfile::tempdir().unwrap();
  let source = directory.path().join("malformed.pem");
  fs::write(
    &source,
    pem::encode(&pem::Pem::new(
      "ENCRYPTED PRIVATE KEY",
      b"secret-malformed-encrypted-key",
    )),
  )
  .unwrap();
  for password in [None, Some(PASSWORD), Some("incorrect")] {
    let error = ClientIdentity::load(
      fixture("rsa-cert.pem").to_str().unwrap(),
      Some(source.to_str().unwrap()),
      password,
    )
    .unwrap_err();
    assert!(!matches!(
      error.downcast_ref::<RequestIdentityError>(),
      Some(RequestIdentityError::NoPassphrase(_) | RequestIdentityError::DecryptError(_))
    ));
    for message in [error.to_string(), format!("{error:?}")] {
      assert!(!message.contains("secret-malformed-encrypted-key"));
      assert!(!message.contains("c2VjcmV0LW1hbGZvcm1lZC1lbmNyeXB0ZWQta2V5"));
    }
  }
}

#[test]
fn mismatched_certificate_key_preserves_native_backend_import_decision() {
  let certificate = fs::read(fixture("root.pem")).unwrap();
  let key = fs::read(fixture("rsa-pkcs8.pem")).unwrap();
  // Preserve the native adapter contract rather than imposing a new matching
  // policy: backend import and usable TLS client identity are separate checks.
  let native = reqwest::Identity::from_pkcs8_pem(&certificate, &key);
  let result = load("root.pem", "rsa-pkcs8.pem", None);
  assert_eq!(result.is_ok(), native.is_ok());
  if let Ok(identity) = result {
    assert_eq!(identity.certificate_data(), certificate);
    assert_eq!(identity.key_data().unwrap(), key);
  }
}
