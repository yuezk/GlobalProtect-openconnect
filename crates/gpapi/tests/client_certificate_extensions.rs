//! Issue #697: local identity matching must not apply a peer's extension policy.
//! OpenSSL is an independent test oracle/server, never a product import backend.
use std::{
  fs,
  io::{Read, Write},
  net::TcpListener,
  thread,
  time::Duration,
};

use gpapi::{
  gp_params::GpParams,
  os_profile::{ClientOs, HostIdentity, HostSnapshot, OsProfile},
  utils::request::{ClientIdentity, RequestIdentityError},
};
use openssl::{
  asn1::{Asn1Object, Asn1OctetString, Asn1Time},
  ec::{EcGroup, EcKey},
  hash::MessageDigest,
  nid::Nid,
  pkey::{Id, PKey, Private},
  ssl::{SslAcceptor, SslConnector, SslMethod, SslVerifyMode, SslVersion},
  x509::{X509, X509Extension, X509NameBuilder},
};
use zeroize::Zeroizing;

const PASSWORD: &str = "fixture-password";

fn key(family: &str) -> PKey<Private> {
  if family == "ec-p384" {
    let group = EcGroup::from_curve_name(Nid::SECP384R1).unwrap();
    return PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
  }
  if family == "ed25519" {
    return PKey::generate_ed25519().unwrap();
  }
  let bytes: &[u8] = match family {
    "rsa" => include_bytes!("files/client-identity/rsa-pkcs8.pem"),
    "ec" => include_bytes!("files/client-identity/ec-pkcs8.pem"),
    _ => panic!("unknown key family"),
  };
  PKey::private_key_from_pem(bytes).unwrap()
}

fn certificate(key: &PKey<Private>, extension: Option<(&str, bool, &[u8])>) -> X509 {
  let mut name = X509NameBuilder::new().unwrap();
  name.append_entry_by_text("CN", "Public regression identity").unwrap();
  let name = name.build();
  let mut cert = X509::builder().unwrap();
  cert.set_version(2).unwrap();
  cert.set_subject_name(&name).unwrap();
  cert.set_issuer_name(&name).unwrap();
  cert.set_pubkey(key).unwrap();
  cert
    .set_not_before(&Asn1Time::from_unix(1_700_000_000).unwrap())
    .unwrap();
  cert
    .set_not_after(&Asn1Time::from_unix(4_000_000_000).unwrap())
    .unwrap();
  if let Some((oid, critical, value)) = extension {
    let oid = Asn1Object::from_str(oid).unwrap();
    let value = Asn1OctetString::new_from_bytes(value).unwrap();
    cert
      .append_extension(X509Extension::new_from_der(&oid, critical, &value).unwrap())
      .unwrap();
  }
  let digest = if key.id() == Id::ED25519 {
    MessageDigest::null()
  } else {
    MessageDigest::sha256()
  };
  cert.sign(key, digest).unwrap();
  cert.build()
}

fn private_extension() -> Option<(&'static str, bool, &'static [u8])> {
  Some(("1.3.6.1.4.1.55555.1", true, &[0x05, 0x00]))
}

fn params(identity: ClientIdentity, ignore_tls_errors: bool) -> GpParams {
  let snapshot = HostSnapshot {
    runtime_os: ClientOs::Linux,
    identity: HostIdentity::application("regression-test".into(), "public-test-id".into()),
    os_version: "test".into(),
    software_version: "test".into(),
  };
  GpParams::builder(OsProfile::builder(ClientOs::Linux).build(snapshot))
    .client_identity(Some(identity))
    .ignore_tls_errors(ignore_tls_errors)
    .build()
}

