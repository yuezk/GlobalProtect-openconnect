use anyhow::{anyhow, bail};
use log::{debug, info, warn};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use specta::Type;
use xmltree::Element;

use crate::{
  error::PortalError,
  gp_params::GpParams,
  os_profile::PreloginBrowserMode,
  params::{gateway_prelogin, portal_prelogin},
  utils::{base64, normalize_server, parse_gp_response, xml::ElementExt},
};

#[derive(Debug, Serialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SamlPrelogin {
  region: String,
  is_gateway: bool,
  saml_request: String,
  support_default_browser: bool,
}

impl SamlPrelogin {
  pub fn region(&self) -> &str {
    &self.region
  }

  pub fn saml_request(&self) -> &str {
    &self.saml_request
  }

  pub fn support_default_browser(&self) -> bool {
    self.support_default_browser
  }
}

#[derive(Debug, Serialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StandardPrelogin {
  region: String,
  is_gateway: bool,
  auth_message: String,
  label_username: String,
  label_password: String,
}

impl StandardPrelogin {
  pub fn region(&self) -> &str {
    &self.region
  }

  pub fn auth_message(&self) -> &str {
    &self.auth_message
  }

  pub fn label_username(&self) -> &str {
    &self.label_username
  }

  pub fn label_password(&self) -> &str {
    &self.label_password
  }
}

#[derive(Debug, Serialize, Type, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Prelogin {
  Saml(SamlPrelogin),
  Standard(StandardPrelogin),
}

impl Prelogin {
  pub fn region(&self) -> &str {
    match self {
      Prelogin::Saml(saml) => saml.region(),
      Prelogin::Standard(standard) => standard.region(),
    }
  }

  pub fn is_gateway(&self) -> bool {
    match self {
      Prelogin::Saml(saml) => saml.is_gateway,
      Prelogin::Standard(standard) => standard.is_gateway,
    }
  }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PreloginOptions {
  external_browser_requested: bool,
  gateway_external_browser_allowed: bool,
}

impl PreloginOptions {
  pub fn external_browser_requested(mut self, external_browser_requested: bool) -> Self {
    self.external_browser_requested = external_browser_requested;
    self
  }

  pub fn gateway_external_browser_allowed(mut self, gateway_external_browser_allowed: bool) -> Self {
    self.gateway_external_browser_allowed = gateway_external_browser_allowed;
    self
  }

  fn browser_mode(self, is_gateway: bool) -> PreloginBrowserMode {
    if self.external_browser_requested && (!is_gateway || self.gateway_external_browser_allowed) {
      PreloginBrowserMode::External
    } else {
      PreloginBrowserMode::Embedded
    }
  }
}

pub async fn prelogin(portal: &str, gp_params: &GpParams, options: PreloginOptions) -> anyhow::Result<Prelogin> {
  let user_agent = gp_params.user_agent();
  let is_gateway = gp_params.is_gateway();
  let prelogin_type = if is_gateway { "Gateway" } else { "Portal" };

  let portal = normalize_server(portal)?;
  let path = if is_gateway { "ssl-vpn" } else { "global-protect" };
  let prelogin_url = format!("{portal}/{}/prelogin.esp", path);
  let browser_mode = options.browser_mode(is_gateway);
  let request_params = if is_gateway {
    gateway_prelogin::build(gp_params.os_profile(), browser_mode)
  } else {
    portal_prelogin::build(gp_params.os_profile(), browser_mode)
  };
  let default_browser = request_param(&request_params, "default-browser").unwrap_or("");

  info!(
    "{} prelogin with user_agent: {}, default_browser: {}",
    prelogin_type, user_agent, default_browser
  );

  let client = Client::try_from(gp_params)?;

  let mut request = client.post(&prelogin_url);
  if !request_params.query.is_empty() {
    request = request.query(&request_params.query);
  }
  if !request_params.body.is_empty() {
    request = request.form(&request_params.body);
  }

  let res = request.send().await.map_err(|e| {
    warn!("Network error: {:?}", e);
    anyhow::anyhow!(PortalError::NetworkError(e))
  })?;

  let res_xml = parse_gp_response(res).await.or_else(|err| {
    if err.status == StatusCode::NOT_FOUND {
      bail!(PortalError::PreloginError("Prelogin endpoint not found".to_string()))
    }

    if err.is_status_error() {
      warn!("{err}");
      bail!("Prelogin error: {}", err.reason)
    }

    Err(anyhow!(PortalError::PreloginError(err.reason)))
  })?;

  debug!("Prelogin response XML: {}", res_xml);

  let prelogin = parse_res_xml(&res_xml, is_gateway).map_err(|err| {
    warn!("Parse response error, response: {}", res_xml);
    PortalError::PreloginError(err.to_string())
  })?;

  Ok(prelogin)
}

fn request_param<'a>(params: &'a crate::params::RequestParams, name: &str) -> Option<&'a str> {
  params
    .body
    .iter()
    .chain(params.query.iter())
    .find_map(|(key, value)| (key == name).then_some(value.as_str()))
}

