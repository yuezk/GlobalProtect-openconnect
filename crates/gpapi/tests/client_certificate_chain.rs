//! A PKCS#12 CA rollover must send the intermediate that signed the client.
use gpapi::{
  gp_params::GpParams,
  os_profile::{ClientOs, HostIdentity, HostSnapshot, OsProfile},
  utils::request::ClientIdentity,
};
use openssl::{
  asn1::Asn1Time,
  bn::BigNum,
  ec::{EcGroup, EcKey},
  hash::MessageDigest,
  nid::Nid,
  pkey::{PKey, Private},
  ssl::{SslAcceptor, SslMethod, SslVerifyMode},
  stack::Stack,
  x509::{
    X509, X509NameBuilder,
    extension::{AuthorityKeyIdentifier, BasicConstraints, KeyUsage, SubjectKeyIdentifier},
  },
};
use std::{
  io::{Read, Write},
  net::TcpListener,
  thread,
  time::Duration,
};
use zeroize::Zeroizing;

fn key() -> PKey<Private> {
  PKey::from_ec_key(EcKey::generate(&EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap()).unwrap()).unwrap()
}

fn certificate(
  name: &str,
  key: &PKey<Private>,
  issuer: Option<(&X509, &PKey<Private>)>,
  serial: u32,
  ca: bool,
  key_identifiers: bool,
) -> X509 {
  let mut subject = X509NameBuilder::new().unwrap();
  subject.append_entry_by_text("CN", name).unwrap();
  let subject = subject.build();
  let mut cert = X509::builder().unwrap();
  cert.set_version(2).unwrap();
  cert
    .set_serial_number(&BigNum::from_u32(serial).unwrap().to_asn1_integer().unwrap())
    .unwrap();
  cert.set_subject_name(&subject).unwrap();
  cert
    .set_issuer_name(issuer.map(|(cert, _)| cert.subject_name()).unwrap_or(&subject))
    .unwrap();
  cert.set_pubkey(key).unwrap();
  cert
    .set_not_before(&Asn1Time::from_unix(1_700_000_000).unwrap())
    .unwrap();
  cert
    .set_not_after(&Asn1Time::from_unix(4_000_000_000).unwrap())
    .unwrap();
  let mut constraints = BasicConstraints::new();
  if ca {
    constraints.ca();
  }
  cert.append_extension(constraints.critical().build().unwrap()).unwrap();
  let mut usage = KeyUsage::new();
  if ca {
    usage.key_cert_sign();
  } else {
    usage.digital_signature();
  }
  cert.append_extension(usage.critical().build().unwrap()).unwrap();
  if key_identifiers {
    let identifier = SubjectKeyIdentifier::new()
      .build(&cert.x509v3_context(None, None))
      .unwrap();
    cert.append_extension(identifier).unwrap();
    if let Some((parent, _)) = issuer {
      let identifier = AuthorityKeyIdentifier::new()
        .keyid(true)
        .build(&cert.x509v3_context(Some(parent), None))
        .unwrap();
      cert.append_extension(identifier).unwrap();
    }
  }
  cert
    .sign(issuer.map(|(_, key)| key).unwrap_or(key), MessageDigest::sha256())
    .unwrap();
  cert.build()
}

#[tokio::test]
async fn pkcs12_ca_rollover_authenticates_in_both_bag_orders() {
  for self_issued in [false, true] {
    let root_key = key();
    // Ordinary rollover also works without AKI/SKI. The self-issued chain
    // needs key identifiers so the independent server distinguishes its root.
    let root = certificate("Root", &root_key, None, 1, true, self_issued);
    let issuer_name = if self_issued { "Root" } else { "Issuing CA" };
    let old_key = key();
    let old = certificate(issuer_name, &old_key, Some((&root, &root_key)), 2, true, self_issued);
    let current_key = key();
    let current = certificate(
      issuer_name,
      &current_key,
      Some((&root, &root_key)),
      3,
      true,
      self_issued,
    );
    let client_key = key();
    let leaf = certificate(
      "Client",
      &client_key,
      Some((&current, &current_key)),
      4,
      false,
      self_issued,
    );
    for old_first in [true, false] {
      let mut ca = Stack::new().unwrap();
      let intermediates = if old_first { [&old, &current] } else { [&current, &old] };
      for cert in intermediates {
        ca.push(cert.clone()).unwrap();
      }
      ca.push(root.clone()).unwrap();
      authenticate(&root, &current, &leaf, &client_key, ca).await;
    }
  }
}

