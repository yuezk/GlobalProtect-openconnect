use anyhow::bail;
use log::info;
use reqwest::Client;
use roxmltree::Document;
use urlencoding::encode;

use crate::{
  credential::Credential,
  gp_params::GpParams,
  utils::{normalize_server, remove_url_scheme},
};

pub async fn gateway_login(
  gateway: &str,
  cred: &Credential,
  gp_params: &GpParams,
) -> anyhow::Result<String> {
  let url = normalize_server(gateway)?;
  let gateway = remove_url_scheme(&url);

  let login_url = format!("{}/ssl-vpn/login.esp", url);
  let client = Client::builder()
    .danger_accept_invalid_certs(gp_params.ignore_tls_errors())
    .user_agent(gp_params.user_agent())
    .build()?;

  let mut params = cred.to_params();
  let extra_params = gp_params.to_params();

  params.extend(extra_params);
  params.insert("server", &gateway);

  info!("Gateway login, user_agent: {}", gp_params.user_agent());

  let res = client.post(&login_url).form(&params).send().await?;
  let status = res.status();

  if status.is_client_error() || status.is_server_error() {
    bail!("Gateway login error: {}", status)
  }

  let res_xml = res.text().await?;
  let doc = Document::parse(&res_xml)?;

  build_gateway_token(&doc, gp_params.computer())
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

fn read_args<'a>(
  args: &'a [String],
  index: usize,
  key: &'a str,
) -> anyhow::Result<(&'a str, &'a str)> {
  args
    .get(index)
    .ok_or_else(|| anyhow::anyhow!("Failed to read {key} from args"))
    .map(|s| (key, s.as_ref()))
}
