use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

use super::PortalError;

/// A VPN HTTP TLS failure. The server identifies the configured VPN origin,
/// not necessarily the peer that failed after an HTTP redirect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerTlsError {
  pub server: String,
  pub kind: ServerTlsErrorKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServerTlsErrorKind {
  Certificate,
  ClientCertificateRequired,
  Other,
}

impl ServerTlsError {
  pub(crate) fn from_request(server: &url::Url, error: &(dyn Error + 'static)) -> Option<Self> {
    Some(Self {
      server: server.origin().ascii_serialization(),
      kind: classify(error)?,
    })
  }
}

impl fmt::Display for ServerTlsError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let message = match self.kind {
      ServerTlsErrorKind::Certificate => "The server's certificate could not be verified",
      ServerTlsErrorKind::ClientCertificateRequired => "The server requires a client certificate",
      ServerTlsErrorKind::Other => "A secure connection to the server could not be established",
    };
    write!(f, "{message}: {}", self.server)
  }
}

impl Error for ServerTlsError {}

impl ServerTlsErrorKind {
  /// One classification interface for CLI hints and desktop error mapping.
  pub fn from_error(error: &anyhow::Error) -> Option<Self> {
    error.chain().find_map(|source| {
      source
        .downcast_ref::<ServerTlsError>()
        .map(|error| error.kind)
        .or_else(|| {
          source
            .downcast_ref::<PortalError>()
            .and_then(PortalError::tls_error_kind)
        })
    })
  }
}