async fn authenticate(root: &X509, current: &X509, leaf: &X509, client_key: &PKey<Private>, ca: Stack<X509>) {
  let archive = openssl::pkcs12::Pkcs12::builder()
    .pkey(client_key)
    .cert(leaf)
    .ca(ca)
    .build2("test")
    .unwrap()
    .to_der()
    .unwrap();
  let identity = ClientIdentity::from_data(Zeroizing::new(archive.clone()), None, Some("test")).unwrap();
  assert_eq!(
    identity.certificate_data(),
    archive,
    "OpenConnect must retain the original archive"
  );
  let profile = OsProfile::builder(ClientOs::Linux).build(HostSnapshot {
    runtime_os: ClientOs::Linux,
    identity: HostIdentity::application("test".into(), "test".into()),
    os_version: "test".into(),
    software_version: "test".into(),
  });
  let params = GpParams::builder(profile)
    .client_identity(Some(identity))
    .ignore_tls_errors(true)
    .build();
  let client = reqwest::Client::try_from(&params).unwrap();
  let server_key = key();
  let mut server = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls()).unwrap();
  server
    .set_certificate(&certificate("Server", &server_key, None, 5, false, false))
    .unwrap();
  server.set_private_key(&server_key).unwrap();
  server.cert_store_mut().add_cert(root.clone()).unwrap();
  server.set_verify(SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT);
  let server = server.build();
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  listener.set_nonblocking(true).unwrap();
  let address = format!("https://{}", listener.local_addr().unwrap());
  let expected_leaf = leaf.to_der().unwrap();
  let expected_issuer = current.to_der().unwrap();
  let worker = thread::spawn(move || {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let socket = loop {
      match listener.accept() {
        Ok((socket, _)) => break socket,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && std::time::Instant::now() < deadline => {
          thread::sleep(Duration::from_millis(5))
        }
        Err(error) => return Err(error.to_string()),
      }
    };
    socket.set_nonblocking(false).unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    socket.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut socket = server.accept(socket).map_err(|error| error.to_string())?;
    assert_eq!(
      socket.ssl().peer_certificate().unwrap().to_der().unwrap(),
      expected_leaf
    );
    assert!(
      socket
        .ssl()
        .peer_cert_chain()
        .unwrap()
        .iter()
        .any(|cert| cert.to_der().unwrap() == expected_issuer)
    );
    let mut request = [0; 4096];
    socket.read(&mut request).map_err(|error| error.to_string())?;
    socket
      .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
      .map_err(|error| error.to_string())
  });
  let response = client.get(address).timeout(Duration::from_secs(5)).send().await;
  let server_result = worker.join().unwrap();
  assert!(server_result.is_ok(), "{server_result:?}; {response:?}");
  assert_eq!(response.unwrap().text().await.unwrap(), "ok");
}

#[tokio::test]
async fn pkcs12_pss_certificate_signature_parameters_authenticate() {
  let root = X509::from_pem(include_bytes!("files/client-identity/pss-chain-root.pem")).unwrap();
  let issuer = X509::from_pem(include_bytes!("files/client-identity/pss-chain-issuer.pem")).unwrap();
  let client_key = PKey::private_key_from_pem(include_bytes!("files/client-identity/rsa-pkcs8.pem")).unwrap();
  let wrong_key = PKey::from_rsa(openssl::rsa::Rsa::generate(2048).unwrap()).unwrap();
  let wrong_issuer = certificate("PSS Issuing CA", &wrong_key, None, 6, true, false);
  for certificate in [
    include_bytes!("files/client-identity/pss-chain-salt-20.pem").as_slice(),
    include_bytes!("files/client-identity/pss-chain-salt-32.pem").as_slice(),
    include_bytes!("files/client-identity/pss-chain-salt-48.pem").as_slice(),
  ] {
    let leaf = X509::from_pem(certificate).unwrap();
    assert!(leaf.verify(&issuer.public_key().unwrap()).unwrap());
    for wrong_first in [true, false] {
      let mut ca = Stack::new().unwrap();
      let candidates = if wrong_first {
        [&wrong_issuer, &issuer]
      } else {
        [&issuer, &wrong_issuer]
      };
      for candidate in candidates {
        ca.push(candidate.clone()).unwrap();
      }
      ca.push(root.clone()).unwrap();
      authenticate(&root, &issuer, &leaf, &client_key, ca).await;
    }
  }
}