fn assert_usable(identity: ClientIdentity) {
  reqwest::Client::try_from(&params(identity.clone(), false)).unwrap();
  reqwest::Client::try_from(&params(identity.clone(), true)).unwrap();
  let restored = ClientIdentity::from_data(
    Zeroizing::new(identity.certificate_data().to_vec()),
    identity.key_data().map(|key| Zeroizing::new(key.to_vec())),
    identity.key_password(),
  )
  .unwrap();
  assert_eq!(restored.certificate_data(), identity.certificate_data());
  let files = restored.write_files(None).unwrap();
  ClientIdentity::load(files.certificate(), files.key(), restored.key_password()).unwrap();
}

#[derive(Clone, Copy, Debug)]
enum Container {
  SeparatePem,
  CombinedPem,
  Der,
  Pkcs12,
  Snapshot,
}

fn load(container: Container, cert: &X509, key: &PKey<Private>) -> anyhow::Result<ClientIdentity> {
  let directory = tempfile::tempdir().unwrap();
  let source = directory.path().join("client.pem");
  let key_path = directory.path().join("client.key");
  let key_pem = key.private_key_to_pem_pkcs8().unwrap();
  fs::write(&key_path, &key_pem).unwrap();
  match container {
    Container::SeparatePem => {
      fs::write(&source, cert.to_pem().unwrap()).unwrap();
      ClientIdentity::load(source.to_str().unwrap(), Some(key_path.to_str().unwrap()), None)
    }
    Container::CombinedPem => {
      let mut combined = cert.to_pem().unwrap();
      combined.extend(&key_pem);
      fs::write(&source, combined).unwrap();
      ClientIdentity::load(source.to_str().unwrap(), None, None)
    }
    Container::Der => {
      fs::write(&source, cert.to_der().unwrap()).unwrap();
      fs::write(&key_path, key.private_key_to_pkcs8().unwrap()).unwrap();
      ClientIdentity::load(source.to_str().unwrap(), Some(key_path.to_str().unwrap()), None)
    }
    Container::Pkcs12 => {
      let mut archive = openssl::pkcs12::Pkcs12::builder();
      archive.name("public regression identity").pkey(key).cert(cert);
      let archive = archive.build2(PASSWORD).unwrap().to_der().unwrap();
      let source = directory.path().join("client.p12");
      fs::write(&source, archive).unwrap();
      ClientIdentity::load(source.to_str().unwrap(), None, Some(PASSWORD))
    }
    Container::Snapshot => ClientIdentity::from_data(
      Zeroizing::new(cert.to_pem().unwrap()),
      Some(Zeroizing::new(key_pem)),
      None,
    ),
  }
}

fn extension_matrix(container: Container) {
  let cases: &[(&str, Option<(&str, bool, &[u8])>)] = &[
    ("no extensions", None),
    (
      "private noncritical",
      Some(("1.3.6.1.4.1.55555.1", false, &[0x05, 0x00])),
    ),
    ("private critical", private_extension()),
    // Netscape certificate type and Microsoft certificate template name are
    // realistic enterprise extensions that WebPKI does not interpret.
    (
      "Netscape client type",
      Some(("2.16.840.1.113730.1.1", true, &[0x03, 0x02, 0x07, 0x80])),
    ),
    (
      "Microsoft template",
      Some(("1.3.6.1.4.1.311.20.2", true, &[0x1e, 0x02, 0x00, b'A'])),
    ),
    (
      "critical key usage",
      Some(("2.5.29.15", true, &[0x03, 0x02, 0x07, 0x80])),
    ),
    (
      "critical client EKU",
      Some((
        "2.5.29.37",
        true,
        &[0x30, 0x0a, 0x06, 0x08, 0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x02],
      )),
    ),
  ];
  for family in ["rsa", "ec", "ec-p384", "ed25519"] {
    let key = key(family);
    for &(name, extension) in cases {
      let cert = certificate(&key, extension);
      // The previous OpenSSL backend accepts this local certificate/key pair.
      let mut oracle = SslConnector::builder(SslMethod::tls()).unwrap();
      oracle.set_certificate(&cert).unwrap();
      oracle.set_private_key(&key).unwrap();
      oracle.check_private_key().unwrap();
      let identity =
        load(container, &cert, &key).unwrap_or_else(|error| panic!("{container:?}/{family}/{name}: {error:#}"));
      if !matches!(container, Container::Pkcs12) {
        assert_eq!(identity.certificate_data(), cert.to_pem().unwrap());
      }
      assert_usable(identity);
    }
  }
}

