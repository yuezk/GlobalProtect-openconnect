use std::sync::Mutex;

use gpapi::utils::parse_gp_response;
use log::{LevelFilter, Log, Metadata, Record};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::TcpListener,
};

struct Capture(Mutex<Vec<String>>);
static LOGS: Capture = Capture(Mutex::new(Vec::new()));
impl Log for Capture {
  fn enabled(&self, _: &Metadata<'_>) -> bool {
    true
  }
  fn log(&self, record: &Record<'_>) {
    if record.target() == "gpapi::utils::response_log" {
      self.0.lock().unwrap().push(record.args().to_string());
    }
  }
  fn flush(&self) {}
}

async fn response(path: &str, status: &str, body: &str) -> reqwest::Response {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let data = format!(
    "HTTP/1.1 {status}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
    body.len()
  );
  tokio::spawn(async move {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut request = [0; 4096];
    socket.read(&mut request).await.unwrap();
    socket.write_all(data.as_bytes()).await.unwrap();
  });
  reqwest::Client::builder()
    .no_proxy()
    .build()
    .unwrap()
    .get(format!("http://{address}{path}"))
    .send()
    .await
    .unwrap()
}

#[tokio::test]
async fn logs_redacted_xml_at_debug_without_changing_responses_or_exposing_error_bodies() {
  rustls::crypto::ring::default_provider().install_default().unwrap();
  log::set_logger(&LOGS).unwrap();
  log::set_max_level(LevelFilter::Debug);
  let portal = include_str!("fixtures/portal-response.xml");
  for path in [
    "/global-protect/getconfig.esp",
    "/global-protect/getconfig_csc.esp",
    "/global-protect/prelogin.esp",
    "/ssl-vpn/login.esp",
  ] {
    let result = parse_gp_response(response(path, "200 OK", portal).await).await.unwrap();
    assert_eq!(result, portal);
  }
  let logs = LOGS.0.lock().unwrap().join("\n");
  assert!(logs.contains("getconfig.esp response XML"));
  assert!(logs.contains("getconfig_csc.esp response XML"));
  assert!(logs.contains("prelogin.esp response XML"));
  assert!(logs.contains("login.esp response XML"));
  assert!(logs.contains("custom-checks") && logs.contains("HipID"));
  for secret in [
    "fixture-user-cookie",
    "fixture-prelogon-cookie",
    "fixture-csc-cookie",
    "fixture-username",
    "fixture-saml",
    "fixture-attribute-token",
    "fixture-property-password",
    "fixture-cookie-name",
    "fixture-query-token",
    "fixture-url-password",
    "fixture-comment",
    "fixture-pi",
  ] {
    assert!(!logs.contains(secret), "logged secret: {secret}");
  }

  let error = parse_gp_response(response("/global-protect/getconfig.esp", "403 Forbidden", portal).await)
    .await
    .unwrap_err();
  assert_eq!(error.status, reqwest::StatusCode::FORBIDDEN);
  assert!(!format!("{error:?} {error}").contains("fixture-"));
  assert!(LOGS.0.lock().unwrap().iter().any(|line| line.contains("HTTP 403")));

  let error = parse_gp_response(
    response(
      "/global-protect/getconfig.esp",
      "500 Internal Server Error",
      "<html><body>fixture-error-secret</body></html>",
    )
    .await,
  )
  .await
  .unwrap_err();
  assert!(!format!("{error:?} {error}").contains("fixture-error-secret"));
  assert!(!LOGS.0.lock().unwrap().join("\n").contains("fixture-error-secret"));

  LOGS.0.lock().unwrap().clear();
  let body = "<jnlp><application-desc><argument>fixture-login-cookie</argument></application-desc></jnlp>";
  parse_gp_response(response("/redirect/fixture-path-token?token=fixture-query-token", "200 OK", body).await)
    .await
    .unwrap();
  let logs = LOGS.0.lock().unwrap().join("\n");
  assert!(logs.contains("[response endpoint]"));
  assert!(!logs.contains("fixture-"));

  LOGS.0.lock().unwrap().clear();
  log::set_max_level(LevelFilter::Info);
  assert_eq!(
    parse_gp_response(response("/global-protect/getconfig.esp", "200 OK", portal).await)
      .await
      .unwrap(),
    portal
  );
  assert!(LOGS.0.lock().unwrap().is_empty());
}
