use std::collections::HashMap;

use log::{debug, info, warn};
use reqwest::Client;
use xmltree::Element;

use crate::{
  error::PortalError,
  gp_params::{ClientOs, GpParams},
  service::{
    request::{ConnectArgs, ConnectRequest},
    session::{SessionInfo, SessionWarning},
  },
  utils::{normalize_server, parse_gp_response, xml::ElementExt},
};

pub async fn retrieve_session_info(req: &ConnectRequest) -> anyhow::Result<SessionInfo> {
  retrieve_session_info_for_gateway(req.gateway().server(), req.args()).await
}

pub async fn retrieve_session_info_for_gateway(server: &str, args: &ConnectArgs) -> anyhow::Result<SessionInfo> {
  let base_url = normalize_server(server)?;
  let client = build_session_client(args)?;
  let form = build_session_form(args)?;
  let url = format!("{base_url}/ssl-vpn/getconfig.esp");

  info!("Retrieving gateway session info");

  let response = client.post(&url).form(&form).send().await.map_err(|err| {
    warn!("Network error: {:?}", err);
    anyhow::anyhow!(PortalError::NetworkError(err))
  })?;

  let response = parse_gp_response(response)
    .await
    .map_err(|err| anyhow::anyhow!(err.reason))?;
  let response = response.trim();
  if !response.starts_with('<') {
    return Err(anyhow::anyhow!("Gateway session info error: {response}"));
  }

  let root = Element::parse(response.as_bytes())?;
  let session_info = parse_session_info(&root)?;

  debug!("Parsed session info: {:?}", session_info);

  Ok(session_info)
}

pub fn build_session_client(args: &ConnectArgs) -> anyhow::Result<Client> {
  let mut builder = GpParams::builder();
  builder.user_agent(args.user_agent().as_deref().unwrap_or_default());
  builder.client_os(args.os().unwrap_or(ClientOs::default()));
  builder.os_version(args.os_version());
  builder.client_version(args.client_version());
  builder.certificate(args.certificate());
  builder.sslkey(args.sslkey());
  builder.key_password(args.key_password());

  Client::try_from(&builder.build())
}

pub fn build_session_form(args: &ConnectArgs) -> anyhow::Result<HashMap<String, String>> {
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
      args.os().unwrap_or(ClientOs::default()).as_str().to_string(),
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

  Ok(SessionInfo {
    lifetime_secs: parse_optional_u32(root, "lifetime")?,
    user_expires,
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn builds_gateway_session_form() {
    let gateway = crate::gateway::Gateway::new("vpn".to_string(), "vpn.example.com".to_string());
    let req = ConnectRequest::new(
      crate::service::vpn_state::ConnectInfo::new("vpn.example.com".to_string(), gateway.clone(), vec![gateway]),
      "authcookie=AUTH&portal=vpn.example.com&user=alice&preferred-ip=10.0.0.10".to_string(),
    )
    .with_os(ClientOs::Mac)
    .with_os_version(Some("macOS 15.0".to_string()))
    .with_client_version("6.3.1-12")
    .with_disable_ipv6(true);

    let form = build_session_form(req.args()).unwrap();

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
    let gateway = crate::gateway::Gateway::new("vpn".to_string(), "vpn.example.com".to_string());
    let req = ConnectRequest::new(
      crate::service::vpn_state::ConnectInfo::new("vpn.example.com".to_string(), gateway.clone(), vec![gateway]),
      "authcookie=AUTH".to_string(),
    )
    .with_client_version("6.3.1-12");

    let form = build_session_form(req.args()).unwrap();

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
}