#[test]
fn separate_pem_extension_matrix() {
  extension_matrix(Container::SeparatePem);
}
#[test]
fn combined_pem_extension_matrix() {
  extension_matrix(Container::CombinedPem);
}
#[test]
fn der_extension_matrix() {
  extension_matrix(Container::Der);
}
#[test]
fn pkcs12_extension_matrix() {
  extension_matrix(Container::Pkcs12);
}
#[test]
fn snapshot_extension_matrix() {
  extension_matrix(Container::Snapshot);
}

#[test]
fn critical_extensions_do_not_hide_a_mismatched_private_key() {
  for family in ["rsa", "ec"] {
    let cert = certificate(&key(family), private_extension());
    let wrong_family = if family == "rsa" { "ec" } else { "rsa" };
    let same_family_wrong_key = if family == "rsa" {
      PKey::from_rsa(openssl::rsa::Rsa::generate(2048).unwrap()).unwrap()
    } else {
      let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap();
      PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap()
    };
    // PEM and service snapshots can carry mismatched material. OpenSSL's
    // PKCS#12 writer itself rejects a mismatch; existing archive fixtures cover it.
    for wrong_key in [key(wrong_family), same_family_wrong_key] {
      for container in [
        Container::SeparatePem,
        Container::CombinedPem,
        Container::Der,
        Container::Snapshot,
      ] {
        let error = load(container, &cert, &wrong_key).unwrap_err();
        assert!(
          matches!(
            error.downcast_ref::<RequestIdentityError>(),
            Some(RequestIdentityError::KeyMismatch)
          ),
          "{error:#}"
        );
      }
    }
  }
}

#[test]
fn cli_identity_preparation_keeps_critical_extensions_after_source_files_are_removed() {
  let key = key("rsa");
  let cert = certificate(&key, private_extension());
  let directory = tempfile::tempdir().unwrap();
  let source = directory.path().join("client.pem");
  let key_path = directory.path().join("client.key");
  fs::write(&source, cert.to_pem().unwrap()).unwrap();
  fs::write(&key_path, key.private_key_to_pem_pkcs8().unwrap()).unwrap();
  let identity = load(Container::Snapshot, &cert, &key).unwrap();
  let profile = params(identity, false).os_profile().clone();
  // Start with the unprepared path used by the reported gateway prelogin.
  let mut params = GpParams::builder(profile)
    .certificate(Some(source.to_string_lossy().into_owned()))
    .sslkey(Some(key_path.to_string_lossy().into_owned()))
    .build();
  reqwest::Client::try_from(&params).unwrap();
  params.prepare_client_identity().unwrap();
  drop(directory);
  reqwest::Client::try_from(&params).unwrap();
  assert_eq!(
    params.client_identity().unwrap().certificate_data(),
    cert.to_pem().unwrap()
  );
}

#[test]
fn critical_extension_in_a_chain_preserves_every_certificate() {
  let key = key("rsa");
  let cert = certificate(&key, private_extension());
  let mut chain = cert.to_pem().unwrap();
  chain.extend(include_bytes!("files/client-identity/root.pem"));
  let identity = ClientIdentity::from_data(
    Zeroizing::new(chain.clone()),
    Some(Zeroizing::new(key.private_key_to_pem_pkcs8().unwrap())),
    None,
  )
  .unwrap();
  assert_eq!(identity.certificate_data(), chain);
  assert_usable(identity);
}