pub(super) fn classify(error: &(dyn Error + 'static)) -> Option<ServerTlsErrorKind> {
  let mut source = Some(error);
  while let Some(error) = source {
    if let Some(tls) = error.downcast_ref::<ServerTlsError>() {
      return Some(tls.kind);
    }
    if let Some(tls) = error.downcast_ref::<rustls::Error>() {
      return Some(match tls {
        rustls::Error::InvalidCertificate(_) => ServerTlsErrorKind::Certificate,
        rustls::Error::AlertReceived(rustls::AlertDescription::CertificateRequired) => {
          ServerTlsErrorKind::ClientCertificateRequired
        }
        _ => ServerTlsErrorKind::Other,
      });
    }
    source = error
      .downcast_ref::<std::io::Error>()
      .and_then(|error| error.get_ref().map(|inner| inner as &(dyn Error + 'static)))
      .or_else(|| error.source());
  }
  None
}

#[cfg(test)]
mod tests {
  use super::*;
  use openssl::{
    asn1::Asn1Time,
    hash::MessageDigest,
    pkey::PKey,
    rsa::Rsa,
    ssl::{SslAcceptor, SslMethod, SslVerifyMode},
    x509::{X509, X509NameBuilder},
  };
  use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
  };

  #[test]
  fn typed_tls_causes_distinguish_local_trust_from_peer_alerts() {
    for reason in [
      rustls::CertificateError::Expired,
      rustls::CertificateError::UnknownIssuer,
    ] {
      let error = std::io::Error::other(std::io::Error::other(rustls::Error::InvalidCertificate(reason)));
      assert_eq!(classify(&error), Some(ServerTlsErrorKind::Certificate));
    }
    for alert in [
      rustls::AlertDescription::UnknownCA,
      rustls::AlertDescription::BadCertificate,
    ] {
      assert_eq!(
        classify(&rustls::Error::AlertReceived(alert)),
        Some(ServerTlsErrorKind::Other)
      );
    }
    let required = rustls::Error::AlertReceived(rustls::AlertDescription::CertificateRequired);
    assert_eq!(classify(&required), Some(ServerTlsErrorKind::ClientCertificateRequired));
    assert_eq!(classify(&std::io::Error::other("certificate verify failed")), None);
  }

  fn server(tls: bool) -> (String, thread::JoinHandle<()>) {
    server_with_client_auth(tls, SslVerifyMode::NONE)
  }

  fn server_with_client_auth(tls: bool, verify: SslVerifyMode) -> (String, thread::JoinHandle<()>) {
    let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "localhost").unwrap();
    let name = name.build();
    let mut cert = X509::builder().unwrap();
    cert.set_version(2).unwrap();
    cert.set_subject_name(&name).unwrap();
    cert.set_issuer_name(&name).unwrap();
    cert.set_pubkey(&key).unwrap();
    cert.set_not_before(&Asn1Time::days_from_now(0).unwrap()).unwrap();
    cert.set_not_after(&Asn1Time::days_from_now(1).unwrap()).unwrap();
    cert.sign(&key, MessageDigest::sha256()).unwrap();
    let mut acceptor = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls()).unwrap();
    acceptor.set_private_key(&key).unwrap();
    acceptor.set_certificate(&cert.build()).unwrap();
    acceptor.set_verify(verify);
    acceptor
      .set_ca_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/files/client-identity/root.pem"
      ))
      .unwrap();
    let acceptor = acceptor.build();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("https://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
      let (mut stream, _) = listener.accept().unwrap();
      stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
      stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
      let body = "<prelogin-response><status>Success</status><saml-auth-method>REDIRECT</saml-auth-method><saml-request>aHR0cHM6Ly9pZHAuZXhhbXBsZS5jb20v</saml-request></prelogin-response>";
      let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
      );
      let mut request = [0; 8192];
      if tls {
        if let Ok(mut stream) = acceptor.accept(stream) {
          let _ = stream.read(&mut request);
          let _ = stream.write_all(response.as_bytes());
        }
      } else {
        let _ = stream.read(&mut request);
        let _ = stream.write_all(response.as_bytes());
      }
    });
    (address, worker)
  }

  #[tokio::test]
  async fn prelogin_returns_typed_certificate_failure_with_only_the_server_origin() {
    use crate::{
      gp_params::GpParams,
      os_profile::{ClientOs, OsProfile},
      portal::{PreloginOptions, prelogin},
    };
    let (address, worker) = server(true);
    let profile = OsProfile::builder(ClientOs::Linux).build(crate::os_profile::test_snapshot());
    let params = GpParams::builder(profile).build();
    let error = prelogin(&address, &params, PreloginOptions::default())
      .await
      .unwrap_err();
    worker.join().unwrap();
    let tls = error
      .downcast_ref::<ServerTlsError>()
      .unwrap_or_else(|| panic!("unclassified: {error:?}"));
    assert_eq!(tls.kind, ServerTlsErrorKind::Certificate);
    assert_eq!(tls.server, address);
  }

  #[tokio::test]
  async fn certificate_bypass_allows_prelogin() {
    use crate::{
      gp_params::GpParams,
      os_profile::{ClientOs, OsProfile},
      portal::{PreloginOptions, prelogin},
    };
    let (address, worker) = server(true);
    let params = GpParams::builder(OsProfile::builder(ClientOs::Linux).build(crate::os_profile::test_snapshot()))
      .ignore_tls_errors(true)
      .build();
    let result = prelogin(&address, &params, PreloginOptions::default()).await;
    worker.join().unwrap();
    assert!(matches!(result.unwrap(), crate::portal::Prelogin::Saml(_)));
  }

  #[tokio::test]
  async fn incompatible_tls_is_not_a_certificate_error_even_with_bypass() {
    let (address, worker) = server(false);
    let error = crate::utils::request::client_builder()
      .no_proxy()
      .danger_accept_invalid_certs(true)
      .build()
      .unwrap()
      .get(&address)
      .send()
      .await
      .unwrap_err();
    worker.join().unwrap();
    let tls = ServerTlsError::from_request(&url::Url::parse(&address).unwrap(), &error).unwrap();
    assert_eq!(tls.kind, ServerTlsErrorKind::Other);
  }

  #[tokio::test]
  async fn connection_refused_is_not_tls() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("https://{}", listener.local_addr().unwrap());
    drop(listener);
    let error = crate::utils::request::client_builder()
      .no_proxy()
      .build()
      .unwrap()
      .get(address)
      .send()
      .await
      .unwrap_err();
    assert_eq!(classify(&error), None);
  }

  #[cfg(feature = "desktop")]
  #[test]
  fn rendering_recovery_retains_client_certificate_requirement() {
    use crate::process::rendering_recovery::{RecoveryState, RenderingCrash};
    use std::os::unix::process::ExitStatusExt;
    let mut recovery = RecoveryState::default();
    assert!(
      recovery.recover(
        &RenderingCrash {
          status: std::process::ExitStatus::from_raw(71 << 8)
        }
        .into()
      )
    );
    let error = recovery.terminal(
      ServerTlsError {
        server: "https://vpn.example.com".into(),
        kind: ServerTlsErrorKind::ClientCertificateRequired,
      }
      .into(),
    );
    assert_eq!(
      ServerTlsErrorKind::from_error(&error),
      Some(ServerTlsErrorKind::ClientCertificateRequired)
    );
  }

  #[tokio::test]
  async fn required_client_certificate_is_not_bypassed_by_ignoring_server_trust() {
    use crate::{
      gp_params::GpParams,
      os_profile::{ClientOs, OsProfile},
      portal::{PreloginOptions, prelogin},
    };
    let params = GpParams::builder(OsProfile::builder(ClientOs::Linux).build(crate::os_profile::test_snapshot()))
      .ignore_tls_errors(true)
      .build();
    let credential = crate::credential::PasswordCredential::new("alice", "password").into();
    let cancellation = tokio_util::sync::CancellationToken::new();
    for operation in ["prelogin", "config", "gateway"] {
      let (address, worker) = server_with_client_auth(true, SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT);
      let error = match operation {
        "prelogin" => prelogin(&address, &params, PreloginOptions::default())
          .await
          .err()
          .unwrap(),
        "config" => crate::portal::retrieve_config(&address, &credential, &params, &cancellation)
          .await
          .err()
          .unwrap(),
        _ => crate::gateway::GatewayLoginClient::new(&address, params.clone())
          .unwrap()
          .login(&credential, None, &cancellation)
          .await
          .err()
          .unwrap(),
      };
      worker.join().unwrap();
      let kind =
        ServerTlsErrorKind::from_error(&error).unwrap_or_else(|| panic!("{operation}: unclassified: {error:?}"));
      assert_eq!(
        kind,
        ServerTlsErrorKind::ClientCertificateRequired,
        "{operation}: {error:?}"
      );
    }
  }

  #[tokio::test]
  async fn prelogin_accepts_a_valid_identity_and_an_optional_certificate_request() {
    use crate::{
      gp_params::GpParams,
      os_profile::{ClientOs, OsProfile},
      portal::{PreloginOptions, prelogin},
    };
    for required in [false, true] {
      let verify = if required {
        SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT
      } else {
        SslVerifyMode::PEER
      };
      let (address, worker) = server_with_client_auth(true, verify);
      let mut builder =
        GpParams::builder(OsProfile::builder(ClientOs::Linux).build(crate::os_profile::test_snapshot()));
      builder.ignore_tls_errors(true);
      if required {
        builder
          .certificate(Some(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/files/client-identity/rsa-cert.pem").to_owned(),
          ))
          .sslkey(Some(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/files/client-identity/rsa-pkcs8.pem").to_owned(),
          ));
      }
      let result = prelogin(&address, &builder.build(), PreloginOptions::default()).await;
      worker.join().unwrap();
      assert!(matches!(result.unwrap(), crate::portal::Prelogin::Saml(_)));
    }
  }

  #[tokio::test]
  async fn imported_identity_formats_complete_mutual_tls_prelogin() {
    use crate::{
      gp_params::GpParams,
      os_profile::{ClientOs, OsProfile},
      portal::{PreloginOptions, prelogin},
    };
    let fixture = |name: &str| format!("{}/tests/files/client-identity/{name}", env!("CARGO_MANIFEST_DIR"));
    for (certificate, key, password) in [
      (
        "rsa-cert.pem",
        Some("rsa-traditional-aes256.pem"),
        Some("fixture-password"),
      ),
      ("ec-cert.pem", Some("ec-traditional-des3.pem"), Some("fixture-password")),
      ("rsa-cert.pem", Some("rsa-pbes1-3des.pem"), Some("fixture-password")),
      ("rsa-cert.pem", Some("rsa-pbes2-scrypt.pem"), Some("fixture-password")),
      ("ec-cert.pem", Some("ec-sec1.pem"), None),
      ("modern.p12", None, Some("fixture-password")),
      ("legacy-rc2.p12", None, Some("fixture-password")),
      ("empty-password.p12", None, None),
      ("no-bag-ids.p12", None, None),
    ] {
      let (address, worker) = server_with_client_auth(true, SslVerifyMode::PEER | SslVerifyMode::FAIL_IF_NO_PEER_CERT);
      let mut builder =
        GpParams::builder(OsProfile::builder(ClientOs::Linux).build(crate::os_profile::test_snapshot()));
      builder
        .ignore_tls_errors(true)
        .certificate(Some(fixture(certificate)))
        .sslkey(key.map(fixture))
        .key_password(password.map(str::to_owned));
      let mut params = builder.build();
      params
        .prepare_client_identity()
        .unwrap_or_else(|error| panic!("{certificate}/{key:?}: {error:?}"));
      let result = prelogin(&address, &params, PreloginOptions::default()).await;
      worker.join().unwrap();
      assert!(matches!(
        result.unwrap_or_else(|error| panic!("{certificate}/{key:?}: {error:?}")),
        crate::portal::Prelogin::Saml(_)
      ));
    }
  }

  /// Point at the GnuTLS fixture configured without RFC 5746, with a trusted test root.
  #[tokio::test]
  #[ignore = "requires GP_TLS_TEST_SERVER and GP_TLS_TEST_CA for the external GnuTLS fixture"]
  async fn tls12_peer_without_renegotiation_extension_uses_default_policy() {
    let server = std::env::var("GP_TLS_TEST_SERVER").unwrap();
    let root = std::fs::read(std::env::var("GP_TLS_TEST_CA").unwrap()).unwrap();
    let client = crate::utils::request::client_builder()
      .no_proxy()
      .add_root_certificate(reqwest::Certificate::from_pem(&root).unwrap())
      .build()
      .unwrap();
    assert!(client.get(&server).send().await.unwrap().status().is_success());
    let error = crate::utils::request::client_builder()
      .no_proxy()
      .build()
      .unwrap()
      .get(&server)
      .send()
      .await
      .unwrap_err();
    assert_eq!(
      ServerTlsError::from_request(&url::Url::parse(&server).unwrap(), &error)
        .unwrap()
        .kind,
      ServerTlsErrorKind::Certificate
    );
  }

  #[tokio::test]
  async fn failed_response_reads_preserve_the_transport_cause() {
    for status in [200, 403] {
      let listener = TcpListener::bind("127.0.0.1:0").unwrap();
      let address = format!("http://{}", listener.local_addr().unwrap());
      let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut request = [0; 8192];
        stream.read(&mut request).unwrap();
        write!(
          stream,
          "HTTP/1.1 {status} Test\r\nContent-Length: 100\r\nConnection: close\r\n\r\ntruncated"
        )
        .unwrap();
      });
      let response = crate::utils::request::client_builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(address)
        .send()
        .await
        .unwrap();
      let error = crate::utils::parse_gp_response(response).await.unwrap_err();
      assert_eq!(error.status.as_u16(), status);
      assert!(error.source().unwrap().is::<reqwest::Error>());
      worker.join().unwrap();
    }
  }
}
