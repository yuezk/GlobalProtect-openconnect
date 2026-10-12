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

#[tokio::test]
async fn rendering_recovery_fetches_new_prelogin_metadata_and_preserves_gateway_mode() {
  let (origin, worker) = gateway(vec![
    (
      "200 OK",
      "<prelogin-response><status>Success</status><region>old</region></prelogin-response>".into(),
    ),
    (
      "200 OK",
      "<prelogin-response><status>Success</status><region>replacement</region></prelogin-response>".into(),
    ),
  ])
  .await;
  let args = TestArgs::parse_from(["test", &origin]).connect;
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
  let original = handler.refresh_auth_prelogin(&origin, true, true).await.unwrap();
  let replacement = handler
    .refresh_auth_prelogin(&origin, original.is_gateway(), true)
    .await
    .unwrap();
  assert_eq!(original.region(), "old");
  assert_eq!(replacement.region(), "replacement");
  assert!(replacement.is_gateway());
  let requests = worker.await.unwrap();
  assert_eq!(requests.len(), 2);
  assert!(
    requests
      .iter()
      .all(|request| request.starts_with("POST /ssl-vpn/prelogin.esp"))
  );
}

#[cfg(feature = "webview-auth")]
#[tokio::test]
async fn cancellation_during_recovery_prelogin_stops_request() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let origin = format!("http://{}", listener.local_addr().unwrap());
  let args = TestArgs::parse_from(["test", &origin]).connect;
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
  handler
    .auth_attempt_fixture
    .borrow_mut()
    .outcomes
    .push_back(Err(rendering_crash()));
  let (result, ()) = tokio::time::timeout(Duration::from_secs(3), async {
    tokio::join!(
      async {
        let original = handler.refresh_auth_prelogin(&origin, false, false).await?;
        handler.obtain_credential_with_recovery(original, &origin, false).await
      },
      async {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut byte = [0];
        socket.read_exact(&mut byte).await.unwrap();
        handler.cancellation.cancel();
      }
    )
  })
  .await
  .unwrap();
  assert!(
    result
      .unwrap_err()
      .downcast_ref::<gpapi::auth::AuthenticationCancelled>()
      .is_some()
  );
}

#[cfg(feature = "webview-auth")]
fn rendering_crash() -> anyhow::Error {
  use std::os::unix::process::ExitStatusExt;
  gpapi::process::rendering_recovery::RenderingCrash {
    status: std::process::ExitStatus::from_raw(71 << 8),
  }
  .into()
}

#[cfg(feature = "webview-auth")]
fn saml_prelogin_response(region: &str, encoded_request: &str) -> String {
  format!(
    "<prelogin-response><status>Success</status><region>{region}</region><saml-auth-method>POST</saml-auth-method><saml-request>{encoded_request}</saml-request></prelogin-response>"
  )
}

#[cfg(feature = "webview-auth")]
#[tokio::test]
async fn auth_crash_uses_fresh_saml_and_clean_once_with_shared_budget() {
  use gpapi::credential::{Credential, PreloginCredential};
  let (origin, worker) = gateway(vec![
    ("200 OK", saml_prelogin_response("old-region", "b2xk")),
    ("200 OK", saml_prelogin_response("new-region", "bmV3")),
    ("200 OK", saml_prelogin_response("gateway-region", "Z2F0ZXdheQ==")),
  ])
  .await;
  let args = TestArgs::parse_from(["test", &origin, "--clean"]).connect;
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
  handler.auth_attempt_fixture.borrow_mut().outcomes.extend([
    Err(rendering_crash()),
    Ok(Credential::Prelogin(PreloginCredential::new(
      "alice",
      Some("new-cookie"),
      None,
      None,
    ))),
    Err(rendering_crash()),
  ]);
  let original = handler.refresh_auth_prelogin(&origin, false, false).await.unwrap();
  let (replacement, credential) = handler
    .obtain_credential_with_recovery(original, &origin, false)
    .await
    .unwrap();
  assert_eq!(replacement.region(), "new-region");
  assert_eq!(credential.username(), "alice");
  assert!(matches!(credential, Credential::Prelogin(ref value) if value.prelogin_cookie() == Some("new-cookie")));
  let gateway_prelogin = handler.refresh_auth_prelogin(&origin, true, false).await.unwrap();
  let error = handler
    .obtain_credential_with_recovery(gateway_prelogin, &origin, false)
    .await
    .unwrap_err();
  assert!(
    error
      .downcast_ref::<gpapi::process::rendering_recovery::RecoveryFailed>()
      .is_some()
  );
  assert_eq!(
    handler.auth_attempt_fixture.borrow().observed,
    [
      ("old".into(), false, true),
      ("new".into(), true, false),
      ("gateway".into(), true, false),
    ]
  );
  assert_eq!(worker.await.unwrap().len(), 3);
}

