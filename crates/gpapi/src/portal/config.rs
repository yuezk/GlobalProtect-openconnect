use anyhow::bail;
use dns_lookup::lookup_addr;
use log::{debug, info, warn};
use reqwest::{Client, StatusCode};
use roxmltree::{Document, Node};
use serde::Serialize;
use specta::Type;

use crate::{
  credential::{AuthCookieCredential, Credential},
  error::PortalError,
  gateway::{parse_gateways, Gateway},
  gp_params::GpParams,
  utils::{normalize_server, parse_gp_response, remove_url_scheme, xml::NodeExt},
};

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortalConfig {
  portal: String,
  auth_cookie: AuthCookieCredential,
  config_cred: Credential,
  gateways: Vec<Gateway>,
  config_digest: Option<String>,
  /**
   * Variants:
   * - None: Internal host detection is not supported
   * - Some(false): Internal host detection is supported but the user is not connected to the internal network
   * - Some(true): Internal host detection is supported and the user is connected to the internal network
   */
  internal_host_detection: Option<bool>,
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

  let mut params = cred.to_params();
  // Avoid sending the auth cookies for the portal config API if the password is cached
  // Otherwise, the portal will return an error even if the password is correct, because
  // the auth cookies could have been invalidated and the portal server takes precedence
  // over the password
  if let Credential::Cached(cache_cred) = cred {
    info!("Using cached credentials, excluding auth cookies from the portal config request");

    if cache_cred.password().is_some() {
      params.remove("prelogin-cookie");
      params.remove("portal-userauthcookie");
      params.remove("portal-prelogonuserauthcookie");
    }
  }

  let extra_params = gp_params.to_params();

  params.extend(extra_params);
  params.insert("server", &server);
  params.insert("host", &server);

  info!("Retrieve the portal config, user_agent: {}", gp_params.user_agent());

  let res = client
    .post(&url)
    .form(&params)
    .send()
    .await
    .map_err(|e| anyhow::anyhow!(PortalError::NetworkError(e.to_string())))?;

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

  let doc = Document::parse(&res_xml).map_err(|e| PortalError::ConfigError(e.to_string()))?;
  let root = doc.root();

  let mut ihd_enabled = false;
  let mut prefer_internal = false;
  if let Some(ihd_node) = root.find_descendant("internal-host-detection") {
    ihd_enabled = true;
    prefer_internal = internal_host_detect(&ihd_node)
  }

  let mut gateways = parse_gateways(&root, prefer_internal).unwrap_or_else(|| {
    info!("No gateways found in portal config");
    vec![]
  });

  let user_auth_cookie = root.descendant_text("portal-userauthcookie").unwrap_or_default();
  let prelogon_user_auth_cookie = root.descendant_text("portal-prelogonuserauthcookie").unwrap_or_default();
  let config_digest = root.descendant_text("config-digest");

  if gateways.is_empty() {
    gateways.push(Gateway::new(server.to_string(), server.to_string()));
  }

  Ok(PortalConfig {
    portal: server.to_string(),
    auth_cookie: AuthCookieCredential::new(cred.username(), user_auth_cookie, prelogon_user_auth_cookie),
    config_cred: cred.clone(),
    gateways,
    config_digest: config_digest.map(|s| s.to_string()),
    internal_host_detection: if ihd_enabled { Some(prefer_internal) } else { None },
  })
}

// Perform DNS lookup and compare the result with the expected hostname
fn internal_host_detect(node: &Node) -> bool {
  let ip_info = [
    (node.child_text("ip-address"), node.child_text("host")),
    (node.child_text("ipv6-address"), node.child_text("ipv6-host")),
  ];

  info!("Found internal-host-detection, performing DNS lookup");

  for (ip_address, host) in ip_info.iter() {
    if let (Some(ip_address), Some(host)) = (ip_address.as_deref(), host.as_deref()) {
      if !ip_address.is_empty() && !host.is_empty() {
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
  }

  false
}
