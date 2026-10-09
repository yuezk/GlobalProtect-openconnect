use std::collections::HashMap;

use anyhow::bail;
use log::{debug, info, warn};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use specta::Type;
use tokio_util::sync::CancellationToken;
use xmltree::Element;

use crate::{
  credential::{AuthCookieCredential, Credential},
  error::PortalError,
  gateway::{Gateway, parse_gateways},
  gp_params::GpParams,
  params,
  session::{InternalSessionPolicy, network::InternalHostDetection},
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
  internal_detection: Option<InternalHostDetection>,
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
  internal_session_policy: Option<InternalSessionPolicy>,
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

  pub fn internal_detection(&self) -> Option<&InternalHostDetection> {
    self.internal_detection.as_ref()
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

  pub fn internal_session_policy(&self) -> Option<InternalSessionPolicy> {
    self.internal_session_policy
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

/// Cancellation is handled inside the request so a running native detection
/// helper is always cancelled and joined. Do not drop this future on disconnect.
pub async fn retrieve_config(
  portal: &str,
  cred: &Credential,
  gp_params: &GpParams,
  cancellation: &CancellationToken,
) -> anyhow::Result<PortalConfig> {
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

  let res_xml = tokio::select! {
    biased;
    _ = cancellation.cancelled() => bail!("Portal configuration cancelled"),
    result = async {
      let res = client.post(&url).form(&request_params.body).send().await.map_err(|e| {
        warn!("Network error: {:?}", e);
        anyhow::anyhow!(PortalError::NetworkError(e))
      })?;

      parse_gp_response(res).await.or_else(|err| {
        if err.status == StatusCode::NOT_FOUND {
          bail!(PortalError::ConfigError("Config endpoint not found".to_string()));
        }

        if err.is_status_error() {
          warn!("{err}");
          bail!("Portal config error: {}", err.reason);
        }

        Err(anyhow::anyhow!(PortalError::ConfigError(err.reason)))
      })
    } => result?,
  };

  if res_xml.is_empty() {
    bail!(PortalError::ConfigError("Empty portal config response".to_string()))
  }

  debug!("Portal config response received: {} bytes", res_xml.len());
  let root = Element::parse(res_xml.as_bytes()).map_err(|e| PortalError::ConfigError(e.to_string()))?;

  if csc::is_config_criteria(&root) {
    info!("Portal returned CSC criteria: {}", csc_criteria_summary(&root));
    if !gp_params.effective_csc_support() {
      bail!(PortalError::ConfigError(
        "Portal returned CSC criteria but CSC support is disabled".to_string()
      ));
    }
    let csc_xml = tokio::select! {
      biased;
      _ = cancellation.cancelled() => bail!("Portal configuration cancelled"),
      result = retrieve_csc_config(&client, &portal, &root, cred.username(), gp_params) => result?,
    };
    debug!("Portal CSC config response received: {} bytes", csc_xml.len());
    let root = Element::parse(csc_xml.as_bytes()).map_err(|e| PortalError::ConfigError(e.to_string()))?;
    return parse_portal_config(&server, cred, root, cancellation).await;
  }

  info!("Portal did not return CSC criteria");
  parse_portal_config(&server, cred, root, cancellation).await
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

async fn parse_portal_config(
  server: &str,
  cred: &Credential,
  root: Element,
  cancellation: &CancellationToken,
) -> anyhow::Result<PortalConfig> {
  let internal_detection = root
    .descendant("internal-host-detection")
    .map(parse_internal_detection)
    .transpose()
    .unwrap_or_else(|error| {
      warn!("Internal host detection policy is unavailable: {error}");
      None
    })
    .flatten();
  let prefer_internal = match &internal_detection {
    Some(detection) => detection.detect(cancellation).await.unwrap_or_else(|error| {
      warn!("Internal host detection failed: {error}");
      false
    }),
    None => false,
  };

  if cancellation.is_cancelled() {
    bail!("Portal configuration cancelled");
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
  let internal_session_policy = parse_internal_session_policy(&root).ok();

  Ok(PortalConfig {
    portal: server.to_string(),
    auth_cookie: AuthCookieCredential::new(cred.username(), &user_auth_cookie, &prelogon_user_auth_cookie),
    config_cred: cred.clone(),
    gateways,
    connect_method,
    config_digest: config_digest.map(|s| s.to_string()),
    internal_host_detection: internal_detection.as_ref().map(|_| prefer_internal),
    internal_detection,
    version,
    allow_extend_session,
    default_browser,
    internal_session_policy,
  })
}

fn parse_internal_session_policy(root: &Element) -> anyhow::Result<InternalSessionPolicy> {
  let hip = root.descendant("hip-collection");
  let interval = hip
    .and_then(|hip| hip.child("hip-report-interval"))
    .map(|element| element.get_text().unwrap_or_default().into_owned());
  let collect = hip
    .and_then(|hip| hip.child("collect-hip-data"))
    .map(|element| element.get_text().unwrap_or_default().into_owned());
  let interval = parse_policy_number(interval, 3600, "hip-report-interval")?;
  let collect = match collect.as_deref().map(str::trim) {
    None | Some("yes") => true,
    Some("no") => false,
    Some(_) => bail!("Invalid portal collect-hip-data policy"),
  };
  InternalSessionPolicy::new(interval, collect)
}

fn parse_policy_number(value: Option<String>, default: u32, field: &str) -> anyhow::Result<u32> {
  match value {
    Some(value) => value
      .trim()
      .parse()
      .map_err(|_| anyhow::anyhow!("Invalid portal {field} policy")),
    None => Ok(default),
  }
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

fn parse_internal_detection(element: &Element) -> anyhow::Result<Option<InternalHostDetection>> {
  let ip_info = [
    (element.child_text("ip-address"), element.child_text("host")),
    (element.child_text("ipv6-address"), element.child_text("ipv6-host")),
  ];

  let mut targets = Vec::new();
  for (ip_address, host) in ip_info.iter() {
    let address = ip_address.as_deref().filter(|value| !value.is_empty());
    let host = host.as_deref().filter(|value| !value.is_empty());
    match (address, host) {
      (None, None) => {}
      (Some(address), Some(host)) => targets.push((address.parse()?, host.to_owned())),
      _ => bail!("Incomplete internal host detection policy"),
    }
  }
  if targets.is_empty() {
    Ok(None)
  } else {
    Ok(Some(InternalHostDetection::new(targets)?))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn invalid_non_tunnel_policy_does_not_reject_tunnel_candidates() {
    let root = parse_xml(
      "<policy><hip-collection><hip-report-interval>invalid</hip-report-interval></hip-collection><internal-host-detection><ip-address>invalid</ip-address><host>internal.example</host></internal-host-detection><gateways><external><list><entry name='vpn.example'/></list></external></gateways></policy>",
    );
    let credential = crate::credential::PasswordCredential::new("alice", "password").into();
    let config = parse_portal_config("portal.example", &credential, root, &CancellationToken::new())
      .await
      .unwrap();
    assert!(config.internal_session_policy().is_none());
    assert_eq!(config.gateways()[0].server(), "vpn.example");
    assert_eq!(config.gateways()[0].kind(), crate::gateway::GatewayKind::External);
  }

  fn parse_xml(xml: &str) -> Element {
    Element::parse(xml.as_bytes()).unwrap()
  }

  #[test]
  fn parses_internal_maintenance_policy_with_explicit_units() {
    let root = parse_xml(
      "<policy><hip-collection><hip-report-interval>60</hip-report-interval><collect-hip-data>no</collect-hip-data></hip-collection><max-internal-gateway-connection-attempts>3</max-internal-gateway-connection-attempts></policy>",
    );
    let policy = parse_internal_session_policy(&root).unwrap();
    assert_eq!(policy.hip_interval(), std::time::Duration::from_secs(60));
    assert!(!policy.collect_hip_data());
    assert_eq!(
      parse_internal_session_policy(&parse_xml("<policy/>")).unwrap(),
      InternalSessionPolicy::default()
    );
  }

  #[test]
  fn rejects_invalid_supplied_internal_policy() {
    for interval in ["0", "-1", "", "4294967295", "not-a-number"] {
      let root = parse_xml(&format!(
        "<policy><hip-collection><hip-report-interval>{interval}</hip-report-interval></hip-collection></policy>"
      ));
      assert!(parse_internal_session_policy(&root).is_err());
    }
    let root = parse_xml("<policy><hip-collection><collect-hip-data>true</collect-hip-data></hip-collection></policy>");
    assert!(parse_internal_session_policy(&root).is_err());
  }

  #[tokio::test]
  async fn accepts_existing_full_portal_fixture() {
    let root = parse_xml(include_str!("../../tests/files/portal_config.xml"));
    let cred = Credential::from(crate::credential::PasswordCredential::new(
      "fixture-user",
      "fixture-password",
    ));
    let config = parse_portal_config("vpn.example.com", &cred, root, &CancellationToken::new())
      .await
      .unwrap();
    assert_eq!(
      config.internal_session_policy().unwrap().hip_interval(),
      std::time::Duration::from_secs(3600)
    );
    assert!(!config.gateways().is_empty());
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

  #[tokio::test]
  async fn parses_csc_policy_response_as_portal_config() {
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

    let config = parse_portal_config("vpn.example.com", &cred, root, &CancellationToken::new())
      .await
      .unwrap();

    assert_eq!(config.auth_cookie().user_auth_cookie(), "user-cookie");
    assert_eq!(config.auth_cookie().prelogon_user_auth_cookie(), "prelogon-cookie");
    assert_eq!(config.gateways().len(), 1);
  }
}