#[cfg(feature = "webview-auth")]
#[tokio::test]
async fn exhausted_auth_recovery_is_terminal_before_portal_gateway_fallback() {
  let (origin, worker) = gateway(vec![
    ("200 OK", saml_prelogin_response("old-region", "b2xk")),
    ("200 OK", saml_prelogin_response("new-region", "bmV3")),
  ])
  .await;
  let args = TestArgs::parse_from(["test", &origin, "--clean"]).connect;
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
  handler.auth_attempt_fixture.borrow_mut().outcomes.extend([
    Err(rendering_crash()),
    Err(anyhow::anyhow!(gpapi::error::PortalError::PreloginError(
      "second failure".into()
    ))),
  ]);
  let error = handler.handle_impl().await.unwrap_err();
  assert!(
    error
      .downcast_ref::<gpapi::process::rendering_recovery::RecoveryFailed>()
      .is_some()
  );
  assert_eq!(handler.auth_attempt_fixture.borrow().observed.len(), 2);
  assert_eq!(worker.await.unwrap().len(), 2);
}

#[cfg(all(
  feature = "webview-auth",
  any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")
))]
mod gtk_retry {
  use super::*;

  #[tokio::test]
  async fn real_gtk_crash_retries_bundled_authenticator() {
    // Environment overrides belong to a separate test process, never the parallel test runner.
    if let Some(directory) = std::env::var_os("GP_TEST_GTK_AUTH_DIR") {
      verify_real_authenticator_retry(std::path::Path::new(&directory)).await;
    } else {
      run_real_authenticator_retry(false);
    }
  }

  #[cfg(target_os = "linux")]
  #[test]
  #[ignore = "requires non-interactive sudo authorization and a running Wayland desktop session"]
  fn sudo_gtk_crash_retries_after_restoring_desktop_environment() {
    run_real_authenticator_retry(true);
  }

