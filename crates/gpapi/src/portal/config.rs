use std::collections::HashMap;

use anyhow::bail;
use dns_lookup::lookup_addr;
use log::{debug, info, warn};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use specta::Type;
use xmltree::Element;

use crate::{
  credential::{AuthCookieCredential, Credential},
  error::PortalError,
  gateway::{Gateway, parse_gateways},
  gp_params::GpParams,
  params,
  utils::{normalize_server, parse_gp_response, redact::redact_form_params, remove_url_scheme, xml::ElementExt},
};

use super::csc;

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortalConfig {
  portal: String,
  auth_cookie: AuthCookieCredential,
  config_cred: Credential,
  gateways: Vec<Gateway>,
  connect_method: Option<String>,
  config_digest: Option<String>,
  /**
   * Variants:
   * - None: Internal host detection is not supported
   * - Some(false): Internal host detection is supported but the user is not connected to the internal network
   * - Some(true): Internal host detection is supported and the user is connected to the internal network
   */
  internal_host_detection: Option<bool>,
  /**
   * The version returned by the portal config, if any
   */
  version: Option<String>,
  /**
   * Whether the portal policy allows extending the gateway session.
   */
  allow_extend_session: Option<bool>,
  /**
   * Whether the portal policy enables default-browser authentication.
   */
  default_browser: Option<bool>,
}

impl PortalConfig {
  pub fn portal(&self) -> &str {
    &self.portal
  }

  pub fn gateways(&self) -> Vec<&Gateway> {
    self.gateways.iter().collect()
  }

  pub fn auth_cookie(&self) -> &AuthCookieCredential {
    &self.auth_cookie
  }

  pub fn config_cred(&self) -> &Credential {
    &self.config_cred
  }

  pub fn internal_host_detection(&self) -> Option<bool> {
    self.internal_host_detection
  }

  pub fn connect_method(&self) -> Option<&str> {
    self.connect_method.as_deref()
  }

  pub fn version(&self) -> Option<&str> {
    self.version.as_deref()
  }

  pub fn allow_extend_session(&self) -> Option<bool> {
    self.allow_extend_session
  }

  pub fn default_browser(&self) -> Option<bool> {
    self.default_browser
  }

  /// In-place sort the gateways by region
  pub fn sort_gateways(&mut self, region: &str) {
    let preferred_gateway = self.find_preferred_gateway(region);
    let preferred_gateway_index = self
      .gateways()
      .iter()
      .position(|gateway| gateway.name == preferred_gateway.name)
      .unwrap();

    // Move the preferred gateway to the front of the list
    self.gateways.swap(0, preferred_gateway_index);
  }

  /// Find a gateway by name or address
  pub fn find_gateway(&self, name_or_address: &str) -> Option<&Gateway> {
    self
      .gateways
      .iter()
      .find(|gateway| gateway.name == name_or_address || gateway.address == name_or_address)
  }

  /// Find the preferred gateway for the given region
  /// Iterates over the gateways and find the first one that
  /// has the lowest priority for the given region.
  /// If no gateway is found, returns the gateway with the lowest priority.
  pub fn find_preferred_gateway(&self, region: &str) -> &Gateway {
    let mut preferred_gateway: Option<&Gateway> = None;
    let mut lowest_region_priority = u32::MAX;

    for gateway in &self.gateways {
      for rule in &gateway.priority_rules {
        if (rule.name == region || rule.name == "Any") && rule.priority < lowest_region_priority {
          preferred_gateway = Some(gateway);
          lowest_region_priority = rule.priority;
        }
      }
    }

    // If no gateway is found, return the gateway with the lowest priority
    preferred_gateway.unwrap_or_else(|| self.gateways.iter().min_by_key(|gateway| gateway.priority).unwrap())
  }
}

