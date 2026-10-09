use std::{
  borrow::Cow,
  net::{SocketAddr, UdpSocket},
};

use anyhow::bail;
use log::{debug, info, warn};
use reqwest::Client;
use tokio_util::sync::CancellationToken;
use urlencoding::{decode, encode};
use xmltree::Element;
use zeroize::Zeroizing;

use crate::{
  credential::Credential,
  error::PortalError,
  gateway::GatewayLoginContext,
  gp_params::GpParams,
  params::gateway_login::{self, GatewayLoginInput},
  session::{
    GatewayAuthentication,
    network::GatewayBinding,
    transport::{bound_client, gateway_origin},
  },
  utils::{normalize_server, parse_gp_response, remove_url_scheme, xml::ElementExt},
};

pub enum GatewayLogin {
  Authenticated(GatewayAuthentication),
  Mfa(String, String),
}

/// Protocol failures cannot be repaired by repeating login with different
/// credentials. Issued credentials, when identifiable, are cleaned up first.
#[derive(Debug, thiserror::Error)]
#[error("Gateway login returned invalid session configuration")]
pub struct GatewayLoginProtocolError;

#[derive(Clone)]
pub struct GatewayLoginClient {
  origin: reqwest::Url,
  client: Client,
  params: GpParams,
  binding: Option<GatewayBinding>,
  login_endpoint: Option<SocketAddr>,
  login_url: Option<reqwest::Url>,
  maintenance_client: Option<Client>,
}

impl GatewayLoginClient {
  pub fn register_session(
    &self,
    gateway: crate::gateway::Gateway,
    authentication: GatewayAuthentication,
    sessions: &mut crate::session::GatewaySessions,
  ) {
    sessions.register(crate::session::GatewaySession {
      gateway,
      transport: self.authenticated_transport(authentication),
      addresses: self
        .binding
        .as_ref()
        .map(|binding| binding.addresses())
        .unwrap_or_default(),
    });
  }
  pub fn new(gateway: &str, params: GpParams) -> anyhow::Result<Self> {
    let origin = reqwest::Url::parse(&normalize_server(gateway)?)?;
    let client = Client::try_from(&params)?;
    Ok(Self {
      origin,
      client,
      params,
      binding: None,
      login_endpoint: None,
      login_url: None,
      maintenance_client: None,
    })
  }

  /// Only called after an authenticated non-tunnel response and registration in
  /// the caller's cleanup ledger. Tunnel login never inspects physical routing.
  pub async fn bind_non_tunnel(
    &mut self,
    disable_ipv6: bool,
    detection: Option<&crate::session::network::InternalHostDetection>,
    cancellation: &tokio_util::sync::CancellationToken,
  ) -> anyhow::Result<()> {
    let origin = gateway_origin(self.origin.as_str())?;
    let response_url = self
      .login_url
      .as_ref()
      .ok_or_else(|| anyhow::anyhow!("Authenticated response endpoint is unavailable"))?;
    anyhow::ensure!(
      response_url.origin() == origin.origin(),
      "Redirected non-tunnel authentication is unsupported"
    );
    let endpoint = self
      .login_endpoint
      .ok_or_else(|| anyhow::anyhow!("Non-tunnel authentication has no direct peer address"))?;
    let network = crate::session::network::PhysicalNetwork::capture_controlled(cancellation).await?;
    let binding = network
      .bind_authenticated_endpoint(origin.as_str(), endpoint, disable_ipv6, cancellation)
      .await?;
    network
      .validate_internal_binding(&binding, detection, cancellation)
      .await?;
    let client = bound_client(origin.as_str(), &self.params, &binding)?;
    self.binding = Some(binding);
    self.maintenance_client = Some(client);
    Ok(())
  }

  pub fn update_registered_session(
    &self,
    gateway: &crate::gateway::Gateway,
    authentication: &GatewayAuthentication,
    sessions: &mut crate::session::GatewaySessions,
  ) {
    sessions.update(crate::session::GatewaySession {
      gateway: gateway.clone(),
      transport: self.authenticated_transport(authentication.clone()),
      addresses: self
        .binding
        .as_ref()
        .map(|binding| binding.addresses())
        .unwrap_or_default(),
    });
  }

