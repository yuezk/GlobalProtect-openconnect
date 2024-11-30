use anyhow::{anyhow, bail};
use log::{info, warn};
use reqwest::{Client, StatusCode};
use roxmltree::Document;
use serde::Serialize;
use specta::Type;

use crate::{
  error::PortalError,
  gp_params::GpParams,
  utils::{base64, normalize_server, parse_gp_response, xml::NodeExt},
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

pub async fn prelogin(portal: &str, gp_params: &GpParams) -> anyhow::Result<Prelogin> {
  let user_agent = gp_params.user_agent();
  let is_gateway = gp_params.is_gateway();
  let prelogin_type = if is_gateway { "Gateway" } else { "Portal" };

  info!("{} prelogin with user_agent: {}", prelogin_type, user_agent);

  let portal = normalize_server(portal)?;
  let path = if is_gateway { "ssl-vpn" } else { "global-protect" };
  let prelogin_url = format!("{portal}/{}/prelogin.esp", path);
  let mut params = gp_params.to_params();

  params.insert("tmp", "tmp");
  params.insert("default-browser", "1");
  params.insert("cas-support", "yes");

  params.retain(|k, _| REQUIRED_PARAMS.iter().any(|required_param| required_param == k));

  let client = Client::try_from(gp_params)?;

  info!("Perform prelogin, user_agent: {}", gp_params.user_agent());

  let res = client
    .post(&prelogin_url)
    .form(&params)
    .send()
    .await
    .map_err(|e| anyhow::anyhow!(PortalError::NetworkError(e.to_string())))?;

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

  let prelogin = parse_res_xml(&res_xml, is_gateway).map_err(|err| {
    warn!("Parse response error, response: {}", res_xml);
    PortalError::PreloginError(err.to_string())
  })?;

  Ok(prelogin)
}

fn parse_res_xml(res_xml: &str, is_gateway: bool) -> anyhow::Result<Prelogin> {
  let doc = Document::parse(res_xml)?;
  let root = doc.root();

  let status = root
    .descendant_text("status")
    .ok_or_else(|| anyhow::anyhow!("Prelogin response does not contain status element"))?;
  // Check the status of the prelogin response
  if status.to_uppercase() != "SUCCESS" {
    let msg = root.descendant_text("msg").unwrap_or("Unknown error");
    bail!("{}", msg)
  }

  let region = root
    .descendant_text("region")
    .unwrap_or_else(|| {
      info!("Prelogin response does not contain region element");
      "Unknown"
    })
    .to_string();

  let saml_method = root.descendant_text("saml-auth-method");
  let saml_request = root.descendant_text("saml-request");
  let saml_default_browser = root.descendant_text("saml-default-browser");
  // Check if the prelogin response is SAML
  if saml_method.is_some() && saml_request.is_some() {
    let saml_request = base64::decode_to_string(saml_request.unwrap())?;
    let support_default_browser = saml_default_browser.map(|s| s.to_lowercase() == "yes").unwrap_or(false);

    let saml_prelogin = SamlPrelogin {
      region,
      is_gateway,
      saml_request,
      support_default_browser,
    };

    return Ok(Prelogin::Saml(saml_prelogin));
  }

  let label_username = root
    .descendant_text("username-label")
    .unwrap_or_else(|| {
      info!("Username label has no value, using default");
      "Username"
    })
    .to_string();
  let label_password = root
    .descendant_text("password-label")
    .unwrap_or_else(|| {
      info!("Password label has no value, using default");
      "Password"
    })
    .to_string();

  let auth_message = root
    .descendant_text("authentication-message")
    .unwrap_or("Please enter the login credentials")
    .to_string();
  let standard_prelogin = StandardPrelogin {
    region,
    is_gateway,
    auth_message,
    label_username,
    label_password,
  };

  Ok(Prelogin::Standard(standard_prelogin))
}
