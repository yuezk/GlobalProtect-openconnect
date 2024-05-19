use std::collections::HashMap;

use log::{info, warn};
use reqwest::Client;
use roxmltree::Document;

use crate::{gp_params::GpParams, process::hip_launcher::HipLauncher, utils::normalize_server};

struct HipReporter<'a> {
  server: String,
  cookie: &'a str,
  md5: &'a str,
  csd_wrapper: &'a str,
  gp_params: &'a GpParams,
  client: Client,
}

impl HipReporter<'_> {
  async fn report(&self) -> anyhow::Result<()> {
    let client_ip = self.retrieve_client_ip().await?;

    let hip_needed = match self.check_hip(&client_ip).await {
      Ok(hip_needed) => hip_needed,
      Err(err) => {
        warn!("Failed to check HIP: {}", err);
        return Ok(());
      }
    };

    if !hip_needed {
      info!("HIP report not needed");
      return Ok(());
    }

    info!("HIP report needed, generating report...");
    let report = self.generate_report(&client_ip).await?;

    if let Err(err) = self.submit_hip(&client_ip, &report).await {
      warn!("Failed to submit HIP report: {}", err);
    }

    Ok(())
  }

  async fn retrieve_client_ip(&self) -> anyhow::Result<String> {
    let config_url = format!("{}/ssl-vpn/getconfig.esp", self.server);
    let mut params: HashMap<&str, &str> = HashMap::new();

    params.insert("client-type", "1");
    params.insert("protocol-version", "p1");
    params.insert("internal", "no");
    params.insert("ipv6-support", "yes");
    params.insert("clientos", self.gp_params.client_os());
    params.insert("hmac-algo", "sha1,md5,sha256");
    params.insert("enc-algo", "aes-128-cbc,aes-256-cbc");

    if let Some(os_version) = self.gp_params.os_version() {
      params.insert("os-version", os_version);
    }
    if let Some(client_version) = self.gp_params.client_version() {
      params.insert("app-version", client_version);
    }

    let params = merge_cookie_params(self.cookie, &params)?;

    let res = self.client.post(&config_url).form(&params).send().await?;
    let res_xml = res.error_for_status()?.text().await?;
    let doc = Document::parse(&res_xml)?;

    // Get <ip-address>
    let ip = doc
      .descendants()
      .find(|n| n.has_tag_name("ip-address"))
      .and_then(|n| n.text())
      .ok_or_else(|| anyhow::anyhow!("ip-address not found"))?;

    Ok(ip.to_string())
  }

  async fn check_hip(&self, client_ip: &str) -> anyhow::Result<bool> {
    let url = format!("{}/ssl-vpn/hipreportcheck.esp", self.server);
    let mut params = HashMap::new();

    params.insert("client-role", "global-protect-full");
    params.insert("client-ip", client_ip);
    params.insert("md5", self.md5);

    let params = merge_cookie_params(self.cookie, &params)?;
    let res = self.client.post(&url).form(&params).send().await?;
    let res_xml = res.error_for_status()?.text().await?;

    is_hip_needed(&res_xml)
  }

  async fn generate_report(&self, client_ip: &str) -> anyhow::Result<String> {
    let launcher = HipLauncher::new(self.csd_wrapper)
      .cookie(self.cookie)
      .md5(self.md5)
      .client_ip(client_ip)
      .client_os(self.gp_params.client_os())
      .client_version(self.gp_params.client_version());

    launcher.launch().await
  }

  async fn submit_hip(&self, client_ip: &str, report: &str) -> anyhow::Result<()> {
    let url = format!("{}/ssl-vpn/hipreport.esp", self.server);

    let mut params = HashMap::new();
    params.insert("client-role", "global-protect-full");
    params.insert("client-ip", client_ip);
    params.insert("report", report);

    let params = merge_cookie_params(self.cookie, &params)?;
    let res = self.client.post(&url).form(&params).send().await?;
    let res_xml = res.error_for_status()?.text().await?;

    info!("HIP check response: {}", res_xml);

    Ok(())
  }
}

fn is_hip_needed(res_xml: &str) -> anyhow::Result<bool> {
  let doc = Document::parse(res_xml)?;

  let hip_needed = doc
    .descendants()
    .find(|n| n.has_tag_name("hip-report-needed"))
    .and_then(|n| n.text())
    .ok_or_else(|| anyhow::anyhow!("hip-report-needed not found"))?;

  Ok(hip_needed == "yes")
}

fn merge_cookie_params(cookie: &str, params: &HashMap<&str, &str>) -> anyhow::Result<HashMap<String, String>> {
  let cookie_params = serde_urlencoded::from_str::<HashMap<String, String>>(cookie)?;
  let params = params
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .chain(cookie_params)
    .collect::<HashMap<String, String>>();

  Ok(params)
}

// Compute md5 for fields except authcookie,preferred-ip,preferred-ipv6
fn build_csd_token(cookie: &str) -> anyhow::Result<String> {
  let mut cookie_params = serde_urlencoded::from_str::<Vec<(String, String)>>(cookie)?;
  cookie_params.retain(|(k, _)| k != "authcookie" && k != "preferred-ip" && k != "preferred-ipv6");

  let token = serde_urlencoded::to_string(cookie_params)?;
  let md5 = format!("{:x}", md5::compute(token));

  Ok(md5)
}

pub async fn hip_report(gateway: &str, cookie: &str, csd_wrapper: &str, gp_params: &GpParams) -> anyhow::Result<()> {
  let client = Client::try_from(gp_params)?;
  let md5 = build_csd_token(cookie)?;

  info!("Submit HIP report md5: {}", md5);

  let reporter = HipReporter {
    server: normalize_server(gateway)?,
    cookie,
    md5: &md5,
    csd_wrapper,
    gp_params,
    client,
  };

  reporter.report().await
}
