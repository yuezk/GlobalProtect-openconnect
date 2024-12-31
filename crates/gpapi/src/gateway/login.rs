use anyhow::bail;
use log::{info, warn};
use reqwest::Client;
use roxmltree::Document;
use urlencoding::encode;

use crate::{
  credential::Credential,
  error::PortalError,
  gp_params::GpParams,
  utils::{normalize_server, parse_gp_response, remove_url_scheme},
};

pub enum GatewayLogin {
  Cookie(String),
  Mfa(String, String),
}

pub async fn gateway_login(gateway: &str, cred: &Credential, gp_params: &GpParams) -> anyhow::Result<GatewayLogin> {
  let url = normalize_server(gateway)?;
  let gateway = remove_url_scheme(&url);

  let login_url = format!("{}/ssl-vpn/login.esp", url);
  let client = Client::try_from(gp_params)?;

  let mut params = cred.to_params();
  let extra_params = gp_params.to_params();

  params.extend(extra_params);
  params.insert("server", &gateway);

  info!("Perform gateway login, user_agent: {}", gp_params.user_agent());

  let res = client.post(&login_url).form(&params).send().await.map_err(|e| {
    warn!("Network error: {:?}", e);
    anyhow::anyhow!(PortalError::NetworkError(e))
  })?;

  let res = parse_gp_response(res).await.map_err(|err| {
    warn!("{err}");
    anyhow::anyhow!("Gateway login error: {}", err.reason)
  })?;

  // MFA detected
  if res.contains("Challenge") {
    let Some((message, input_str)) = parse_mfa(&res) else {
      bail!("Failed to parse MFA challenge: {res}");
    };

    return Ok(GatewayLogin::Mfa(message, input_str));
  }

  let doc = Document::parse(&res)?;

  let cookie = build_gateway_token(&doc, gp_params.computer())?;

  Ok(GatewayLogin::Cookie(cookie))
}

fn build_gateway_token(doc: &Document, computer: &str) -> anyhow::Result<String> {
  let args = doc
    .descendants()
    .filter(|n| n.has_tag_name("argument"))
    .map(|n| n.text().unwrap_or("").to_string())
    .collect::<Vec<_>>();

  let params = [
    read_args(&args, 1, "authcookie")?,
    read_args(&args, 3, "portal")?,
    read_args(&args, 4, "user")?,
    read_args(&args, 7, "domain")?,
    read_args(&args, 15, "preferred-ip")?,
    ("computer", computer),
  ];

  let token = params
    .iter()
    .map(|(k, v)| format!("{}={}", k, encode(v)))
    .collect::<Vec<_>>()
    .join("&");

  Ok(token)
}

fn read_args<'a>(args: &'a [String], index: usize, key: &'a str) -> anyhow::Result<(&'a str, &'a str)> {
  args
    .get(index)
    .ok_or_else(|| anyhow::anyhow!("Failed to read {key} from args"))
    .map(|s| (key, s.as_ref()))
}

fn parse_mfa(res: &str) -> Option<(String, String)> {
  let message = res
    .lines()
    .find(|l| l.contains("respMsg"))
    .and_then(|l| l.split('"').nth(1).map(|s| s.to_string()))?;

  let input_str = res
    .lines()
    .find(|l| l.contains("inputStr"))
    .and_then(|l| l.split('"').nth(1).map(|s| s.to_string()))?;

  Some((message, input_str))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn mfa() {
    let res = r#"var respStatus = "Challenge";
var respMsg = "MFA message";
thisForm.inputStr.value = "5ef64e83000119ed";"#;

    let (message, input_str) = parse_mfa(res).unwrap();
    assert_eq!(message, "MFA message");
    assert_eq!(input_str, "5ef64e83000119ed");
  }
}
