use anyhow::bail;
use log::info;
use reqwest::{Client, StatusCode};
use roxmltree::Document;
use serde::Serialize;
use specta::Type;

use crate::{
  credential::{AuthCookieCredential, Credential},
  gateway::{parse_gateways, Gateway},
  gp_params::GpParams,
  portal::PortalError,
  utils::{normalize_server, remove_url_scheme, xml},
};

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortalConfig {
  portal: String,
  auth_cookie: AuthCookieCredential,
  config_cred: Credential,
  gateways: Vec<Gateway>,
  config_digest: Option<String>,
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
    preferred_gateway.unwrap_or_else(|| {
      self
        .gateways
        .iter()
        .min_by_key(|gateway| gateway.priority)
        .unwrap()
    })
  }
}

pub async fn retrieve_config(
  portal: &str,
  cred: &Credential,
  gp_params: &GpParams,
) -> anyhow::Result<PortalConfig> {
  let portal = normalize_server(portal)?;
  let server = remove_url_scheme(&portal);

  let url = format!("{}/global-protect/getconfig.esp", portal);
  let client = Client::builder()
    .danger_accept_invalid_certs(gp_params.ignore_tls_errors())
    .user_agent(gp_params.user_agent())
    .build()?;

  let mut params = cred.to_params();
  let extra_params = gp_params.to_params();

  params.extend(extra_params);
  params.insert("server", &server);
  params.insert("host", &server);

  info!("Portal config, user_agent: {}", gp_params.user_agent());

  let res = client.post(&url).form(&params).send().await?;
  let status = res.status();

  if status == StatusCode::NOT_FOUND {
    bail!(PortalError::ConfigError(
      "Config endpoint not found".to_string()
    ))
  }

  if status.is_client_error() || status.is_server_error() {
    bail!("Portal config error: {}", status)
  }

  let res_xml = res
    .text()
    .await
    .map_err(|e| PortalError::ConfigError(e.to_string()))?;

  if res_xml.is_empty() {
    bail!(PortalError::ConfigError(
      "Empty portal config response".to_string()
    ))
  }

  let doc = Document::parse(&res_xml).map_err(|e| PortalError::ConfigError(e.to_string()))?;

  let mut gateways = parse_gateways(&doc).unwrap_or_else(|| {
    info!("No gateways found in portal config");
    vec![]
  });

  let user_auth_cookie = xml::get_child_text(&doc, "portal-userauthcookie").unwrap_or_default();
  let prelogon_user_auth_cookie =
    xml::get_child_text(&doc, "portal-prelogonuserauthcookie").unwrap_or_default();
  let config_digest = xml::get_child_text(&doc, "config-digest");

  if gateways.is_empty() {
    gateways.push(Gateway::new(server.to_string(), server.to_string()));
  }

  Ok(PortalConfig {
    portal: server.to_string(),
    auth_cookie: AuthCookieCredential::new(
      cred.username(),
      &user_auth_cookie,
      &prelogon_user_auth_cookie,
    ),
    config_cred: cred.clone(),
    gateways,
    config_digest,
  })
}