pub async fn retrieve_config(portal: &str, cred: &Credential, gp_params: &GpParams) -> anyhow::Result<PortalConfig> {
  let portal = normalize_server(portal)?;
  let server = remove_url_scheme(&portal);

  let url = format!("{}/global-protect/getconfig.esp", portal);
  let client = Client::try_from(gp_params)?;

  let request_params = params::portal_getconfig::build(cred, gp_params, &server);
  let body_pairs: Vec<(&str, &str)> = request_params
    .body
    .iter()
    .map(|(k, v)| (k.as_str(), v.as_str()))
    .collect();

  info!("Retrieve the portal config, user_agent: {}", gp_params.user_agent());
  info!("Portal config request params: {}", redact_form_params(&body_pairs));

  let res = client.post(&url).form(&request_params.body).send().await.map_err(|e| {
    warn!("Network error: {:?}", e);
    anyhow::anyhow!(PortalError::NetworkError(e))
  })?;

  let res_xml = parse_gp_response(res).await.or_else(|err| {
    if err.status == StatusCode::NOT_FOUND {
      bail!(PortalError::ConfigError("Config endpoint not found".to_string()));
    }

    if err.is_status_error() {
      warn!("{err}");
      bail!("Portal config error: {}", err.reason);
    }

    Err(anyhow::anyhow!(PortalError::ConfigError(err.reason)))
  })?;

  if res_xml.is_empty() {
    bail!(PortalError::ConfigError("Empty portal config response".to_string()))
  }

  debug!("Portal config response: {}", res_xml);
  let root = Element::parse(res_xml.as_bytes()).map_err(|e| PortalError::ConfigError(e.to_string()))?;

  if csc::is_config_criteria(&root) {
    info!("Portal returned CSC criteria: {}", csc_criteria_summary(&root));
    if !gp_params.effective_csc_support() {
      bail!(PortalError::ConfigError(
        "Portal returned CSC criteria but CSC support is disabled".to_string()
      ));
    }
    let csc_xml = retrieve_csc_config(&client, &portal, &root, cred.username(), gp_params).await?;
    debug!("Portal CSC config response: {}", csc_xml);
    let root = Element::parse(csc_xml.as_bytes()).map_err(|e| PortalError::ConfigError(e.to_string()))?;
    return parse_portal_config(&server, cred, root);
  }

  info!("Portal did not return CSC criteria");
  parse_portal_config(&server, cred, root)
}

async fn retrieve_csc_config(
  client: &Client,
  portal: &str,
  root: &Element,
  username: &str,
  gp_params: &GpParams,
) -> anyhow::Result<String> {
  let csc_req = csc::build_csc_request(root, username, gp_params)?;
  let swg_nonce = csc::swg_nonce();
  let params = csc::csc_params(&csc_req, username, gp_params, &swg_nonce);
  let url = format!("{}/global-protect/getconfig_csc.esp", portal);

  info!("Portal CSC config request summary: {}", csc_req.summary());
  info!("Portal CSC config request params: {}", redact_params(&params));

  let res = client.post(&url).form(&params).send().await.map_err(|e| {
    warn!("Network error: {:?}", e);
    anyhow::anyhow!(PortalError::NetworkError(e))
  })?;

  parse_gp_response(res).await.or_else(|err| {
    if err.status == StatusCode::NOT_FOUND {
      bail!(PortalError::ConfigError("CSC config endpoint not found".to_string()));
    }

    if err.is_status_error() {
      warn!("{err}");
      bail!("Portal CSC config error: {}", err.reason);
    }

    Err(anyhow::anyhow!(PortalError::ConfigError(err.reason)))
  })
}

fn redact_params(params: &HashMap<&str, &str>) -> String {
  let params = params.iter().map(|(key, value)| (*key, *value)).collect::<Vec<_>>();
  redact_form_params(&params)
}

fn csc_criteria_summary(root: &Element) -> String {
  let auth_cookie = present(root.descendant_text("portal-csc-auth-cookie").as_deref());
  let config_digest = present(root.descendant_text("config-digest").as_deref());
  let custom_check_entries = root
    .descendant("custom-checks")
    .map(|custom_checks| custom_checks.descendants("entry").len())
    .unwrap_or_default();

  format!("auth_cookie={auth_cookie}, config_digest={config_digest}, custom_check_entries={custom_check_entries}")
}

fn present(value: Option<&str>) -> &'static str {
  match value {
    Some(value) if !value.is_empty() => "present",
    _ => "empty",
  }
}

fn parse_portal_config(server: &str, cred: &Credential, root: Element) -> anyhow::Result<PortalConfig> {
  let mut ihd_enabled = false;
  let mut prefer_internal = false;
  if let Some(ihd_node) = root.descendant("internal-host-detection") {
    ihd_enabled = true;
    prefer_internal = internal_host_detect(ihd_node)
  }

  let mut gateways = parse_gateways(&root, prefer_internal).unwrap_or_else(|| {
    info!("No gateways found in portal config");
    vec![]
  });

  let user_auth_cookie = root.descendant_text("portal-userauthcookie").unwrap_or_default();
  let prelogon_user_auth_cookie = root
    .descendant_text("portal-prelogonuserauthcookie")
    .unwrap_or_default();
  let config_digest = root.descendant_text("config-digest");
  let connect_method = parse_connect_method(&root);

  if gateways.is_empty() {
    gateways.push(Gateway::new(server.to_string(), server.to_string()));
  } else {
    info!("Found {} gateways in portal config", gateways.len());
  }

  let version = root.descendant_text("version").map(|s| s.to_string());
  info!("Detected portal version: {:?}", version);
  let allow_extend_session = parse_allow_extend_session(&root);
  let default_browser = parse_default_browser(&root);

  Ok(PortalConfig {
    portal: server.to_string(),
    auth_cookie: AuthCookieCredential::new(cred.username(), &user_auth_cookie, &prelogon_user_auth_cookie),
    config_cred: cred.clone(),
    gateways,
    connect_method,
    config_digest: config_digest.map(|s| s.to_string()),
    internal_host_detection: if ihd_enabled { Some(prefer_internal) } else { None },
    version,
    allow_extend_session,
    default_browser,
  })
}