fn server(
  cert: &X509,
  key: &PKey<Private>,
  version: SslVersion,
  expected_client: Vec<u8>,
  accept_client: bool,
) -> (String, thread::JoinHandle<bool>) {
  let mut acceptor = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls()).unwrap();
  acceptor.set_certificate(cert).unwrap();
  acceptor.set_private_key(key).unwrap();
  acceptor.set_min_proto_version(Some(version)).unwrap();
  acceptor.set_max_proto_version(Some(version)).unwrap();
  // Model a VPN that understands this private extension. TLS still verifies
  // CertificateVerify; the server also checks the exact leaf bytes below.
  acceptor.set_verify_callback(
    SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT,
    move |_, _| accept_client,
  );
  let acceptor = acceptor.build();
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  listener.set_nonblocking(true).unwrap();
  let address = format!("https://{}", listener.local_addr().unwrap());
  let worker = thread::spawn(move || {
    // Bound failures even if client construction or connection fails.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let stream = loop {
      match listener.accept() {
        Ok((stream, _)) => break stream,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
          if std::time::Instant::now() >= deadline {
            return false;
          }
          thread::sleep(Duration::from_millis(5));
        }
        Err(error) => panic!("{error}"),
      }
    };
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    let Ok(mut stream) = acceptor.accept(stream) else {
      return false;
    };
    let peer = stream.ssl().peer_certificate().unwrap();
    assert_eq!(
      peer.to_der().unwrap(),
      expected_client,
      "the original signed certificate must be sent unchanged"
    );
    assert!(peer.verify(&peer.public_key().unwrap()).unwrap());
    let mut request = [0; 4096];
    if stream.read(&mut request).is_err() {
      return false;
    }
    stream
      .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
      .is_ok()
  });
  (address, worker)
}

#[tokio::test]
async fn critical_extension_identity_completes_tls12_and_tls13_client_authentication() {
  let server_key = key("rsa");
  let server_cert = certificate(&server_key, None);
  for family in ["rsa", "ec", "ec-p384", "ed25519"] {
    let key = key(family);
    let cert = certificate(&key, private_extension());
    for container in [Container::SeparatePem, Container::Pkcs12] {
      for version in [SslVersion::TLS1_2, SslVersion::TLS1_3] {
        let identity = load(container, &cert, &key).unwrap();
        let client = reqwest::Client::try_from(&params(identity, true)).unwrap();
        let (address, worker) = server(&server_cert, &server_key, version, cert.to_der().unwrap(), true);
        let result = client.get(address).timeout(Duration::from_secs(5)).send().await;
        assert!(
          worker.join().unwrap(),
          "{family}/{container:?}/{version:?}: server did not complete client authentication"
        );
        assert_eq!(result.unwrap().text().await.unwrap(), "ok");
      }
    }
  }
}

#[tokio::test]
async fn client_extensions_do_not_disable_server_trust_or_peer_rejection() {
  let key = key("rsa");
  let cert = certificate(&key, private_extension());
  let server_cert = certificate(&key, None);
  for (ignore_server_errors, accept_client) in [(false, true), (true, false)] {
    for version in [SslVersion::TLS1_2, SslVersion::TLS1_3] {
      let identity = load(Container::SeparatePem, &cert, &key).unwrap();
      let client = reqwest::Client::try_from(&params(identity, ignore_server_errors)).unwrap();
      let (address, worker) = server(&server_cert, &key, version, cert.to_der().unwrap(), accept_client);
      let result = client.get(address).timeout(Duration::from_secs(5)).send().await;
      assert!(
        result.is_err(),
        "server trust and peer rejection must still be enforced"
      );
      assert!(!worker.join().unwrap());
      let error = result.unwrap_err();
      assert!(
        !error.is_timeout(),
        "expected a TLS rejection, not a timeout: {error:?}"
      );
      let details = format!("{error:?}");
      if ignore_server_errors {
        assert!(
          details.contains("AlertReceived"),
          "expected the server's client-certificate rejection: {details}"
        );
      } else {
        assert!(
          details.contains("InvalidCertificate"),
          "expected server certificate validation to fail: {details}"
        );
      }
    }
  }
}
