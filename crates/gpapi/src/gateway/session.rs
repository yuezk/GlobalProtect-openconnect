use std::{
  collections::HashMap,
  time::{SystemTime, UNIX_EPOCH},
};

use anyhow::bail;
use log::{info, warn};
use reqwest::Client;
use xmltree::Element;

use crate::{
  error::PortalError,
  gp_params::GpParams,
  service::{
    session::{SessionInfo, SessionRequestArgs, SessionWarning},
  },
  utils::{normalize_server, parse_gp_response, xml::ElementExt},
};

const EXTEND_SESSION_MESSAGE: &str = "User Session Extension";
const EXTEND_SESSION_COMMENT: &str = "User extends the login lifetime";

#[derive(Debug, Clone)]
pub struct SessionContext {
  server: String,
  portal: String,
  session_args: SessionRequestArgs,
}

impl SessionContext {
  pub fn new(server: String, portal: String, session_args: SessionRequestArgs) -> Self {
    Self {
      server,
      portal,
      session_args,
    }
  }

  pub fn server(&self) -> &str {
    &self.server
  }

  pub fn portal(&self) -> &str {
    &self.portal
  }

  pub fn session_args(&self) -> &SessionRequestArgs {
    &self.session_args
  }
}

pub async fn retrieve_session_info_for_gateway(server: &str, args: &SessionRequestArgs) -> anyhow::Result<SessionInfo> {
  let base_url = normalize_server(server)?;
  let client = build_session_client(args)?;
  let form = build_session_form(args)?;
  let url = format!("{base_url}/ssl-vpn/getconfig.esp");

  info!("Retrieving gateway session info");

  let response = client.post(&url).form(&form).send().await.map_err(|err| {
    warn!("Network error: {:?}", err);
    anyhow::anyhow!(PortalError::NetworkError(err))
  })?;

  let response = parse_gp_response(response).await?;
  let response = response.trim();
  if !response.starts_with('<') {
    return Err(anyhow::anyhow!("Gateway session info error: {response}"));
  }

  let root = Element::parse(response.as_bytes())?;
  let session_info = parse_session_info(&root)?;
  info!("Parsed session info: {:?}", session_info);

  Ok(session_info)
}

pub async fn extend_session(ctx: &SessionContext) -> anyhow::Result<SessionInfo> {
  let base_url = normalize_server(ctx.server())?;
  let client = build_session_client(ctx.session_args())?;
  let form = build_extend_session_form(ctx)?;
  let url = format!("{base_url}/ssl-vpn/agentmessage.esp");

  info!("Sending extend-session request");

  let response = client.post(&url).form(&form).send().await?;
  let response = parse_gp_response(response).await?;
  let root = Element::parse(response.as_bytes())?;
  let status = parse_extend_session_status(&root, &response)?;

  info!("Extend-session gateway response status: {}", status);

  match status.as_str() {
    "success" => {
      let session_info = retrieve_session_info_for_gateway(ctx.server(), ctx.session_args()).await?;
      info!("Extend-session refreshed session info: {:?}", session_info);
      Ok(session_info)
    }
    status => bail!("Extend session rejected: {status}"),
  }
}

fn build_session_client(args: &SessionRequestArgs) -> anyhow::Result<Client> {
  let mut builder = GpParams::builder();
  builder.user_agent(args.user_agent().as_deref().unwrap_or_default());
  builder.client_os(args.os().unwrap_or_default());
  builder.os_version(args.os_version());
  builder.client_version(args.client_version());
  builder.certificate(args.certificate());
  builder.sslkey(args.sslkey());
  builder.key_password(args.key_password());

  Client::try_from(&builder.build())
}

fn build_session_form(args: &SessionRequestArgs) -> anyhow::Result<HashMap<String, String>> {
  let mut form = HashMap::from([
    ("client-type".to_string(), "1".to_string()),
    ("protocol-version".to_string(), "p1".to_string()),
    ("internal".to_string(), "no".to_string()),
    (
      "app-version".to_string(),
      args.client_version().unwrap_or_else(|| "6.3.0-33".to_string()),
    ),
    (
      "ipv6-support".to_string(),
      if args.disable_ipv6() { "no" } else { "yes" }.to_string(),
    ),
    (
      "clientos".to_string(),
      args.os().unwrap_or_default().as_str().to_string(),
    ),
    ("hmac-algo".to_string(), "sha1,md5,sha256".to_string()),
    ("enc-algo".to_string(), "aes-128-cbc,aes-256-cbc".to_string()),
  ]);

  if let Some(os_version) = args.os_version() {
    form.insert("os-version".to_string(), os_version);
  }

  form.extend(serde_urlencoded::from_str::<HashMap<String, String>>(args.cookie())?);

  Ok(form)
}

