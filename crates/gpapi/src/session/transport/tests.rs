use super::*;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::TcpListener,
  task::JoinHandle,
};

pub(crate) async fn gateway(responses: Vec<String>) -> (GatewayTransport, JoinHandle<Vec<(String, String)>>) {
  gateway_fixture(responses, None).await
}

pub(crate) async fn login_gateway(responses: Vec<String>) -> (Client, Url, JoinHandle<Vec<(String, String)>>) {
  let (transport, server) = gateway(responses).await;
  (transport.client, transport.origin, server)
}

pub(crate) async fn gateway_with_first_response_gate(
  responses: Vec<String>,
  arrived: std::sync::Arc<tokio::sync::Notify>,
  release: std::sync::Arc<tokio::sync::Notify>,
) -> (GatewayTransport, JoinHandle<Vec<(String, String)>>) {
  gateway_fixture(responses, Some((arrived, release))).await
}

async fn gateway_fixture(
  responses: Vec<String>,
  gate: Option<(std::sync::Arc<tokio::sync::Notify>, std::sync::Arc<tokio::sync::Notify>)>,
) -> (GatewayTransport, JoinHandle<Vec<(String, String)>>) {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
  let server = tokio::spawn(async move {
    let mut requests = Vec::new();
    for (index, response) in responses.into_iter().enumerate() {
      let (mut stream, _) = listener.accept().await.unwrap();
      let mut bytes = Vec::new();
      let (headers_end, length) = loop {
        let mut chunk = [0; 1024];
        let count = stream.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0);
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
          let headers = std::str::from_utf8(&bytes[..position]).unwrap();
          let length = headers
            .lines()
            .find_map(|line| {
              let (key, value) = line.split_once(':')?;
              key
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
          break (position + 4, length);
        }
      };
      while bytes.len() < headers_end + length {
        let mut chunk = [0; 1024];
        let count = stream.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0);
        bytes.extend_from_slice(&chunk[..count]);
      }
      let path = std::str::from_utf8(&bytes[..headers_end])
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned();
      let body = String::from_utf8(bytes[headers_end..headers_end + length].to_vec()).unwrap();
      requests.push((path, body));
      if index == 0
        && let Some((arrived, release)) = &gate
      {
        arrived.notify_one();
        release.notified().await;
      }
      stream.write_all(response.as_bytes()).await.unwrap();
    }
    requests
  });
  let transport = GatewayTransport {
    client: Client::builder()
      .redirect(Policy::none())
      .no_proxy()
      .timeout(REQUEST_TIMEOUT)
      .build()
      .unwrap(),
    origin,
    authentication: GatewayAuthentication::new(
      "authcookie=test-cookie&portal=portal&user=alice&domain=example&computer=test".into(),
      "non-tunnel".into(),
    )
    .unwrap(),
    profile: OsProfile::builder(crate::os_profile::ClientOs::Linux)
      .computer_name_override("test")
      .build(),
  };
  (transport, server)
}

pub(crate) fn response(body: &str) -> String {
  format!(
    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
    body.len()
  )
}

#[test]
fn checksum_matches_c_protocol_golden_fixture() {
  for volatile in [
    "authcookie=test&preferred-ip=192.0.2.1&preferred-ipv6=2001%3Adb8%3A%3A1",
    "authcookie=refreshed&preferred-ip=192.0.2.2&preferred-ipv6=2001%3Adb8%3A%3A2",
  ] {
    let cookie =
      format!("user=test&{volatile}&persistent-cookie=persistent%2Bcookie&portal=test&domain=test&computer=test");
    assert_eq!(hip_token(&cookie), "20f30054f721fd212925866b5778447d");
  }
  assert_ne!(hip_token("user=alice%20bob"), hip_token("user=alice+bob"));
  assert_ne!(hip_token("user=alice&portal=a"), hip_token("portal=a&user=alice"));
}