fn parse_default_browser(root: &Element) -> Option<bool> {
  match root.descendant_text("default-browser")?.trim() {
    "yes" => Some(true),
    "no" => Some(false),
    _ => None,
  }
}

fn parse_allow_extend_session(root: &Element) -> Option<bool> {
  match root.descendant_text("allow-extend-session")?.trim() {
    "yes" => Some(true),
    "no" => Some(false),
    _ => None,
  }
}

fn parse_connect_method(root: &Element) -> Option<String> {
  root
    .descendant_text("connect-method")
    .filter(|s| !s.is_empty())
    .map(|s| s.to_string())
}

// Perform DNS lookup and compare the result with the expected hostname
fn internal_host_detect(element: &Element) -> bool {
  let ip_info = [
    (element.child_text("ip-address"), element.child_text("host")),
    (element.child_text("ipv6-address"), element.child_text("ipv6-host")),
  ];

  info!("Found internal-host-detection, performing DNS lookup");

  for (ip_address, host) in ip_info.iter() {
    if let (Some(ip_address), Some(host)) = (ip_address.as_deref(), host.as_deref())
      && !ip_address.is_empty()
      && !host.is_empty()
    {
      match ip_address.parse::<std::net::IpAddr>() {
        Ok(ip) => match lookup_addr(&ip) {
          Ok(host_lookup) if host_lookup.to_lowercase() == host.to_lowercase() => {
            return true;
          }
          Ok(host_lookup) => {
            info!(
              "rDNS lookup for {} returned {}, expected {}",
              ip_address, host_lookup, host
            );
          }
          Err(err) => warn!("rDNS lookup failed for {}: {}", ip_address, err),
        },
        Err(err) => warn!("Invalid IP address {}: {}", ip_address, err),
      }
    }
  }

  false
}

#[cfg(test)]
mod tests {
  use super::*;

  fn parse_xml(xml: &str) -> Element {
    Element::parse(xml.as_bytes()).unwrap()
  }

  #[test]
  fn parses_allow_extend_session_yes() {
    let root = parse_xml("<response><allow-extend-session>yes</allow-extend-session></response>");

    assert_eq!(parse_allow_extend_session(&root), Some(true));
  }

  #[test]
  fn parses_allow_extend_session_no() {
    let root = parse_xml("<response><allow-extend-session>no</allow-extend-session></response>");

    assert_eq!(parse_allow_extend_session(&root), Some(false));
  }

  #[test]
  fn leaves_absent_allow_extend_session_unknown() {
    let root = parse_xml("<response></response>");

    assert_eq!(parse_allow_extend_session(&root), None);
  }

  #[test]
  fn ignores_non_official_allow_extend_session_values() {
    let root = parse_xml("<response><allow-extend-session>true</allow-extend-session></response>");

    assert_eq!(parse_allow_extend_session(&root), None);
  }

  #[test]
  fn parses_default_browser_yes() {
    let root = parse_xml("<response><default-browser>yes</default-browser></response>");

    assert_eq!(parse_default_browser(&root), Some(true));
  }

  #[test]
  fn parses_default_browser_no() {
    let root = parse_xml("<response><default-browser>no</default-browser></response>");

    assert_eq!(parse_default_browser(&root), Some(false));
  }

  #[test]
  fn leaves_absent_default_browser_unknown() {
    let root = parse_xml("<response></response>");

    assert_eq!(parse_default_browser(&root), None);
  }

  #[test]
  fn parses_connect_method() {
    let root = parse_xml("<policy><connect-method>on-demand</connect-method></policy>");

    assert_eq!(parse_connect_method(&root).as_deref(), Some("on-demand"));
  }

  #[test]
  fn parses_csc_policy_response_as_portal_config() {
    let root = parse_xml(
      r#"<policy>
        <portal-userauthcookie>user-cookie</portal-userauthcookie>
        <portal-prelogonuserauthcookie>prelogon-cookie</portal-prelogonuserauthcookie>
        <gateways>
          <external>
            <list>
              <entry name="US_East">
                <description>us1.vpn.example.com</description>
              </entry>
            </list>
          </external>
        </gateways>
      </policy>"#,
    );
    let cred = Credential::from(crate::credential::PasswordCredential::new("alice", "secret"));

    let config = parse_portal_config("vpn.example.com", &cred, root).unwrap();

    assert_eq!(config.auth_cookie().user_auth_cookie(), "user-cookie");
    assert_eq!(config.auth_cookie().prelogon_user_auth_cookie(), "prelogon-cookie");
    assert_eq!(config.gateways().len(), 1);
  }
}
