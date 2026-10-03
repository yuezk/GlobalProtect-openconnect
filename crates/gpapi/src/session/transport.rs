use std::{net::IpAddr, time::Duration};

use reqwest::{Client, StatusCode, Url, redirect::Policy};
use xmltree::Element;

use crate::{gp_params::GpParams, os_profile::OsProfile, utils::xml::ElementExt};

use super::{GatewayAuthentication, network::GatewayBinding};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const MAX_GATEWAY_RESPONSE_BYTES: usize = 1024 * 1024;
pub const MAX_HIP_REPORT_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum GatewayRequestError {
  #[error("Gateway authentication cookie expired")]
  InvalidCookie,
  #[error("Gateway request failed: {0}")]
  Transport(#[from] reqwest::Error),
  #[error("Gateway returned HTTP {0}")]
  Http(StatusCode),
  #[error("Gateway rejected the request")]
  Rejected,
  #[error("Gateway returned a malformed response")]
  MalformedResponse,
  #[error("Gateway response exceeds its size limit")]
  ResponseTooLarge,
  #[error("HIP report is empty, contains NUL, or exceeds its size limit")]
  InvalidReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HipCheck {
  pub report_needed: bool,
  pub delay: Duration,
}

/// Authenticated HTTP requests retain the login identity. Non-tunnel maintenance
/// uses a validated physical binding; pre-tunnel cleanup uses ordinary HTTP.
pub struct GatewayTransport {
  client: Client,
  origin: Url,
  authentication: GatewayAuthentication,
  profile: OsProfile,
}

impl GatewayTransport {
  pub fn new(
    server: &str,
    authentication: GatewayAuthentication,
    params: &GpParams,
    binding: Option<&GatewayBinding>,
  ) -> anyhow::Result<Self> {
    let origin = Url::parse(&crate::utils::normalize_server(server)?)?;
    let client = match binding {
      Some(binding) => bound_client(server, params, binding)?,
      None => Client::try_from(params)?,
    };
    Ok(Self {
      client,
      origin,
      authentication,
      profile: params.os_profile().clone(),
    })
  }

  /// The login client already validated and built this transport before issuing
  /// authentication. Registering its cleanup context cannot fail afterwards.
  pub(crate) fn from_authenticated_login(
    client: Client,
    origin: Url,
    authentication: GatewayAuthentication,
    profile: OsProfile,
  ) -> Self {
    Self {
      client,
      origin,
      authentication,
      profile,
    }
  }

  pub fn authentication(&self) -> &GatewayAuthentication {
    &self.authentication
  }

  pub fn hip_token(&self) -> String {
    hip_token(self.authentication.cookie())
  }

  pub async fn check_hip(&self, ipv4: Option<IpAddr>, ipv6: Option<IpAddr>) -> Result<HipCheck, GatewayRequestError> {
    let mut form = self.hip_form(ipv4, ipv6);
    form.push(("md5".into(), self.hip_token()));
    parse_hip_check(&self.post("hipreportcheck.esp", &form).await?)
  }

  pub async fn submit_hip(
    &self,
    report: &str,
    ipv4: Option<IpAddr>,
    ipv6: Option<IpAddr>,
  ) -> Result<(), GatewayRequestError> {
    if report.is_empty() || report.len() > MAX_HIP_REPORT_BYTES || report.contains('\0') {
      return Err(GatewayRequestError::InvalidReport);
    }
    let mut form = self.hip_form(ipv4, ipv6);
    form.push(("report".into(), report.into()));
    self.post("hipreport.esp", &form).await?;
    Ok(())
  }

  pub async fn logout(&self) -> Result<(), GatewayRequestError> {
    logout_cookie(&self.client, &self.origin, self.authentication.cookie(), &self.profile).await
  }

  fn cookie_form(&self) -> Vec<(String, String)> {
    url::form_urlencoded::parse(self.authentication.cookie().as_bytes())
      .into_owned()
      .collect()
  }

  fn hip_form(&self, ipv4: Option<IpAddr>, ipv6: Option<IpAddr>) -> Vec<(String, String)> {
    let mut form = self.cookie_form();
    set_form(&mut form, "client-role", "global-protect-full");
    set_form(&mut form, "computer", self.profile.computer());
    set_form(&mut form, "host-id", self.profile.host_id());
    set_form(&mut form, "serialno", self.profile.serialno());
    if let Some(ip) = ipv4 {
      set_form(&mut form, "client-ip", &ip.to_string());
    }
    if let Some(ip) = ipv6 {
      set_form(&mut form, "client-ipv6", &ip.to_string());
    }
    form
  }

  async fn post(&self, endpoint: &str, form: &[(String, String)]) -> Result<Element, GatewayRequestError> {
    post(&self.client, &self.origin, endpoint, form).await
  }
}

/// Cleanup uses trustworthy issued cookie fields even when execution mode is
/// invalid. It never creates a runnable authentication with a fabricated mode.
pub(crate) async fn logout_cookie(
  client: &Client,
  origin: &Url,
  cookie: &str,
  profile: &OsProfile,
) -> Result<(), GatewayRequestError> {
  let mut form = url::form_urlencoded::parse(cookie.as_bytes()).into_owned().collect();
  set_form(&mut form, "computer", profile.computer());
  set_form(&mut form, "clientos", profile.client_os().as_str());
  set_form(&mut form, "os-version", profile.os_version());
  post(client, origin, "logout.esp", &form).await?;
  Ok(())
}

async fn post(
  client: &Client,
  origin: &Url,
  endpoint: &str,
  form: &[(String, String)],
) -> Result<Element, GatewayRequestError> {
  let mut url = origin.clone();
  url.set_path(&format!("/ssl-vpn/{endpoint}"));
  let response = client.post(url).form(form).send().await?;
  let status = response.status();
  let invalid_cookie = status.as_u16() == 512
    || response
      .headers()
      .get("x-private-pan-globalprotect")
      .is_some_and(|value| value == "Invalid authentication cookie");
  if invalid_cookie {
    return Err(GatewayRequestError::InvalidCookie);
  }
  if !status.is_success() {
    return Err(GatewayRequestError::Http(status));
  }
  let body = crate::utils::request::read_bounded_response(response, MAX_GATEWAY_RESPONSE_BYTES)
    .await
    .map_err(|error| match error {
      crate::utils::request::ResponseBodyError::Transport(error) => GatewayRequestError::Transport(error),
      crate::utils::request::ResponseBodyError::TooLarge => GatewayRequestError::ResponseTooLarge,
    })?;
  parse_response(&body)
}

pub(crate) fn bound_client(server: &str, params: &GpParams, binding: &GatewayBinding) -> anyhow::Result<Client> {
  binding.validate_server(server)?;
  let origin = gateway_origin(server)?;
  let host = origin
    .host_str()
    .ok_or_else(|| anyhow::anyhow!("Gateway host is missing"))?;
  let builder = params
    .client_builder()?
    .redirect(Policy::none())
    .no_proxy()
    .local_address(binding.source().ip())
    .resolve(host, binding.endpoint())
    .timeout(REQUEST_TIMEOUT);
  let builder = bind_interface(builder, binding)?;
  Ok(builder.build()?)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn bind_interface(builder: reqwest::ClientBuilder, binding: &GatewayBinding) -> anyhow::Result<reqwest::ClientBuilder> {
  Ok(builder.interface(binding.interface()))
}

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
fn bind_interface(builder: reqwest::ClientBuilder, binding: &GatewayBinding) -> anyhow::Result<reqwest::ClientBuilder> {
  if let std::net::SocketAddr::V6(source) = binding.source() {
    anyhow::ensure!(
      source.scope_id() == 0,
      "Scoped IPv6 source requires a native socket connector"
    );
  }
  Ok(builder)
}

pub(crate) fn gateway_origin(server: &str) -> anyhow::Result<Url> {
  let server = if server.starts_with("https://") {
    server.to_owned()
  } else {
    format!("https://{server}")
  };
  let origin = Url::parse(&server)?;
  anyhow::ensure!(
    origin.scheme() == "https"
      && origin.username().is_empty()
      && origin.password().is_none()
      && origin.path() == "/"
      && origin.query().is_none()
      && origin.fragment().is_none(),
    "Gateway endpoint must be an HTTPS origin"
  );
  Ok(origin)
}

fn set_form(form: &mut Vec<(String, String)>, key: &str, value: &str) {
  form.retain(|(name, _)| name != key);
  form.push((key.into(), value.into()));
}

/// Match gpst.c::build_csd_token without decoding or reordering cookie fields.
pub fn hip_token(cookie: &str) -> String {
  let stable = cookie
    .split('&')
    .filter(|field| {
      let key = field.split('=').next().unwrap_or_default();
      !matches!(key, "authcookie" | "preferred-ip" | "preferred-ipv6")
    })
    .collect::<Vec<_>>()
    .join("&");
  format!("{:x}", md5::compute(stable.as_bytes()))
}

fn parse_response(body: &[u8]) -> Result<Element, GatewayRequestError> {
  let text = std::str::from_utf8(body).map_err(|_| GatewayRequestError::MalformedResponse)?;
  let lower = text.to_ascii_lowercase();
  if lower.contains("<!doctype") || lower.contains("<!entity") {
    return Err(GatewayRequestError::MalformedResponse);
  }
  if let Some(error) = javascript_error(text) {
    return Err(error);
  }
  let root = Element::parse(body).map_err(|_| GatewayRequestError::MalformedResponse)?;
  if root.name == "html" {
    return Err(
      root
        .descendant_text("body")
        .as_deref()
        .and_then(javascript_error)
        .unwrap_or(GatewayRequestError::MalformedResponse),
    );
  }
  if root.name == "prelogin-response" && root.child_text("status").as_deref() == Some("Error") {
    return Err(message_error(root.child_text("msg").as_deref()));
  }
  if root.name != "response" {
    return Err(GatewayRequestError::MalformedResponse);
  }
  match root.attr("status") {
    Some("error") => Err(message_error(root.child_text("error").as_deref())),
    None | Some("success") => Ok(root),
    _ => Err(GatewayRequestError::Rejected),
  }
}

fn message_error(message: Option<&str>) -> GatewayRequestError {
  if message == Some("Invalid authentication cookie") {
    GatewayRequestError::InvalidCookie
  } else {
    GatewayRequestError::Rejected
  }
}

fn javascript_error(text: &str) -> Option<GatewayRequestError> {
  static ERROR: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
  let regex = ERROR.get_or_init(|| {
    regex::Regex::new(r#"(?s)^\s*var\s+respStatus\s*=\s*"Error"\s*;\s*var\s+respMsg\s*=\s*("(?:\\.|[^"\\])*")"#)
      .expect("valid GlobalProtect error pattern")
  });
  let captures = regex.captures(text)?;
  match serde_json::from_str::<String>(&captures[1]) {
    Ok(message) => Some(message_error(Some(&message))),
    Err(_) => Some(GatewayRequestError::MalformedResponse),
  }
}

fn parse_hip_check(root: &Element) -> Result<HipCheck, GatewayRequestError> {
  let report_needed = match root.child_text("hip-report-needed").as_deref() {
    Some("yes") => true,
    Some("no") => false,
    _ => return Err(GatewayRequestError::MalformedResponse),
  };
  let delay = match root.child_text("delay") {
    Some(value) => value
      .parse::<u32>()
      .map_err(|_| GatewayRequestError::MalformedResponse)?,
    None => 0,
  };
  // Limit this client's supported response delay to one hour. The official
  // registry scheduling cap is not evidence of a wire-protocol maximum.
  if delay > 3600 {
    return Err(GatewayRequestError::MalformedResponse);
  }
  Ok(HipCheck {
    report_needed,
    delay: Duration::from_secs(u64::from(delay)),
  })
}

#[cfg(test)]
pub(crate) mod tests;
