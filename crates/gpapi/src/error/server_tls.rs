use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

use super::PortalError;

#[cfg(target_os = "macos")]
#[path = "server_tls/macos.rs"]
mod backend;
#[cfg(not(target_os = "macos"))]
#[path = "server_tls/openssl.rs"]
mod backend;

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
  LegacyRenegotiation,
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
      ServerTlsErrorKind::LegacyRenegotiation => "The server requires unsafe legacy TLS renegotiation",
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
  let mut fallback = None;
  while let Some(error) = source {
    if let Some(tls) = error.downcast_ref::<ServerTlsError>() {
      return Some(tls.kind);
    }
    if let Some(kind) = backend::classify(error) {
      return Some(kind);
    }
    if let Some(tls) = error.downcast_ref::<native_tls::Error>() {
      let kind = classify_native(tls);
      if kind != ServerTlsErrorKind::Other {
        return Some(kind);
      }
      fallback = Some(kind);
    }
    source = error
      .downcast_ref::<std::io::Error>()
      .and_then(|error| error.get_ref().map(|inner| inner as &(dyn Error + 'static)))
      .or_else(|| error.source());
  }
  fallback
}

fn classify_native(error: &native_tls::Error) -> ServerTlsErrorKind {
  // native-tls hides some backend reason codes and verification results. Keep
  // backend text matching here, scoped to a confirmed native TLS error.
  let detail = format!("{error:?}").to_ascii_lowercase();
  classify_native_detail(&detail)
}

fn classify_native_detail(detail: &str) -> ServerTlsErrorKind {
  // native-tls forwards Security.framework's Debug representation, but hides
  // its OSStatus from the public error chain. Prefer that status to localized
  // text, including for peer alerts that can mention a rejected client cert.
  if let Some(code) = detail
    .strip_prefix("error { code: ")
    .and_then(|detail| detail.split_once(','))
    .and_then(|(code, _)| code.parse::<i32>().ok())
  {
    return classify_macos_status(code);
  }
  if detail.contains("unsafe legacy renegotiation") {
    return ServerTlsErrorKind::LegacyRenegotiation;
  }
  if detail.contains("alert certificate required") {
    return ServerTlsErrorKind::ClientCertificateRequired;
  }
  let certificate = [
    "certificate verify failed",
    "certificate verification failed",
    "certificate was not trusted",
    "certificate is not trusted",
    "self signed certificate",
    "hostname mismatch",
    "certificate has expired",
    "certificate is not yet valid",
    "certificate has been revoked",
  ]
  .iter()
  .any(|reason| detail.contains(reason));
  if certificate {
    ServerTlsErrorKind::Certificate
  } else {
    ServerTlsErrorKind::Other
  }
}

fn classify_macos_status(code: i32) -> ServerTlsErrorKind {
  if code == -9863 {
    return ServerTlsErrorKind::ClientCertificateRequired;
  }
  // Local server-certificate validation failures from Security/SecBase.h.
  // Peer alerts (errSSLPeer*) reject our client certificate; protocol/cipher,
  // keychain, and internal errors also cannot be fixed by skipping server trust.
  let certificate = matches!(
    code,
    -9807  // errSSLXCertChainInvalid
      | -9812 // errSSLUnknownRootCert
      | -9813 // errSSLNoRootCert
      | -9814 // errSSLCertExpired
      | -9815 // errSSLCertNotYetValid
      | -9843 // errSSLHostNameMismatch
      | -9884 // errSSLATSLeafCertificateHashAlgorithmViolation
      | -9885 // errSSLATSCertificateHashAlgorithmViolation
      | -9886 // errSSLATSCertificateTrustViolation
      | -67602 // errSecHostNameMismatch
      | -67603 // errSecUnknownCriticalExtensionFlag
      | -67604 // errSecNoBasicConstraints
      | -67605 // errSecNoBasicConstraintsCA
      | -67606 // errSecInvalidAuthorityKeyID
      | -67607 // errSecInvalidSubjectKeyID
      | -67608 // errSecInvalidKeyUsageForPolicy
      | -67609 // errSecInvalidExtendedKeyUsage
      | -67610 // errSecInvalidIDLinkage
      | -67611 // errSecPathLengthConstraintExceeded
      | -67612 // errSecInvalidRoot
      | -67629 // errSecSSLBadExtendedKeyUsage
      | -67654 // errSecTrustSettingDeny
      | -67818 // errSecCertificateExpired
      | -67819 // errSecCertificateNotValidYet
      | -67820 // errSecCertificateRevoked
      | -67821 // errSecCertificateSuspended
      | -67826 // errSecInvalidCertAuthority
      | -67843 // errSecNotTrusted
      | -67899 // errSecCertificatePolicyNotAllowed
      | -67900 // errSecCertificateNameNotAllowed
      | -67901 // errSecCertificateValidityPeriodTooLong
      | -67902 // errSecCertificateIsCA
      | -67903 // errSecCertificateDuplicateExtension
      | -67904 // errSecMissingQualifiedCertStatement
  );
  if certificate {
    ServerTlsErrorKind::Certificate
  } else {
    ServerTlsErrorKind::Other
  }
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
  fn explicit_client_certificate_requirement_has_a_distinct_kind() {
    let kind = classify_native_detail("error { code: -9863, message: \"localized message\" }");
    assert_eq!(serde_json::to_value(kind).unwrap(), "clientCertificateRequired");
    assert_eq!(
      classify_native_detail("error { code: -9842, message: \"client cert requested\" }"),
      ServerTlsErrorKind::Other
    );
  }

  #[test]
  fn macos_certificate_statuses_do_not_depend_on_the_error_message() {
    for code in [-67818, -67819, -67820, -67843, -67602, -67629, -67899, -67901] {
      let detail = format!("error {{ code: {code}, message: \"localized message\" }}");
      assert_eq!(
        classify_native_detail(&detail),
        ServerTlsErrorKind::Certificate,
        "{code}"
      );
    }
  }

  #[test]
  fn macos_peer_alerts_and_handshake_errors_are_not_certificate_validation_failures() {
    for (code, message) in [
      (-9800, "SSL protocol error"),
      (-9801, "Cipher Suite negotiation failure"),
      (-9828, "peer certificate has expired"),
      (-9831, "unknown certificate authority"),
      (-9850, "weak ephemeral DH key"),
    ] {
      let detail = format!("error {{ code: {code}, message: \"{message}\" }}").to_ascii_lowercase();
      assert_eq!(classify_native_detail(&detail), ServerTlsErrorKind::Other, "{code}");
    }
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
    let profile = OsProfile::builder(ClientOs::Linux).build();
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
    let params = GpParams::builder(OsProfile::builder(ClientOs::Linux).build())
      .ignore_tls_errors(true)
      .build();
    let result = prelogin(&address, &params, PreloginOptions::default()).await;
    worker.join().unwrap();
    assert!(matches!(result.unwrap(), crate::portal::Prelogin::Saml(_)));
  }

  #[tokio::test]
  async fn incompatible_tls_is_not_a_certificate_error_even_with_bypass() {
    let (address, worker) = server(false);
    let error = reqwest::Client::builder()
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
    let error = reqwest::Client::builder()
      .no_proxy()
      .build()
      .unwrap()
      .get(address)
      .send()
      .await
      .unwrap_err();
    assert_eq!(classify(&error), None);
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn wrapped_macos_io_status_retains_the_client_certificate_requirement() {
    let error = std::io::Error::other(std::io::Error::other(security_framework::base::Error::from(-9863)));
    let server = url::Url::parse("https://vpn.example.com/login?token=secret").unwrap();
    let tls = ServerTlsError::from_request(&server, &error).unwrap();
    assert_eq!(tls.kind, ServerTlsErrorKind::ClientCertificateRequired);
    assert_eq!(tls.server, "https://vpn.example.com");
    assert_eq!(classify(&std::io::Error::other("alert certificate required")), None);
  }

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
    let params = GpParams::builder(OsProfile::builder(ClientOs::Linux).build())
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
      // Secure Transport's TLS 1.2 handshake can return only a generic peer alert.
      if cfg!(target_os = "macos") {
        assert!(matches!(
          kind,
          ServerTlsErrorKind::ClientCertificateRequired | ServerTlsErrorKind::Other
        ));
      } else {
        assert_eq!(
          kind,
          ServerTlsErrorKind::ClientCertificateRequired,
          "{operation}: {error:?}"
        );
      }
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
      let mut builder = GpParams::builder(OsProfile::builder(ClientOs::Linux).build());
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
      let response = reqwest::Client::builder()
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
