//! Supported desktop identities and actionable failures for unsupported inputs.
#![cfg(any(
  target_os = "macos",
  target_os = "linux",
  target_os = "freebsd",
  target_os = "openbsd"
))]
use gpapi::utils::request::{ClientIdentity, RequestIdentityError};
use openssl::{
  asn1::Asn1Time,
  bn::BigNum,
  hash::MessageDigest,
  pkey::PKey,
  stack::Stack,
  x509::{X509, X509NameBuilder},
};
use std::{fs, path::PathBuf};
use zeroize::Zeroizing;

fn fixture(name: &str) -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("tests/files/client-identity")
    .join(name)
}

fn assert_unsupported_key(certificate: &str, key: &str) {
  let error = ClientIdentity::load(
    fixture(certificate).to_str().unwrap(),
    Some(fixture(key).to_str().unwrap()),
    None,
  )
  .unwrap_err();
  assert!(
    matches!(
      error.downcast_ref::<RequestIdentityError>(),
      Some(RequestIdentityError::UnsupportedKeyAlgorithm(_))
    ),
    "{error:#}"
  );
  let message = error.to_string();
  assert!(message.contains("unsupported"), "{message}");
  assert!(message.contains("VPN administrator"), "{message}");
  assert!(
    !message.contains("password") && !message.contains("does not match"),
    "{message}"
  );
}

#[test]
fn compressed_ec_certificate_reports_an_unsupported_encoding() {
  assert_unsupported_key("ec-compressed-cert.pem", "ec-pkcs8.pem");
}

#[test]
fn compressed_ec_private_key_reports_an_unsupported_encoding() {
  assert_unsupported_key("ec-compressed-cert.pem", "ec-compressed.pem");
}

#[test]
fn explicit_ec_parameters_report_an_unsupported_encoding() {
  assert_unsupported_key("ec-explicit-cert.pem", "ec-explicit.pem");
}

#[test]
fn p521_key_reports_the_known_tls_capability_limit() {
  assert_unsupported_key("ec-p521-cert.pem", "ec-p521.pem");
}

#[test]
fn rsa_pss_key_reports_the_known_tls_capability_limit() {
  assert_unsupported_key("rsa-pss-cert.pem", "rsa-pss.pem");
}

fn legacy_archive(name: &str) {
  let path = fixture(name);
  let bytes = fs::read(&path).unwrap();
  let identity = ClientIdentity::load(path.to_str().unwrap(), None, Some("fixture-password"))
    .unwrap_or_else(|error| panic!("{name}: {error:#}"));
  assert_eq!(identity.certificate_data(), bytes);
  assert!(ClientIdentity::load(path.to_str().unwrap(), None, Some("incorrect")).is_err());
}

fn unsupported_archive(name: &str) {
  let path = fixture(name);
  let bytes = fs::read(&path).unwrap();
  for password in [None, Some("fixture-password"), Some("incorrect"), Some("")] {
    for error in [
      ClientIdentity::load(path.to_str().unwrap(), None, password).unwrap_err(),
      ClientIdentity::from_data(Zeroizing::new(bytes.clone()), None, password).unwrap_err(),
    ] {
      assert!(
        matches!(
          error.downcast_ref::<RequestIdentityError>(),
          Some(RequestIdentityError::UnsupportedEncryption)
        ),
        "{name}: {error:#}"
      );
      assert!(error.to_string().contains("AES"), "{error}");
    }
  }
}

#[test]
fn legacy_rc2_128_pkcs12_explains_the_unsupported_encryption() {
  unsupported_archive("legacy-RC2-128.p12");
}
#[test]
fn legacy_rc4_128_pkcs12_is_usable() {
  legacy_archive("legacy-RC4-128.p12");
}
#[test]
fn legacy_two_key_3des_pkcs12_explains_the_unsupported_encryption() {
  unsupported_archive("legacy-2DES.p12");
}

#[test]
fn renewed_pkcs12_with_one_key_is_accepted_without_artificial_ambiguity() {
  let key = PKey::private_key_from_pem(&fs::read(fixture("rsa-pkcs8.pem")).unwrap()).unwrap();
  let mut name = X509NameBuilder::new().unwrap();
  name.append_entry_by_text("CN", "Public renewed identity").unwrap();
  let name = name.build();
  let mut certs = Vec::new();
  for serial in [10, 11] {
    let mut builder = X509::builder().unwrap();
    builder.set_version(2).unwrap();
    builder.set_subject_name(&name).unwrap();
    builder.set_issuer_name(&name).unwrap();
    builder.set_pubkey(&key).unwrap();
    builder
      .set_serial_number(&BigNum::from_u32(serial).unwrap().to_asn1_integer().unwrap())
      .unwrap();
    builder.set_not_before(&Asn1Time::days_from_now(0).unwrap()).unwrap();
    builder.set_not_after(&Asn1Time::days_from_now(1).unwrap()).unwrap();
    builder.sign(&key, MessageDigest::sha256()).unwrap();
    certs.push(builder.build());
  }
  let mut extra = Stack::new().unwrap();
  extra.push(certs[0].clone()).unwrap();
  let mut builder = openssl::pkcs12::Pkcs12::builder();
  builder.name("renewed").pkey(&key).cert(&certs[1]).ca(extra);
  let archive = builder.build2("fixture-password").unwrap().to_der().unwrap();
  // Independent oracle for this bag order. Native bag-order permutation tests
  // establish selection; localKeyId alone does not override native ordering.
  let parsed = openssl::pkcs12::Pkcs12::from_der(&archive)
    .unwrap()
    .parse2("fixture-password")
    .unwrap();
  assert_eq!(parsed.cert.unwrap().to_der().unwrap(), certs[1].to_der().unwrap());
  ClientIdentity::from_data(Zeroizing::new(archive), None, Some("fixture-password")).unwrap();
}