  pub fn authenticated_transport(
    &self,
    authentication: GatewayAuthentication,
  ) -> crate::session::transport::GatewayTransport {
    crate::session::transport::GatewayTransport::from_authenticated_login(
      self.maintenance_client.as_ref().unwrap_or(&self.client).clone(),
      self.origin.clone(),
      authentication,
      self.params.os_profile().clone(),
    )
  }

  pub fn binding(&self) -> Option<&GatewayBinding> {
    self.binding.as_ref()
  }
  pub fn client_identity(&self) -> Option<&crate::utils::request::ClientIdentity> {
    self.params.client_identity()
  }
  pub(crate) fn request_client(&self) -> &Client {
    &self.client
  }
  pub(crate) fn origin(&self) -> &reqwest::Url {
    &self.origin
  }

  pub fn respond_mfa(&mut self, input: &str, otp: &str) {
    self.params.set_input_str(input);
    self.params.set_otp(otp);
  }

  pub async fn login(
    &mut self,
    cred: &Credential,
    context: Option<&GatewayLoginContext>,
    cancellation: &CancellationToken,
  ) -> anyhow::Result<GatewayLogin> {
    if cancellation.is_cancelled() {
      return Err(crate::auth::AuthenticationCancelled.into());
    }
    let client_ip = context.and_then(|context| {
      context
        .client_ip()
        .map(str::to_owned)
        .or_else(|| detect_local_ipv4(context.host()))
    });
    let request = receive_gateway_login(
      &self.origin,
      &self.client,
      cred,
      &self.params,
      context,
      client_ip.as_deref(),
      false,
    );
    let response = tokio::select! {
      biased;
      response = request => response?,
      _ = cancellation.cancelled() => return Err(crate::auth::AuthenticationCancelled.into()),
    };
    // Once the full response is available, retain issued authentication or finish
    // cleanup-only logout before the caller observes cancellation.
    let response = parse_login_response(response, &self.client, &self.origin, &self.params).await?;
    self.login_endpoint = response.endpoint;
    self.login_url = Some(response.url);
    Ok(response.login)
  }

  pub(crate) async fn extend_lifetime(&self, cred: &Credential) -> anyhow::Result<GatewayLogin> {
    let response = receive_gateway_login(&self.origin, &self.client, cred, &self.params, None, None, true).await?;
    Ok(
      parse_login_response(response, &self.client, &self.origin, &self.params)
        .await?
        .login,
    )
  }
}

struct LoginResponse {
  login: GatewayLogin,
  endpoint: Option<SocketAddr>,
  url: reqwest::Url,
}

struct ReceivedLogin {
  body: String,
  endpoint: Option<SocketAddr>,
  url: reqwest::Url,
}

fn detect_local_ipv4(host: &str) -> Option<String> {
  let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
  socket.connect((host, 443)).ok()?;
  let ip = socket.local_addr().ok()?.ip();
  if ip.is_ipv4() { Some(ip.to_string()) } else { None }
}

async fn receive_gateway_login(
  origin: &reqwest::Url,
  client: &Client,
  cred: &Credential,
  gp_params: &GpParams,
  context: Option<&GatewayLoginContext>,
  client_ip: Option<&str>,
  extend_lifetime: bool,
) -> anyhow::Result<ReceivedLogin> {
  let gateway = remove_url_scheme(origin.as_str().trim_end_matches('/'));
  let login_url = format!("{}/ssl-vpn/login.esp", origin.as_str().trim_end_matches('/'));
  let request_params = gateway_login::build(&GatewayLoginInput {
    gp_params,
    cred,
    gateway_host: &gateway,
    context,
    client_ip,
    extend_lifetime,
  });

  info!("Perform gateway login, user_agent: {}", gp_params.user_agent());
  log_gateway_login_context(context, client_ip, gp_params);

  let res = client
    .post(login_url)
    .form(&request_params.body)
    .send()
    .await
    .map_err(|e| {
      warn!("Network error: {:?}", e);
      anyhow::anyhow!(PortalError::NetworkError(e))
    })?;

  let endpoint = res.remote_addr();
  let url = res.url().clone();
  let res = parse_gp_response(res).await.map_err(|err| {
    warn!("Gateway login response failed: {}", err.reason);
    anyhow::anyhow!("Gateway login error: {}", err.reason)
  })?;

  Ok(ReceivedLogin {
    body: res,
    endpoint,
    url,
  })
}