fn parse_session_info(root: &Element) -> anyhow::Result<SessionInfo> {
  let user_expires = parse_optional_u32(root, "user-expires")?.or(parse_optional_u32(root, "user_expires")?);
  let lifetime_secs = parse_optional_u32(root, "lifetime")?;

  Ok(SessionInfo {
    lifetime_secs,
    user_expires,
    expires_in_human: build_human_readable_expiry(user_expires, lifetime_secs),
    lifetime_warning: parse_warning(root, "lifetime-notify-prior", "lifetime-notify-message")?,
    inactivity_warning: parse_warning(root, "inactivity-notify-prior", "inactivity-notify-message")?,
    admin_logout_message: root.descendant_text("admin-logout-notify-message"),
    allow_extend_session: parse_bool_flag(root.descendant_text("allow-extend-session").as_deref()),
  })
}

fn parse_warning(root: &Element, prior_tag: &str, message_tag: &str) -> anyhow::Result<Option<SessionWarning>> {
  let prior_secs = parse_optional_u32(root, prior_tag)?;
  let message = root.descendant_text(message_tag).filter(|value| !value.is_empty());

  Ok(match (prior_secs, message) {
    (Some(prior_secs), Some(message)) => Some(SessionWarning { prior_secs, message }),
    _ => None,
  })
}

fn parse_optional_u32(root: &Element, tag: &str) -> anyhow::Result<Option<u32>> {
  let Some(value) = root.descendant_text(tag) else {
    return Ok(None);
  };
  let value = value.trim();
  if value.is_empty() {
    return Ok(None);
  }

  Ok(Some(value.parse()?))
}

fn parse_bool_flag(value: Option<&str>) -> bool {
  matches!(value, Some("yes" | "true" | "1"))
}

fn build_human_readable_expiry(user_expires: Option<u32>, lifetime_secs: Option<u32>) -> Option<String> {
  if let Some(user_expires) = user_expires {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    return Some(format_human_duration((user_expires as u64).saturating_sub(now)));
  }

  lifetime_secs.map(|secs| format_human_duration(secs as u64))
}

fn format_human_duration(total_secs: u64) -> String {
  let days = total_secs / 86_400;
  let hours = (total_secs % 86_400) / 3_600;
  let minutes = (total_secs % 3_600) / 60;
  let seconds = total_secs % 60;

  let mut parts = Vec::new();
  if days > 0 {
    parts.push(format!("{days}days"));
  }
  if hours > 0 {
    parts.push(format!("{hours}h"));
  }
  if minutes > 0 {
    parts.push(format!("{minutes}m"));
  }
  if seconds > 0 || parts.is_empty() {
    parts.push(format!("{seconds}s"));
  }

  parts.join(" ")
}

fn parse_cookie_form(args: &SessionRequestArgs) -> anyhow::Result<HashMap<String, String>> {
  Ok(serde_urlencoded::from_str(args.cookie())?)
}

fn build_extend_session_form(ctx: &SessionContext) -> anyhow::Result<HashMap<String, String>> {
  let mut form = parse_cookie_form(ctx.session_args())?;
  form.insert("portal".to_string(), ctx.portal().to_string());
  form.insert("timestamp".to_string(), unix_timestamp().to_string());
  form.insert("message".to_string(), EXTEND_SESSION_MESSAGE.to_string());
  form.insert("comment".to_string(), EXTEND_SESSION_COMMENT.to_string());

  Ok(form)
}

fn parse_extend_session_status(root: &Element, response: &str) -> anyhow::Result<String> {
  match root.attr("status") {
    Some(status) => Ok(status.to_string()),
    None => {
      warn!("Malformed extend-session response body: {}", response.trim());
      bail!("Malformed extend session response")
    }
  }
}

fn unix_timestamp() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs()
}

#[cfg(test)]
mod tests {
  use crate::{
    gp_params::ClientOs,
    service::session::SessionRequestArgs,
  };

  use super::*;

  fn build_session_args() -> SessionRequestArgs {
    SessionRequestArgs::new("authcookie=AUTH&portal=vpn.example.com&user=alice&preferred-ip=10.0.0.10".to_string())
      .with_os(Some(ClientOs::Mac))
      .with_os_version(Some("macOS 15.0".to_string()))
      .with_client_version(Some("6.3.1-12".to_string()))
      .with_disable_ipv6(true)
  }

  #[test]
  fn builds_gateway_session_form() {
    let form = build_session_form(&build_session_args()).unwrap();

    assert_eq!(form.get("client-type").map(String::as_str), Some("1"));
    assert_eq!(form.get("protocol-version").map(String::as_str), Some("p1"));
    assert_eq!(form.get("internal").map(String::as_str), Some("no"));
    assert_eq!(form.get("app-version").map(String::as_str), Some("6.3.1-12"));
    assert_eq!(form.get("ipv6-support").map(String::as_str), Some("no"));
    assert_eq!(form.get("clientos").map(String::as_str), Some("Mac"));
    assert_eq!(form.get("os-version").map(String::as_str), Some("macOS 15.0"));
    assert_eq!(form.get("authcookie").map(String::as_str), Some("AUTH"));
    assert_eq!(form.get("portal").map(String::as_str), Some("vpn.example.com"));
    assert_eq!(form.get("user").map(String::as_str), Some("alice"));
    assert_eq!(form.get("preferred-ip").map(String::as_str), Some("10.0.0.10"));
  }

