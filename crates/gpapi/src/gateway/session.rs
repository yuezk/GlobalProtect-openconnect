use std::{
  collections::HashMap,
  time::{SystemTime, UNIX_EPOCH},
};

use anyhow::bail;
use log::{info, warn};
use xmltree::Element;

use crate::{
  credential::Credential,
  gateway::login::{GatewayLogin, GatewayLoginClient},
  session::SessionRequestArgs,
  utils::{parse_gp_response, xml::ElementExt},
};

const EXTEND_SESSION_MESSAGE: &str = "User Session Extension";
const EXTEND_SESSION_COMMENT: &str = "User extends the login lifetime";

#[derive(Clone)]
pub struct SessionExtensionAuth {
  credential: Credential,
  client: GatewayLoginClient,
}

impl SessionExtensionAuth {
  pub fn new(credential: Credential, client: GatewayLoginClient) -> Self {
    Self { credential, client }
  }
  pub fn client_identity(&self) -> Option<&crate::utils::request::ClientIdentity> {
    self.client.client_identity()
  }
  pub fn binding(&self) -> Option<&crate::session::network::GatewayBinding> {
    self.client.binding()
  }
}

impl std::fmt::Debug for SessionExtensionAuth {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("SessionExtensionAuth")
      .field("username", &self.credential.username())
      .field("client", &"<redacted>")
      .finish()
  }
}

#[derive(Debug, Clone)]
pub struct SessionContext {
  server: String,
  portal: String,
  session_args: SessionRequestArgs,
  extension_auth: Option<SessionExtensionAuth>,
}

impl SessionContext {
  pub fn new(server: String, portal: String, session_args: SessionRequestArgs) -> Self {
    Self {
      server,
      portal,
      session_args,
      extension_auth: None,
    }
  }

  pub fn with_extension_auth(mut self, extension_auth: SessionExtensionAuth) -> Self {
    self.extension_auth = Some(extension_auth);
    self
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

  fn extension_auth(&self) -> Option<&SessionExtensionAuth> {
    self.extension_auth.as_ref()
  }
}

pub async fn extend_session(ctx: &SessionContext) -> anyhow::Result<()> {
  extend_session_lifetime(ctx).await?;

  let auth = ctx
    .extension_auth()
    .ok_or_else(|| anyhow::anyhow!("Session extension requires retained gateway auth state"))?;
  let client = auth.client.request_client();
  let url = format!(
    "{}/ssl-vpn/agentmessage.esp",
    auth.client.origin().as_str().trim_end_matches('/')
  );
  let form = build_extend_session_form(ctx)?;

  info!("Sending extend-session request");

  let response = client.post(&url).form(&form).send().await?;
  let response = parse_gp_response(response).await?;
  let root = Element::parse(response.as_bytes())?;
  let status = parse_extend_session_status(&root, &response)?;

  info!("Extend-session gateway response status: {}", status);

  match status.as_str() {
    "success" => Ok(()),
    status => bail!("Extend session rejected: {status}"),
  }
}

async fn extend_session_lifetime(ctx: &SessionContext) -> anyhow::Result<()> {
  let auth = ctx
    .extension_auth()
    .ok_or_else(|| anyhow::anyhow!("Session extension requires retained gateway auth state"))?;

  let login = auth.client.extend_lifetime(&auth.credential).await?;
  validate_extension_login(login)
}

fn validate_extension_login(login: GatewayLogin) -> anyhow::Result<()> {
  match login {
    GatewayLogin::Authenticated(_) => Ok(()),
    GatewayLogin::Mfa(_, _) => bail!("Session extension requires an interactive gateway challenge"),
  }
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
    os_profile::{ClientOs, HostIdentity, OsProfile},
    session::SessionRequestArgs,
  };

  use super::*;

  fn build_session_args() -> SessionRequestArgs {
    let profile = OsProfile::builder(ClientOs::Mac)
      .host_identity(HostIdentity::new(
        "test-mac".to_string(),
        "host-id".to_string(),
        "serial".to_string(),
        "aa:bb:cc:dd:ee:ff".to_string(),
      ))
      .client_version("6.3.1-12".to_string())
      .build();
    SessionRequestArgs::new("authcookie=AUTH&portal=vpn.example.com&user=alice&preferred-ip=10.0.0.10".to_string())
      .with_user_agent(profile.user_agent().to_string())
      .with_disable_ipv6(true)
  }

  #[test]
  fn active_session_context_preserves_connect_args_for_extension() {
    let session_args = build_session_args();
    let ctx = SessionContext::new(
      "vpn.example.com".to_string(),
      "portal.example.com".to_string(),
      session_args,
    );

    let form = parse_cookie_form(ctx.session_args()).unwrap();

    assert_eq!(form.get("authcookie").map(String::as_str), Some("AUTH"));
    assert_eq!(form.get("portal").map(String::as_str), Some("vpn.example.com"));
    assert_eq!(form.get("user").map(String::as_str), Some("alice"));
    assert_eq!(form.get("preferred-ip").map(String::as_str), Some("10.0.0.10"));
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

  #[test]
  fn rejects_extension_login_mfa_challenge() {
    assert_eq!(
      validate_extension_login(GatewayLogin::Mfa("MFA required".to_string(), "input".to_string()))
        .unwrap_err()
        .to_string(),
      "Session extension requires an interactive gateway challenge"
    );
  }
}
