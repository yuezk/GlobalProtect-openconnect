use anyhow::bail;
use log::{info, trace};
use reqwest::Client;
use roxmltree::Document;
use serde::Serialize;
use specta::Type;

use crate::{
  gp_params::GpParams,
  utils::{base64, normalize_server, xml},
};

const REQUIRED_PARAMS: [&str; 8] = [
  "tmp",
  "clientVer",
  "clientos",
  "os-version",
  "host-id",
  "ipv6-support",
  "default-browser",
  "cas-support",
];

#[derive(Debug, Serialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SamlPrelogin {
  region: String,
  saml_request: String,
}

impl SamlPrelogin {
  pub fn region(&self) -> &str {
    &self.region
  }

  pub fn saml_request(&self) -> &str {
    &self.saml_request
  }
}

#[derive(Debug, Serialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StandardPrelogin {
  region: String,
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
}

pub async fn prelogin(portal: &str, gp_params: &GpParams) -> anyhow::Result<Prelogin> {
  let user_agent = gp_params.user_agent();
  info!("Portal prelogin, user_agent: {}", user_agent);

  let portal = normalize_server(portal)?;
  let prelogin_url = format!(
    "{}/global-protect/prelogin.esp?kerberos-support=yes",
    portal
  );
  let mut params = gp_params.to_params();
  params.insert("tmp", "tmp");
  params.insert("default-browser", "0");
  params.insert("cas-support", "yes");

  params.retain(|k, _| {
    REQUIRED_PARAMS
      .iter()
      .any(|required_param| required_param == k)
  });

  let client = Client::builder()
    .danger_accept_invalid_certs(gp_params.ignore_tls_errors())
    .user_agent(user_agent)
    .build()?;

  let res = client.post(&prelogin_url).form(&params).send().await?;
  let res_xml = res.error_for_status()?.text().await?;

  trace!("Prelogin response: {}", res_xml);
  let doc = Document::parse(&res_xml)?;

  let status = xml::get_child_text(&doc, "status")
    .ok_or_else(|| anyhow::anyhow!("Prelogin response does not contain status element"))?;
  // Check the status of the prelogin response
  if status.to_uppercase() != "SUCCESS" {
    let msg = xml::get_child_text(&doc, "msg").unwrap_or(String::from("Unknown error"));
    bail!("Prelogin failed: {}", msg)
  }

  let region = xml::get_child_text(&doc, "region")
    .ok_or_else(|| anyhow::anyhow!("Prelogin response does not contain region element"))?;

  let saml_method = xml::get_child_text(&doc, "saml-auth-method");
  let saml_request = xml::get_child_text(&doc, "saml-request");
  // Check if the prelogin response is SAML
  if saml_method.is_some() && saml_request.is_some() {
    let saml_request = base64::decode_to_string(&saml_request.unwrap())?;
    let saml_prelogin = SamlPrelogin {
      region,
      saml_request,
    };

    return Ok(Prelogin::Saml(saml_prelogin));
  }

  let label_username = xml::get_child_text(&doc, "username-label");
  let label_password = xml::get_child_text(&doc, "password-label");
  // Check if the prelogin response is standard login
  if label_username.is_some() && label_password.is_some() {
    let auth_message = xml::get_child_text(&doc, "authentication-message")
      .unwrap_or(String::from("Please enter the login credentials"));
    let standard_prelogin = StandardPrelogin {
      region,
      auth_message,
      label_username: label_username.unwrap(),
      label_password: label_password.unwrap(),
    };

    return Ok(Prelogin::Standard(standard_prelogin));
  }

  bail!("Invalid prelogin response");
}