fn parse_res_xml(res_xml: &str, is_gateway: bool) -> anyhow::Result<Prelogin> {
  let root = Element::parse(res_xml.as_bytes())?;

  let status = root
    .descendant_text("status")
    .ok_or_else(|| anyhow::anyhow!("Prelogin response does not contain status element"))?;
  // Check the status of the prelogin response
  if status.to_uppercase() != "SUCCESS" {
    let msg = root
      .descendant_text("msg")
      .unwrap_or_else(|| String::from("Unknown error"));
    bail!("{}", msg)
  }

  let region = root.descendant_text("region").unwrap_or_else(|| {
    info!("Prelogin response does not contain region element");
    String::from("Unknown")
  });

  let saml_method = root.descendant_text("saml-auth-method");
  let saml_request = root.descendant_text("saml-request");
  let saml_default_browser = root.descendant_text("saml-default-browser");
  // Check if the prelogin response is SAML
  if saml_method.is_some() && saml_request.is_some() {
    let saml_request = base64::decode_to_string(&saml_request.unwrap())?;
    let support_default_browser = saml_default_browser.map(|s| s.to_lowercase() == "yes").unwrap_or(false);

    let saml_prelogin = SamlPrelogin {
      region,
      is_gateway,
      saml_request,
      support_default_browser,
    };

    return Ok(Prelogin::Saml(saml_prelogin));
  }

  let label_username = root.descendant_text("username-label").unwrap_or_else(|| {
    info!("Username label has no value, using default");
    String::from("Username")
  });
  let label_password = root.descendant_text("password-label").unwrap_or_else(|| {
    info!("Password label has no value, using default");
    String::from("Password")
  });

  let auth_message = root
    .descendant_text("authentication-message")
    .unwrap_or_else(|| String::from("Please enter the login credentials"));
  let standard_prelogin = StandardPrelogin {
    region,
    is_gateway,
    auth_message,
    label_username,
    label_password,
  };

  Ok(Prelogin::Standard(standard_prelogin))
}

#[cfg(test)]
mod tests {
  use super::PreloginOptions;
  use crate::{
    gp_params::GpParams,
    os_profile::{ClientOs, HostIdentity, OsProfile, PreloginBrowserMode},
    params::{gateway_prelogin, portal_prelogin},
  };

  fn gp_params(client_os: ClientOs) -> GpParams {
    let profile = OsProfile::builder(client_os)
      .host_identity(HostIdentity::new(
        "test-computer".to_string(),
        "host-id".to_string(),
        "serial".to_string(),
        "aa:bb:cc:dd:ee:ff".to_string(),
      ))
      .build();
    GpParams::builder(profile).build()
  }

  #[test]
  fn portal_prelogin_params_include_official_profile_fields() {
    let gp_params = gp_params(ClientOs::Linux);
    let request_params = portal_prelogin::build(gp_params.os_profile(), PreloginBrowserMode::Embedded);

    let body: std::collections::HashMap<&str, &str> = request_params
      .body
      .iter()
      .map(|(k, v)| (k.as_str(), v.as_str()))
      .collect();
    let query: std::collections::HashMap<&str, &str> = request_params
      .query
      .iter()
      .map(|(k, v)| (k.as_str(), v.as_str()))
      .collect();

    assert_eq!(body.get("host-id"), Some(&gp_params.os_profile().host_id()));
    assert_eq!(body.get("default-browser"), Some(&"-10"));
    assert_eq!(body.get("cas-support"), Some(&"yes"));
    assert_eq!(body.get("data"), Some(&r#"eyJjYXNfZW1iZWRkZWRfYnJvd3NlciI6InllcyJ9"#));
    // Linux does not place kerberos-support in the query string.
    assert_eq!(query.get("kerberos-support"), None);
  }

  #[test]
  fn gateway_prelogin_uses_gateway_default_browser() {
    let gp_params = gp_params(ClientOs::Mac);
    let request_params = gateway_prelogin::build(gp_params.os_profile(), PreloginBrowserMode::Embedded);

    let body: std::collections::HashMap<&str, &str> = request_params
      .body
      .iter()
      .map(|(k, v)| (k.as_str(), v.as_str()))
      .collect();

    assert_eq!(body.get("default-browser"), Some(&"0"));
  }

  #[test]
  fn browser_mode_uses_external_for_portal_when_requested() {
    let options = PreloginOptions::default().external_browser_requested(true);

    assert_eq!(options.browser_mode(false), PreloginBrowserMode::External);
  }

  #[test]
  fn browser_mode_requires_gateway_external_browser_allowed_for_gateway_external() {
    let options = PreloginOptions::default().external_browser_requested(true);

    assert_eq!(options.browser_mode(true), PreloginBrowserMode::Embedded);

    let options = options.gateway_external_browser_allowed(true);

    assert_eq!(options.browser_mode(true), PreloginBrowserMode::External);
  }
}
