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
  let base_url = normalize_server(req.gateway().server())?;
  let client = build_client(req.args())?;
  let form = parse_cookie_form(req.args().cookie())?;
  let url = format!("{base_url}/ssl-vpn/getconfig.esp");

  info!("Retrieving gateway session info");

  let response = client.post(&url).form(&form).send().await.map_err(|err| {
    warn!("Network error: {:?}", err);
    anyhow::anyhow!(PortalError::NetworkError(err))
  })?;

  let response = parse_gp_response(response).await.map_err(|err| anyhow::anyhow!(err.reason))?;
  let root = Element::parse(response.as_bytes())?;
  let session_info = parse_session_info(&root)?;

  debug!("Parsed session info: {:?}", session_info);

  Ok(session_info)
}

fn build_client(args: &ConnectArgs) -> anyhow::Result<Client> {
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

fn parse_cookie_form(cookie: &str) -> anyhow::Result<HashMap<String, String>> {
  Ok(serde_urlencoded::from_str(cookie)?)
}

fn parse_session_info(root: &Element) -> anyhow::Result<SessionInfo> {
  Ok(SessionInfo {
    lifetime_secs: parse_optional_u64(root, "lifetime")?,
    user_expires: parse_optional_u64(root, "user-expires")
      .or_else(|_| parse_optional_u64(root, "user_expires"))?,
    lifetime_warning: parse_warning(root, "lifetime-notify-prior", "lifetime-notify-message")?,
    inactivity_warning: parse_warning(root, "inactivity-notify-prior", "inactivity-notify-message")?,
    admin_logout_message: root.descendant_text("admin-logout-notify-message"),
    allow_extend_session: parse_bool_flag(root.descendant_text("allow-extend-session").as_deref()),
  })
}

fn parse_warning(root: &Element, prior_tag: &str, message_tag: &str) -> anyhow::Result<Option<SessionWarning>> {
  let prior_secs = parse_optional_u64(root, prior_tag)?;
  let message = root.descendant_text(message_tag).filter(|value| !value.is_empty());

  Ok(match (prior_secs, message) {
    (Some(prior_secs), Some(message)) => Some(SessionWarning { prior_secs, message }),
    _ => None,
  })
}

fn parse_optional_u64(root: &Element, tag: &str) -> anyhow::Result<Option<u64>> {
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
    assert_eq!(info.admin_logout_message.as_deref(), Some("Logged out by administrator."));
    assert!(info.allow_extend_session);
  }

  #[test]
  fn parses_allow_extend_session_false() {
    let xml = r#"<response><allow-extend-session>no</allow-extend-session></response>"#;
    let root = Element::parse(xml.as_bytes()).unwrap();

    let info = parse_session_info(&root).unwrap();

    assert!(!info.allow_extend_session);
  }
}