async fn parse_login_response(
  response: ReceivedLogin,
  client: &Client,
  origin: &reqwest::Url,
  gp_params: &GpParams,
) -> anyhow::Result<LoginResponse> {
  let ReceivedLogin {
    body: res,
    endpoint,
    url,
  } = response;

  // It's possible to get an empty response, log the response headers for debugging
  if res.trim().is_empty() {
    info!("Empty gateway login response headers: {:?}", res);
    bail!("Got empty gateway login response");
  }

  // MFA detected
  if res.contains("Challenge") {
    let Some((message, input_str)) = parse_mfa(&res) else {
      bail!("Failed to parse MFA challenge");
    };

    return Ok(LoginResponse {
      login: GatewayLogin::Mfa(message, input_str),
      endpoint,
      url,
    });
  }

  let lower = res.to_ascii_lowercase();
  if lower.contains("<!doctype") || lower.contains("<!entity") {
    return Err(GatewayLoginProtocolError.into());
  }
  let root = Element::parse(res.as_bytes()).map_err(|_| GatewayLoginProtocolError)?;
  let (cookie, connection_type) =
    parse_issued_authentication(&root, gp_params.computer()).map_err(|_| GatewayLoginProtocolError)?;
  let cookie = Zeroizing::new(cookie);
  let authentication = connection_type.and_then(|mode| GatewayAuthentication::new(cookie.to_string(), mode));
  let authentication = match authentication {
    Ok(authentication) => authentication,
    Err(_) => {
      match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        crate::session::transport::logout_cookie(client, origin, &cookie, gp_params.os_profile()),
      )
      .await
      {
        Ok(Ok(())) => {}
        Ok(Err(error)) => warn!("Cleanup-only gateway logout failed: {error}"),
        Err(_) => warn!("Cleanup-only gateway logout timed out"),
      }
      return Err(GatewayLoginProtocolError.into());
    }
  };
  debug!("Gateway login succeeded, mode: {:?}", authentication.mode());
  Ok(LoginResponse {
    login: GatewayLogin::Authenticated(authentication),
    endpoint,
    url,
  })
}

fn log_gateway_login_context(context: Option<&GatewayLoginContext>, client_ip: Option<&str>, gp_params: &GpParams) {
  let Some(context) = context else {
    return;
  };

  info!(
    "Gateway login context: host_present={}, gateway_name_present={}, connect_method_present={}, selection_type={}, internal={}, clientgpversion_present={}, client_ip_present={}",
    !context.host().is_empty(),
    !context.name().is_empty(),
    context.connect_method().is_some(),
    context.selection().as_login_param(),
    context.kind().as_login_param(),
    gp_params.client_version().is_some(),
    client_ip.is_some()
  );
}

fn parse_issued_authentication(element: &Element, computer: &str) -> anyhow::Result<(String, anyhow::Result<String>)> {
  anyhow::ensure!(element.name == "jnlp", "Gateway login response is not JNLP");
  anyhow::ensure!(
    element.attr("status").is_none_or(|status| status == "success"),
    "Gateway login response rejected authentication"
  );
  let application = element
    .child("application-desc")
    .ok_or_else(|| anyhow::anyhow!("Gateway login response has no application description"))?;
  let args = application
    .children("argument")
    .iter()
    .map(|e| e.get_text().unwrap_or_default())
    .collect::<Vec<_>>();
  let cookie = build_gateway_token(&args, computer)?;
  let connection_type = read_arg_value(&args, 12)
    .and_then(|mode| mode.ok_or_else(|| anyhow::anyhow!("Gateway login response has no connection type")));
  Ok((cookie, connection_type))
}

