use std::time::Duration;

use clap::Parser;
use gpapi::{
  cookie_store::{self, StoredCookie},
  credential::AuthCookieCredential,
  gateway::{Gateway, GatewayLoginContext, GatewaySelection},
  log_format::LogFormat,
  session::SessionMode,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{ConnectArgs, ConnectHandler};
use crate::cli::SharedArgs;

#[derive(Parser)]
struct TestArgs {
  #[command(flatten)]
  connect: ConnectArgs,
}

fn login_response() -> String {
  let mut fields = [""; 20];
  fields[1] = "issued-cookie";
  fields[3] = "gateway";
  fields[4] = "alice";
  fields[5] = "fixture-auth";
  fields[6] = "vsys1";
  fields[12] = "tunnel";
  format!(
    "<jnlp><application-desc>{}</application-desc></jnlp>",
    fields
      .iter()
      .map(|field| format!("<argument>{field}</argument>"))
      .collect::<String>()
  )
}

async fn gateway(responses: Vec<(&'static str, String)>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let origin = format!("http://{}", listener.local_addr().unwrap());
  let worker = tokio::spawn(async move {
    tokio::time::timeout(Duration::from_secs(5), async move {
      let mut requests = Vec::new();
      for (status, body) in responses {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
          let mut buf = [0; 4096];
          let count = stream.read(&mut buf).await.unwrap();
          assert_ne!(count, 0, "Request ended before its body");
          request.extend_from_slice(&buf[..count]);
          if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
            let length = headers
              .lines()
              .find_map(|line| line.strip_prefix("content-length:"))
              .map(|value| value.trim().parse::<usize>().unwrap())
              .unwrap_or(0);
            if request.len() >= end + 4 + length {
              break;
            }
          }
        }
        requests.push(String::from_utf8(request).unwrap());
        stream
          .write_all(
            format!(
              "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
              body.len()
            )
            .as_bytes(),
          )
          .await
          .unwrap();
      }
      requests
    })
    .await
    .expect("Gateway fixture timed out")
  });
  (origin, worker)
}

#[tokio::test]
async fn cached_tunnel_authentication_does_not_require_portal_discovery() {
  let (gateway, worker) = gateway(vec![("200 OK", login_response())]).await;
  let directory = tempfile::tempdir().unwrap();
  let cache = directory.path().join("cookie.json");
  let args = TestArgs::parse_from([
    "test",
    "http://unavailable-portal.invalid",
    "--cookie-only",
    "--cookie-cache",
    cache.to_str().unwrap(),
  ])
  .connect;
  let lock = directory.path().join("client.pid");
  let verbose = gpapi::clap::InfoLevelVerbosity::new(0, 0);
  let shared = SharedArgs {
    fix_openssl: false,
    ignore_tls_errors: true,
    lock_file: &lock,
    verbose: &verbose,
    log_format: LogFormat::Text,
  };
  let handler = ConnectHandler::new(&args, &shared);
  cookie_store::save(
    &cache,
    &StoredCookie::new(
      args.server.clone(),
      "alice".into(),
      handler.os_profile.borrow().host_identity().host_id().into(),
      gateway,
      AuthCookieCredential::new("alice", "portal-cookie", ""),
    ),
  )
  .unwrap();
  handler.handle_impl().await.unwrap();
  let requests = worker.await.unwrap();
  assert_eq!(requests.len(), 1);
  assert!(requests[0].starts_with("POST /ssl-vpn/login.esp "));
  assert!(requests[0].contains("portal-cookie"));
  assert_eq!(
    handler.sessions.borrow().members()[0].transport.authentication().mode(),
    SessionMode::Tunnel
  );
}

#[tokio::test]
async fn internal_tunnel_keeps_gateway_authentication_after_portal_cookie_rejection() {
  let (origin, worker) = gateway(vec![
    (
      "200 OK",
      "<prelogin-response><status>Success</status></prelogin-response>".into(),
    ),
    ("403 Forbidden", "<response><error>rejected</error></response>".into()),
    ("200 OK", login_response()),
  ])
  .await;
  let args = TestArgs::parse_from(["test", &origin, "--user", "alice", "--passwd-on-stdin"]).connect;
  let directory = tempfile::tempdir().unwrap();
  let lock = directory.path().join("client.pid");
  let verbose = gpapi::clap::InfoLevelVerbosity::new(0, 0);
  let shared = SharedArgs {
    fix_openssl: false,
    ignore_tls_errors: true,
    lock_file: &lock,
    verbose: &verbose,
    log_format: LogFormat::Text,
  };
  let handler = ConnectHandler::new(&args, &shared);
  handler.password_from_stdin.replace(Some("gateway-password".into()));
  let mut value = serde_json::to_value(Gateway::new("internal".into(), origin.clone())).unwrap();
  value["kind"] = serde_json::json!("internal");
  let gateway: Gateway = serde_json::from_value(value).unwrap();
  let session = handler
    .authenticate_gateway(
      &origin,
      &origin,
      &AuthCookieCredential::new("alice", "rejected-cookie", ""),
      false,
      GatewayLoginContext::new(&gateway, GatewaySelection::Auto),
    )
    .await
    .unwrap();
  assert_eq!(session.authentication.mode(), SessionMode::Tunnel);
  assert!(session.extension_auth.binding().is_none());
  let requests = worker.await.unwrap();
  assert_eq!(requests.len(), 3);
  assert!(requests[1].contains("rejected-cookie"));
  assert!(requests[2].contains("gateway-password"));
}