  #[test]
  fn builds_gateway_session_form_with_ipv6_enabled() {
    let args = SessionRequestArgs::new("authcookie=AUTH".to_string()).with_client_version(Some("6.3.1-12".to_string()));

    let form = build_session_form(&args).unwrap();

    assert_eq!(form.get("ipv6-support").map(String::as_str), Some("yes"));
  }

  #[test]
  fn parses_session_info() {
    let xml = r#"
      <response>
        <lifetime>43200</lifetime>
        <user-expires>1776828409</user-expires>
        <lifetime-notify-prior>1800</lifetime-notify-prior>
        <lifetime-notify-message>The VPN will disconnect after 12 hours.</lifetime-notify-message>
        <inactivity-notify-prior>300</inactivity-notify-prior>
        <inactivity-notify-message>The VPN will disconnect after 5 minutes of inactivity.</inactivity-notify-message>
        <admin-logout-notify-message>Logged out by administrator.</admin-logout-notify-message>
        <allow-extend-session>yes</allow-extend-session>
      </response>
    "#;
    let root = Element::parse(xml.as_bytes()).unwrap();

    let info = parse_session_info(&root).unwrap();

    assert_eq!(info.lifetime_secs, Some(43200));
    assert_eq!(info.user_expires, Some(1776828409));
    assert!(info.expires_in_human.as_ref().is_some_and(|value| !value.is_empty()));
    assert_eq!(info.lifetime_warning.unwrap().prior_secs, 1800);
    assert_eq!(info.inactivity_warning.unwrap().prior_secs, 300);
    assert_eq!(
      info.admin_logout_message.as_deref(),
      Some("Logged out by administrator.")
    );
    assert!(info.allow_extend_session);
  }

  #[test]
  fn parses_allow_extend_session_false() {
    let xml = r#"<response><allow-extend-session>no</allow-extend-session></response>"#;
    let root = Element::parse(xml.as_bytes()).unwrap();

    let info = parse_session_info(&root).unwrap();

    assert!(!info.allow_extend_session);
  }

  #[test]
  fn parses_user_expires_underscore_variant() {
    let xml = r#"
      <response>
        <user_expires>1779810767</user_expires>
      </response>
    "#;
    let root = Element::parse(xml.as_bytes()).unwrap();

    let info = parse_session_info(&root).unwrap();

    assert_eq!(info.user_expires, Some(1779810767));
  }

  #[test]
  fn active_session_context_preserves_connect_args_for_refresh() {
    let session_args = build_session_args();
    let ctx = SessionContext::new(
      "vpn.example.com".to_string(),
      "portal.example.com".to_string(),
      session_args,
    );

    let form = build_session_form(ctx.session_args()).unwrap();

    assert_eq!(form.get("ipv6-support").map(String::as_str), Some("no"));
    assert_eq!(form.get("clientos").map(String::as_str), Some("Mac"));
    assert_eq!(form.get("os-version").map(String::as_str), Some("macOS 15.0"));
  }

  #[test]
  fn builds_extend_session_form() {
    let session_args = build_session_args();
    let ctx = SessionContext::new(
      "vpn.example.com".to_string(),
      "portal.example.com".to_string(),
      session_args,
    );

    let form = build_extend_session_form(&ctx).unwrap();

    assert_eq!(form.get("authcookie").map(String::as_str), Some("AUTH"));
    assert_eq!(form.get("portal").map(String::as_str), Some("portal.example.com"));
    assert_eq!(form.get("message").map(String::as_str), Some(EXTEND_SESSION_MESSAGE));
    assert_eq!(form.get("comment").map(String::as_str), Some(EXTEND_SESSION_COMMENT));
    assert!(form.contains_key("timestamp"));
  }

  #[test]
  fn parses_extend_session_status_attribute() {
    let root = Element::parse(r#"<response status="success"></response>"#.as_bytes()).unwrap();

    assert_eq!(
      parse_extend_session_status(&root, r#"<response status="success"></response>"#).unwrap(),
      "success"
    );
  }

  #[test]
  fn rejects_extend_session_response_without_status() {
    let root = Element::parse("<response><result>ok</result></response>".as_bytes()).unwrap();

    assert_eq!(
      parse_extend_session_status(&root, "<response><result>ok</result></response>")
        .unwrap_err()
        .to_string(),
      "Malformed extend session response"
    );
  }
}
