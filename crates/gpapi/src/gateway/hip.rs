use std::collections::HashMap;

use anyhow::bail;
use reqwest::Client;

use crate::{gp_params::GpParams, utils::normalize_server};

async fn retrieve_config(gateway: &str, cookie: &str, gp_params: &GpParams) -> anyhow::Result<()> {
  let url = normalize_server(gateway)?;

  let config_url = format!("{}/ssl-vpn/getconfig.esp", url);
  let client = Client::builder()
    .danger_accept_invalid_certs(gp_params.ignore_tls_errors())
    .user_agent(gp_params.user_agent())
    .build()?;

  let mut params = serde_urlencoded::from_str::<HashMap<&str, &str>>(cookie)?;

  println!("{:?}", params);

  params.insert("client-type", "1");
  params.insert("protocol-version", "p1");
  params.insert("internal", "no");
  params.insert("ipv6-support", "yes");
  params.insert("clientos", gp_params.client_os());
  params.insert("hmac-algo", "sha1,md5,sha256");
  params.insert("enc-algo", "aes-128-cbc,aes-256-cbc");

  if let Some(os_version) = gp_params.os_version() {
    params.insert("os-version", os_version);
  }
  if let Some(client_version) = gp_params.client_version() {
    params.insert("app-version", client_version);
  }

  let res = client.post(&config_url).form(&params).send().await?;
  let status = res.status();

  if status.is_client_error() || status.is_server_error() {
    bail!("Retrieve config error: {}", status)
  }

  let res_xml = res.text().await?;
  println!("{}", res_xml);

  Ok(())
}

pub async fn hip_report(gateway: &str, cookie: &str, gp_params: &GpParams) -> anyhow::Result<()> {
  retrieve_config(gateway, cookie, gp_params).await
}