#[tokio::test]
async fn maintenance_uses_only_hip_and_logout_with_physical_identity() {
  let (transport, server) = gateway(vec![
    response("<response><hip-report-needed>yes</hip-report-needed><delay>7</delay></response>"),
    response("<response status='success'/>"),
    response("<response status='success'/>"),
  ])
  .await;
  let ipv4 = Some("192.0.2.7".parse().unwrap());
  let ipv6 = Some("2001:db8::7".parse().unwrap());
  let check = transport.check_hip(ipv4, ipv6).await.unwrap();
  assert_eq!(
    check,
    HipCheck {
      report_needed: true,
      delay: Duration::from_secs(7)
    }
  );
  transport.submit_hip("<hip-report/>", ipv4, ipv6).await.unwrap();
  transport.logout().await.unwrap();
  let requests = server.await.unwrap();
  assert_eq!(
    requests.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>(),
    [
      "/ssl-vpn/hipreportcheck.esp",
      "/ssl-vpn/hipreport.esp",
      "/ssl-vpn/logout.esp"
    ]
  );
  let form: std::collections::HashMap<String, String> = serde_urlencoded::from_str(&requests[0].1).unwrap();
  assert_eq!(form["authcookie"], "test-cookie");
  assert_eq!(form["client-ip"], "192.0.2.7");
  assert_eq!(form["client-ipv6"], "2001:db8::7");
  assert_eq!(form["md5"], transport.hip_token());
}

#[tokio::test]
async fn rejects_redirects_without_forwarding_authentication() {
  let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let (transport, server) = gateway(vec![format!(
    "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{}/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    destination.local_addr().unwrap()
  )])
  .await;
  assert!(
    matches!(transport.check_hip(None, None).await, Err(GatewayRequestError::Http(status)) if status.as_u16() == 307)
  );
  assert!(
    tokio::time::timeout(Duration::from_millis(100), destination.accept())
      .await
      .is_err()
  );
  assert_eq!(server.await.unwrap().len(), 1);
}

#[tokio::test]
async fn distinguishes_expired_cookie_from_transport_and_protocol_errors() {
  let (transport, server) = gateway(vec![
    response("<response status='error'><error>Invalid authentication cookie</error></response>"),
    "HTTP/1.1 512 Invalid Cookie\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
    "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
    response("<response><hip-report-needed>maybe</hip-report-needed></response>"),
    response("<response status='error'><error>server-private-payload</error></response>"),
  ])
  .await;
  for _ in 0..2 {
    assert!(matches!(
      transport.check_hip(None, None).await,
      Err(GatewayRequestError::InvalidCookie)
    ));
  }
  assert!(matches!(
    transport.check_hip(None, None).await.unwrap_err(),
    GatewayRequestError::Http(StatusCode::SERVICE_UNAVAILABLE)
  ));
  assert!(matches!(
    transport.check_hip(None, None).await,
    Err(GatewayRequestError::MalformedResponse)
  ));
  let error = transport.check_hip(None, None).await.unwrap_err();
  assert!(!format!("{error:?}").contains("server-private-payload"));
  server.await.unwrap();
}

#[tokio::test]
async fn bounds_response_size_before_reading_body() {
  let (transport, server) = gateway(vec![format!(
    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
    MAX_GATEWAY_RESPONSE_BYTES + 1
  )])
  .await;
  assert!(matches!(
    transport.check_hip(None, None).await,
    Err(GatewayRequestError::ResponseTooLarge)
  ));
  server.await.unwrap();
}

#[test]
fn rejects_invalid_delays_and_unsafe_response_xml() {
  for delay in ["-1", "3601", "4294967296", "invalid"] {
    let root = parse_response(
      format!("<response><hip-report-needed>no</hip-report-needed><delay>{delay}</delay></response>").as_bytes(),
    )
    .unwrap();
    assert!(parse_hip_check(&root).is_err());
  }
  assert!(parse_response(b"<!DOCTYPE response><response/>").is_err());
}

#[test]
fn recognizes_cookie_expiry_in_official_error_formats() {
  for body in [
    r#"var respStatus = "Error"; var respMsg = "Invalid authentication\u0020cookie"; thisForm.inputStr.value = "";"#,
    r#"<html><head/><body>var respStatus = "Error"; var respMsg = "Invalid authentication cookie";</body></html>"#,
    "<prelogin-response><status>Error</status><msg>Invalid authentication cookie</msg></prelogin-response>",
  ] {
    assert!(matches!(
      parse_response(body.as_bytes()),
      Err(GatewayRequestError::InvalidCookie)
    ));
  }
  assert!(matches!(
    parse_response(b"<response status='error'><error>Denied</error></response>"),
    Err(GatewayRequestError::Rejected)
  ));
}
