//! Recorded 3.2 compatibility gap: TLS 1.2 ECDSA hashes are not curve-bound.
//! OpenSSL is a test-only baseline and peer, never a product TLS backend.
#![cfg(any(
  target_os = "macos",
  target_os = "linux",
  target_os = "freebsd",
  target_os = "openbsd"
))]

use std::{
  io::{Read, Write},
  net::{TcpListener, TcpStream},
  thread,
  time::{Duration, Instant},
};

use gpapi::{
  gp_params::GpParams,
  os_profile::{ClientOs, HostIdentity, HostSnapshot, OsProfile},
  utils::request::ClientIdentity,
};
use openssl::{
  asn1::Asn1Time,
  ec::{EcGroup, EcKey},
  hash::MessageDigest,
  nid::Nid,
  pkey::{PKey, Private},
  ssl::{SslAcceptor, SslConnector, SslMethod, SslVerifyMode, SslVersion},
  x509::{X509, X509NameBuilder},
};
use zeroize::Zeroizing;

fn identity(curve: Nid, name: &str) -> (PKey<Private>, X509) {
  let group = EcGroup::from_curve_name(curve).unwrap();
  let key = PKey::from_ec_key(EcKey::generate(&group).unwrap()).unwrap();
  let mut subject = X509NameBuilder::new().unwrap();
  subject.append_entry_by_text("CN", name).unwrap();
  let subject = subject.build();
  let mut certificate = X509::builder().unwrap();
  certificate.set_version(2).unwrap();
  certificate.set_subject_name(&subject).unwrap();
  certificate.set_issuer_name(&subject).unwrap();
  certificate.set_pubkey(&key).unwrap();
  certificate
    .set_not_before(&Asn1Time::days_from_now(0).unwrap())
    .unwrap();
  certificate.set_not_after(&Asn1Time::days_from_now(1).unwrap()).unwrap();
  certificate.sign(&key, MessageDigest::sha256()).unwrap();
  (key, certificate.build())
}

fn sha256_only_tls12_peer(expected: &X509) -> (String, thread::JoinHandle<Result<(), String>>) {
  let (key, certificate) = identity(Nid::X9_62_PRIME256V1, "Public TLS 1.2 peer");
  let mut acceptor = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls()).unwrap();
  acceptor.set_certificate(&certificate).unwrap();
  acceptor.set_private_key(&key).unwrap();
  acceptor.set_min_proto_version(Some(SslVersion::TLS1_2)).unwrap();
  acceptor.set_max_proto_version(Some(SslVersion::TLS1_2)).unwrap();
  // P-256 server authentication works with this list. The P-384 client must
  // also sign with SHA-256, which is allowed in TLS 1.2 but not TLS 1.3.
  acceptor.set_sigalgs_list("ECDSA+SHA256").unwrap();
  let expected = expected.to_der().unwrap();
  let pinned = expected.clone();
  acceptor.set_verify_callback(
    SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT,
    move |_, context| {
      context.error_depth() == 0
        && context
          .current_cert()
          .is_some_and(|certificate| certificate.to_der().unwrap() == pinned)
    },
  );
  let acceptor = acceptor.build();
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  listener.set_nonblocking(true).unwrap();
  let address = listener.local_addr().unwrap().to_string();
  let worker = thread::spawn(move || {
    let deadline = Instant::now() + Duration::from_secs(5);
    let stream = loop {
      match listener.accept() {
        Ok((stream, _)) => break stream,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
          thread::sleep(Duration::from_millis(5));
        }
        Err(error) => return Err(format!("Accepting TLS peer: {error}")),
      }
    };
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    // SSL_accept checks the client's CertificateVerify proof of possession.
    let mut stream = acceptor
      .accept(stream)
      .map_err(|error| format!("Client authentication: {error}"))?;
    assert_eq!(stream.ssl().version2(), Some(SslVersion::TLS1_2));
    assert_eq!(stream.ssl().peer_certificate().unwrap().to_der().unwrap(), expected);
    let mut request = [0; 4096];
    let length = stream.read(&mut request).map_err(|error| error.to_string())?;
    assert!(length > 0);
    stream
      .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
      .map_err(|error| error.to_string())
  });
  (address, worker)
}

#[test]
fn openssl_baseline_authenticates_p384_with_tls12_sha256() {
  let (key, certificate) = identity(Nid::SECP384R1, "Public P-384 client");
  let (address, peer) = sha256_only_tls12_peer(&certificate);
  let mut connector = SslConnector::builder(SslMethod::tls()).unwrap();
  connector.set_certificate(&certificate).unwrap();
  connector.set_private_key(&key).unwrap();
  // This fixture isolates client authentication; the peer pins the exact leaf.
  connector.set_verify(SslVerifyMode::NONE);
  let socket = TcpStream::connect(address).unwrap();
  socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
  socket.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
  let mut stream = connector.build().connect("localhost", socket).unwrap();
  stream.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
  let mut response = [0; 1024];
  assert!(stream.read(&mut response).unwrap() > 0);
  peer.join().unwrap().unwrap();
}

#[tokio::test]
#[ignore = "Known regression from 3.2: P-384 signs only SHA-384; Rustls SigningKey has no negotiated version to safely allow TLS 1.2 SHA-256 while preserving TLS 1.3 curve binding"]
async fn p384_authenticates_with_tls12_sha256() {
  let (key, certificate) = identity(Nid::SECP384R1, "Public P-384 client");
  let identity = ClientIdentity::from_data(
    Zeroizing::new(certificate.to_pem().unwrap()),
    Some(Zeroizing::new(key.private_key_to_pem_pkcs8().unwrap())),
    None,
  )
  .unwrap();
  let snapshot = HostSnapshot {
    runtime_os: ClientOs::Linux,
    identity: HostIdentity::application("regression-test".into(), "public-test-id".into()),
    os_version: "test".into(),
    software_version: "test".into(),
  };
  let params = GpParams::builder(OsProfile::builder(ClientOs::Linux).build(snapshot))
    .client_identity(Some(identity))
    .ignore_tls_errors(true)
    .build();
  let client = reqwest::Client::try_from(&params).unwrap();
  let (address, peer) = sha256_only_tls12_peer(&certificate);
  let response = client
    .get(format!("https://{address}"))
    .timeout(Duration::from_secs(5))
    .send()
    .await;
  let authentication = peer.join().unwrap();
  assert!(authentication.is_ok(), "{authentication:?}; HTTP result: {response:?}");
  assert_eq!(response.unwrap().text().await.unwrap(), "ok");
}