  fn run_real_authenticator_retry(sudo: bool) {
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};

    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("gpauth");
    // Fixtures contain only dummy data and must remain accessible after a root test drops privileges.
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let attempts_path = directory.path().join("attempts");
    fs::write(&attempts_path, "").unwrap();
    fs::set_permissions(&attempts_path, fs::Permissions::from_mode(0o666)).unwrap();
    fs::write(&executable, r#"#!/bin/sh
set -eu
request=
while [ "$#" -gt 0 ]; do
  if [ "$1" = '--saml-request' ]; then
    shift
    request=$1
  fi
  shift
done
printf '%s|%s|%s|%s\n' "$(id -u)" "${WAYLAND_DISPLAY:-}" "${WEBKIT_DISABLE_DMABUF_RENDERER-unset}" "$request" >> "$GP_TEST_GTK_AUTH_DIR/attempts"
if [ "$request" = 'old' ]; then
  printf 'Gdk-Message: 10:49:22.617: Error 71 (Protocol error) dispatching to Wayland display.\n' >&2
  exit 1
fi
[ "$request" = 'new' ] && [ "${WEBKIT_DISABLE_DMABUF_RENDERER:-}" = '1' ] || exit 2
printf '%s\n' '{"success":{"username":"alice","preloginCookie":"new-cookie"}}'
"#).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();

    let mut command = if sudo {
      assert_ne!(
        uzers::get_effective_uid(),
        0,
        "run this test as the Wayland desktop user"
      );
      let mut command = Command::new("sudo");
      command.args([
        "-n",
        "env",
        "-u",
        "WAYLAND_DISPLAY",
        "-u",
        "DISPLAY",
        "-u",
        "XDG_SESSION_TYPE",
        "-u",
        "WEBKIT_DISABLE_DMABUF_RENDERER",
      ]);
      command.arg("GP_TEST_EXPECT_ROOT=1");
      command
    } else {
      let mut command = Command::new("env");
      command.arg("-u").arg("WEBKIT_DISABLE_DMABUF_RENDERER");
      command.arg("WAYLAND_DISPLAY=wayland-test");
      command
    };
    command.arg(format!("GP_AUTH_BINARY={}", executable.display()));
    command.arg(format!("GP_TEST_GTK_AUTH_DIR={}", directory.path().display()));
    command.arg(std::env::current_exe().unwrap()).args([
      "--exact",
      "connect::flow_tests::gtk_retry::real_gtk_crash_retries_bundled_authenticator",
      "--nocapture",
    ]);
    let output = command.output().unwrap();
    assert!(
      output.status.success(),
      "retry subprocess failed: {}\n{}",
      String::from_utf8_lossy(&output.stdout),
      String::from_utf8_lossy(&output.stderr)
    );
    let expected_uid = gpapi::process::users::get_non_root_user().unwrap().uid().to_string();
    let attempts = fs::read_to_string(attempts_path).unwrap();
    let attempts: Vec<Vec<&str>> = attempts.lines().map(|line| line.split('|').collect()).collect();
    assert_eq!(attempts.len(), 2, "expected exactly one retry");
    for attempt in &attempts {
      assert_eq!(attempt[0], expected_uid, "authenticator must run as the desktop user");
      assert!(!attempt[1].is_empty(), "authenticator must receive WAYLAND_DISPLAY");
    }
    assert_eq!(&attempts[0][2..], ["unset", "old"]);
    assert_eq!(&attempts[1][2..], ["1", "new"]);
  }

  async fn verify_real_authenticator_retry(directory: &std::path::Path) {
    use gpapi::credential::Credential;

    if std::env::var_os("GP_TEST_EXPECT_ROOT").is_some() {
      assert_eq!(uzers::get_effective_uid(), 0);
      assert!(std::env::var_os("SUDO_UID").is_some());
      assert!(std::env::var_os("WAYLAND_DISPLAY").is_none());
    }
    let (origin, worker) = gateway(vec![
      ("200 OK", saml_prelogin_response("old-region", "b2xk")),
      ("200 OK", saml_prelogin_response("new-region", "bmV3")),
    ])
    .await;
    let args = TestArgs::parse_from(["test", &origin]).connect;
    let lock = directory.join("client.pid");
    let verbose = gpapi::clap::InfoLevelVerbosity::new(0, 0);
    let shared = SharedArgs {
      fix_openssl: false,
      ignore_tls_errors: true,
      lock_file: &lock,
      verbose: &verbose,
      log_format: LogFormat::Text,
    };
    let handler = ConnectHandler::new(&args, &shared);
    let original = handler.refresh_auth_prelogin(&origin, false, false).await.unwrap();
    let (replacement, credential) = handler
      .obtain_credential_with_recovery(original, &origin, false)
      .await
      .unwrap();
    assert_eq!(replacement.region(), "new-region");
    assert!(matches!(credential, Credential::Prelogin(ref value) if value.prelogin_cookie() == Some("new-cookie")));
    assert_eq!(
      worker.await.unwrap().len(),
      2,
      "recovery must fetch fresh prelogin data"
    );
  }
}