#[cfg(test)]
fn parse_gateway_authentication(element: &Element, computer: &str) -> anyhow::Result<GatewayAuthentication> {
  let (cookie, mode) = parse_issued_authentication(element, computer)?;
  GatewayAuthentication::new(cookie, mode?)
}

fn build_gateway_token(args: &[Cow<'_, str>], computer: &str) -> anyhow::Result<String> {
  let mut params = vec![
    read_required_arg(&args, 1, "authcookie")?,
    read_optional_arg(&args, 2, "persistent-cookie")?,
    read_optional_arg(&args, 3, "portal")?,
    read_required_arg(&args, 4, "user")?,
    read_optional_arg(&args, 7, "domain")?,
    read_optional_arg(&args, 15, "preferred-ip")?,
    read_optional_arg(&args, 18, "preferred-ipv6")?,
  ]
  .into_iter()
  .flatten()
  .collect::<Vec<_>>();
  params.push(("computer", computer.to_string()));

  let token = params
    .iter()
    .map(|(k, v)| format!("{}={}", k, encode(v)))
    .collect::<Vec<_>>()
    .join("&");

  Ok(token)
}

fn read_required_arg(
  args: &[Cow<'_, str>],
  index: usize,
  key: &'static str,
) -> anyhow::Result<Option<(&'static str, String)>> {
  let Some(value) = read_arg_value(args, index)? else {
    bail!("Failed to read {key} from args");
  };

  Ok(Some((key, value)))
}

fn read_optional_arg(
  args: &[Cow<'_, str>],
  index: usize,
  key: &'static str,
) -> anyhow::Result<Option<(&'static str, String)>> {
  Ok(read_arg_value(args, index)?.map(|value| (key, value)))
}

fn read_arg_value(args: &[Cow<'_, str>], index: usize) -> anyhow::Result<Option<String>> {
  Ok(
    args
      .get(index)
      .map(|s| normalize_arg_value(s.as_ref()))
      .transpose()
      .map_err(|err| anyhow::anyhow!("Failed to decode gateway login argument {index}: {err}"))?
      .flatten(),
  )
}

fn normalize_arg_value(value: &str) -> anyhow::Result<Option<String>> {
  let value = decode(value)?;
  if value.is_empty() || value == "(null)" || value == "-1" {
    return Ok(None);
  }
  anyhow::ensure!(
    !value.chars().any(char::is_control),
    "Gateway argument contains control characters"
  );
  Ok(Some(value.into_owned()))
}

fn parse_mfa(res: &str) -> Option<(String, String)> {
  let message = res
    .lines()
    .find(|l| l.contains("respMsg"))
    .and_then(|l| l.split('"').nth(1).map(|s| s.to_string()))?;

  let input_str = res
    .lines()
    .find(|l| l.contains("inputStr"))
    .and_then(|l| l.split('"').nth(1).map(|s| s.to_string()))?;

  Some((message, input_str))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn tunnel_response() -> String {
    let root = login_response(Some("tunnel"), "issued-cookie");
    let mut xml = Vec::new();
    root.write(&mut xml).unwrap();
    crate::session::transport::tests::response(std::str::from_utf8(&xml).unwrap())
  }

  fn test_params() -> GpParams {
    GpParams::builder(crate::os_profile::OsProfile::builder(crate::os_profile::ClientOs::Linux).build()).build()
  }

  #[tokio::test]
  async fn gateway_login_cancels_stalled_headers_and_body() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for partial_response in ["", "HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n<jnlp>"] {
      let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
      let origin = format!("http://{}", listener.local_addr().unwrap());
      let (ready, received) = tokio::sync::oneshot::channel();
      let (release, released) = tokio::sync::oneshot::channel();
      let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        assert!(socket.read(&mut buffer).await.unwrap() > 0);
        socket.write_all(partial_response.as_bytes()).await.unwrap();
        ready.send(()).unwrap();
        released.await.unwrap();
      });
      let cancellation = CancellationToken::new();
      let token = cancellation.clone();
      let login = tokio::spawn(async move {
        let mut client = GatewayLoginClient::new(&origin, test_params()).unwrap();
        let credential = crate::credential::PasswordCredential::new("alice", "password").into();
        client.login(&credential, None, &token).await
      });
      received.await.unwrap();
      cancellation.cancel();
      let result = tokio::time::timeout(std::time::Duration::from_secs(1), login)
        .await
        .unwrap()
        .unwrap();
      assert!(result.err().unwrap().is::<crate::auth::AuthenticationCancelled>());
      release.send(()).unwrap();
      server.await.unwrap();
    }
  }

  #[tokio::test]
  async fn cancellation_during_cleanup_only_logout_awaits_cleanup() {
    use crate::session::transport::tests::response;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let mut xml = Vec::new();
    login_response(None, "issued-cookie").write(&mut xml).unwrap();
    let login_response = response(std::str::from_utf8(&xml).unwrap());
    let (ready, received) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
      let (mut socket, _) = listener.accept().await.unwrap();
      let mut buffer = [0; 4096];
      assert!(socket.read(&mut buffer).await.unwrap() > 0);
      socket.write_all(login_response.as_bytes()).await.unwrap();
      drop(socket);
      let (mut socket, _) = listener.accept().await.unwrap();
      let read = socket.read(&mut buffer).await.unwrap();
      assert!(
        std::str::from_utf8(&buffer[..read])
          .unwrap()
          .contains("/ssl-vpn/logout.esp")
      );
      ready.send(()).unwrap();
      released.await.unwrap();
      socket
        .write_all(response("<response status='success'/>").as_bytes())
        .await
        .unwrap();
    });
    let cancellation = CancellationToken::new();
    let token = cancellation.clone();
    let mut login = tokio::spawn(async move {
      let mut client = GatewayLoginClient::new(&origin, test_params()).unwrap();
      let credential = crate::credential::PasswordCredential::new("alice", "password").into();
      client.login(&credential, None, &token).await
    });
    received.await.unwrap();
    cancellation.cancel();
    assert!(
      tokio::time::timeout(std::time::Duration::from_millis(20), &mut login)
        .await
        .is_err()
    );
    release.send(()).unwrap();
    assert!(login.await.unwrap().err().unwrap().is::<GatewayLoginProtocolError>());
    server.await.unwrap();
  }

  #[tokio::test]
  async fn ordinary_tunnel_login_follows_redirects_without_physical_binding() {
    let (_, origin, server) = crate::session::transport::tests::login_gateway(vec![
      "HTTP/1.1 307 Temporary Redirect\r\nLocation: /redirected-login\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
      tunnel_response(),
      "HTTP/1.1 307 Temporary Redirect\r\nLocation: /redirected-login\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
      tunnel_response(),
    ]).await;
    let mut client = GatewayLoginClient::new(origin.as_str(), test_params()).unwrap();
    let credential = crate::credential::PasswordCredential::new("alice", "password").into();
    assert!(
      matches!(client.login(&credential, None, &CancellationToken::new()).await.unwrap(), GatewayLogin::Authenticated(authentication) if authentication.mode() == crate::session::SessionMode::Tunnel)
    );
    assert!(client.binding().is_none());
    assert!(matches!(
      client.extend_lifetime(&credential).await.unwrap(),
      GatewayLogin::Authenticated(_)
    ));
    let requests = server.await.unwrap();
    assert_eq!(requests[0].0, "/ssl-vpn/login.esp");
    assert_eq!(requests[1].0, "/redirected-login");
    assert_eq!(requests[3].0, "/redirected-login");
    assert!(requests[3].1.contains("extend-lifetime=true"));
  }

  #[tokio::test]
  async fn ordinary_login_keeps_reqwest_address_fallback() {
    let (_, origin, server) = crate::session::transport::tests::login_gateway(vec![tunnel_response()]).await;
    let port = origin.port().unwrap();
    let params = test_params();
    let mut client = GatewayLoginClient::new(&format!("http://gateway.invalid:{port}"), params.clone()).unwrap();
    // Supply deterministic DNS candidates; all other client behavior is the
    // ordinary request path. The first address refuses, the second is reachable.
    client.client = params
      .client_builder()
      .unwrap()
      .no_proxy()
      .resolve_to_addrs(
        "gateway.invalid",
        &[
          SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port)),
          SocketAddr::from(([127, 0, 0, 1], port)),
        ],
      )
      .build()
      .unwrap();
    let credential = crate::credential::PasswordCredential::new("alice", "password").into();
    assert!(matches!(
      client
        .login(&credential, None, &CancellationToken::new())
        .await
        .unwrap(),
      GatewayLogin::Authenticated(_)
    ));
    assert!(client.binding().is_none());
    assert_eq!(server.await.unwrap().len(), 1);
  }

  #[tokio::test]
  async fn ordinary_tunnel_login_can_use_a_proxy_without_local_gateway_resolution() {
    let (_, proxy, server) = crate::session::transport::tests::login_gateway(vec![tunnel_response()]).await;
    let params = test_params();
    let mut client = GatewayLoginClient::new("http://unresolvable.invalid", params.clone()).unwrap();
    client.client = params
      .client_builder()
      .unwrap()
      .proxy(reqwest::Proxy::all(proxy.as_str()).unwrap())
      .build()
      .unwrap();
    let credential = crate::credential::PasswordCredential::new("alice", "password").into();
    assert!(matches!(
      client
        .login(&credential, None, &CancellationToken::new())
        .await
        .unwrap(),
      GatewayLogin::Authenticated(_)
    ));
    assert!(client.binding().is_none());
    assert_eq!(
      server.await.unwrap()[0].0,
      "http://unresolvable.invalid/ssl-vpn/login.esp"
    );
  }

  #[tokio::test]
  async fn invalid_mode_logs_out_issued_cookie_before_returning_protocol_error() {
    use crate::session::transport::tests::{login_gateway, response};
    let root = login_response(None, "issued-cookie");
    let mut xml = Vec::new();
    root.write(&mut xml).unwrap();
    let (client, origin, server) = login_gateway(vec![
      response(std::str::from_utf8(&xml).unwrap()),
      response("<response status='success'/>"),
    ])
    .await;
    let params =
      GpParams::builder(crate::os_profile::OsProfile::builder(crate::os_profile::ClientOs::Linux).build()).build();
    let credential = crate::credential::PasswordCredential::new("alice", "password").into();
    let mut client = GatewayLoginClient {
      client,
      ..GatewayLoginClient::new(origin.as_str(), params).unwrap()
    };
    let error = client
      .login(&credential, None, &CancellationToken::new())
      .await
      .err()
      .unwrap();
    assert!(error.is::<GatewayLoginProtocolError>());
    let requests = server.await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].0, "/ssl-vpn/login.esp");
    assert_eq!(requests[1].0, "/ssl-vpn/logout.esp");
    let form: std::collections::HashMap<String, String> = serde_urlencoded::from_str(&requests[1].1).unwrap();
    assert_eq!(form.get("authcookie").map(String::as_str), Some("issued-cookie"));
    assert_eq!(form.get("user").map(String::as_str), Some("alice"));
  }

  #[test]
  fn cleanup_only_parsing_does_not_salvage_server_error_authentication() {
    let mut root = login_response(None, "issued-cookie");
    root.attributes.insert("status".into(), "error".into());
    assert!(parse_issued_authentication(&root, "computer").is_err());
    for encoded in ["%28null%29", "%2D1", "%00"] {
      assert!(parse_issued_authentication(&login_response(Some("tunnel"), encoded), "computer").is_err());
    }
  }

  fn login_response(mode: Option<&str>, cookie: &str) -> Element {
    let mut args = vec![""; 19];
    args[1] = cookie;
    args[4] = "alice";
    if let Some(mode) = mode {
      args[12] = mode;
    }
    let arguments = args
      .iter()
      .map(|value| format!("<argument>{value}</argument>"))
      .collect::<String>();
    Element::parse(format!("<jnlp><application-desc>{arguments}</application-desc></jnlp>").as_bytes()).unwrap()
  }

  #[test]
  fn login_mode_is_independent_of_network_location() {
    for (value, mode) in [
      ("tunnel", crate::session::SessionMode::Tunnel),
      ("non-tunnel", crate::session::SessionMode::NonTunnel),
      ("internal", crate::session::SessionMode::NonTunnel),
    ] {
      let authentication =
        parse_gateway_authentication(&login_response(Some(value), "test-cookie"), "computer").unwrap();
      assert_eq!(authentication.mode(), mode);
      assert_eq!(authentication.connection_type(), value);
      assert!(!format!("{authentication:?}").contains("test-cookie"));
    }
  }

  #[test]
  fn login_rejects_missing_mode_or_cookie() {
    for mode in [None, Some(""), Some("(null)"), Some("-1"), Some("   ")] {
      assert!(parse_gateway_authentication(&login_response(mode, "test-cookie"), "computer").is_err());
    }
    assert!(parse_gateway_authentication(&login_response(Some("tunnel"), ""), "computer").is_err());
    let response = Element::parse(b"<response status='error'/>".as_slice()).unwrap();
    assert!(parse_gateway_authentication(&response, "computer").is_err());
  }

  #[test]
  fn mfa() {
    let res = r#"var respStatus = "Challenge";
var respMsg = "MFA message";
thisForm.inputStr.value = "5ef64e83000119ed";"#;

    let (message, input_str) = parse_mfa(res).unwrap();
    assert_eq!(message, "MFA message");
    assert_eq!(input_str, "5ef64e83000119ed");
  }

  #[test]
  fn gateway_token_keeps_upstream_cookie_fields() {
    let res = r#"
<jnlp>
  <application-desc>
    <argument></argument>
    <argument>AUTHCOOKIE</argument>
    <argument>PERSISTENTCOOKIE</argument>
    <argument>vpn.example.com</argument>
    <argument>alice</argument>
    <argument>LDAP-auth</argument>
    <argument>vsys1</argument>
    <argument>%28empty_domain%29</argument>
    <argument></argument>
    <argument></argument>
    <argument></argument>
    <argument></argument>
    <argument>tunnel</argument>
    <argument>-1</argument>
    <argument>4100</argument>
    <argument>10.0.0.10</argument>
    <argument>unused-user-cookie</argument>
    <argument>unused-prelogon-cookie</argument>
    <argument>2001:db8::10</argument>
  </application-desc>
</jnlp>
"#;

    let root = Element::parse(res.as_bytes()).unwrap();
    let authentication = parse_gateway_authentication(&root, "metalklesk").unwrap();
    let token = authentication.cookie();
    assert_eq!(authentication.mode(), crate::session::SessionMode::Tunnel);

    assert_eq!(
      token,
      "authcookie=AUTHCOOKIE&persistent-cookie=PERSISTENTCOOKIE&portal=vpn.example.com&user=alice&domain=%28empty_domain%29&preferred-ip=10.0.0.10&preferred-ipv6=2001%3Adb8%3A%3A10&computer=metalklesk"
    );
  }

  #[test]
  fn gateway_token_omits_empty_optional_fields() {
    let res = r#"
<jnlp>
  <application-desc>
    <argument></argument>
    <argument>AUTHCOOKIE</argument>
    <argument></argument>
    <argument>vpn.example.com</argument>
    <argument>alice</argument>
    <argument>LDAP-auth</argument>
    <argument>vsys1</argument>
    <argument>-1</argument>
    <argument></argument>
    <argument></argument>
    <argument></argument>
    <argument></argument>
    <argument>tunnel</argument>
    <argument>-1</argument>
    <argument>4100</argument>
    <argument></argument>
    <argument>unused-user-cookie</argument>
    <argument>unused-prelogon-cookie</argument>
    <argument>(null)</argument>
  </application-desc>
</jnlp>
"#;

    let root = Element::parse(res.as_bytes()).unwrap();
    let authentication = parse_gateway_authentication(&root, "metalklesk").unwrap();
    let token = authentication.cookie();

    assert_eq!(
      token,
      "authcookie=AUTHCOOKIE&portal=vpn.example.com&user=alice&computer=metalklesk"
    );
  }
}
