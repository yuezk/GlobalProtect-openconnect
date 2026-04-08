use std::borrow::Cow;

use anyhow::bail;
use log::{debug, info, warn};
use reqwest::Client;
use urlencoding::{decode, encode};
use xmltree::Element;

use crate::{
  credential::Credential,
  error::PortalError,
  gp_params::GpParams,
  utils::{normalize_server, parse_gp_response, remove_url_scheme, xml::ElementExt},
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

  // It's possible to get an empty response, log the response headers for debugging
  if res.trim().is_empty() {
    debug!("Empty gateway login response headers: {:?}", res);
    bail!("Got empty gateway login response");
  }

  // MFA detected
  if res.contains("Challenge") {
    let Some((message, input_str)) = parse_mfa(&res) else {
      bail!("Failed to parse MFA challenge: {res}");
    };

    return Ok(GatewayLogin::Mfa(message, input_str));
  }

  debug!("Gateway login response: {}", res);

  let root = Element::parse(res.as_bytes())?;

  let cookie = build_gateway_token(&root, gp_params.computer())?;

  Ok(GatewayLogin::Cookie(cookie))
}

fn build_gateway_token(element: &Element, computer: &str) -> anyhow::Result<String> {
  let args = element
    .descendants("argument")
    .iter()
    .map(|e| e.get_text().unwrap_or_default())
    .collect::<Vec<_>>();

  let mut params = vec![
    read_required_arg(&args, 1, "authcookie")?,
    read_optional_arg(&args, 2, "persistent-cookie")?,
    read_optional_arg(&args, 3, "portal")?,
    read_required_arg(&args, 4, "user")?,
    read_optional_arg(&args, 7, "domain")?,
    read_optional_arg(&args, 15, "preferred-ip")?,
    read_optional_arg(&args, 18, "preferred-ipv6")?,
  ]
  .into_iter()
  .flatten()
  .collect::<Vec<_>>();
  params.push(("computer", computer.to_string()));

  let token = params
    .iter()
    .map(|(k, v)| format!("{}={}", k, encode(v)))
    .collect::<Vec<_>>()
    .join("&");

  Ok(token)
}

fn read_required_arg(
  args: &[Cow<'_, str>],
  index: usize,
  key: &'static str,
) -> anyhow::Result<Option<(&'static str, String)>> {
  let Some(value) = read_arg_value(args, index)? else {
    bail!("Failed to read {key} from args");
  };

  Ok(Some((key, value)))
}

fn read_optional_arg(
  args: &[Cow<'_, str>],
  index: usize,
  key: &'static str,
) -> anyhow::Result<Option<(&'static str, String)>> {
  Ok(read_arg_value(args, index)?.map(|value| (key, value)))
}

fn read_arg_value(args: &[Cow<'_, str>], index: usize) -> anyhow::Result<Option<String>> {
  Ok(
    args
      .get(index)
      .map(|s| normalize_arg_value(s.as_ref()))
      .transpose()
      .map_err(|err| anyhow::anyhow!("Failed to decode gateway login argument {index}: {err}"))?
      .flatten(),
  )
}

fn normalize_arg_value(value: &str) -> anyhow::Result<Option<String>> {
  if value.is_empty() || value == "(null)" || value == "-1" {
    return Ok(None);
  }

  Ok(Some(decode(value)?.into_owned()))
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

  #[test]
  fn gateway_token_keeps_upstream_cookie_fields() {
    let res = r#"
<jnlp>
  <application-desc>
    <argument></argument>
    <argument>AUTHCOOKIE</argument>
    <argument>PERSISTENTCOOKIE</argument>
    <argument>vpn.example.com</argument>
    <argument>alice</argument>
    <argument>LDAP-auth</argument>
    <argument>vsys1</argument>
    <argument>%28empty_domain%29</argument>
    <argument></argument>
    <argument></argument>
    <argument></argument>
    <argument></argument>
    <argument>tunnel</argument>
    <argument>-1</argument>
    <argument>4100</argument>
    <argument>10.0.0.10</argument>
    <argument>unused-user-cookie</argument>
    <argument>unused-prelogon-cookie</argument>
    <argument>2001:db8::10</argument>
  </application-desc>
</jnlp>
"#;

    let root = Element::parse(res.as_bytes()).unwrap();
    let token = build_gateway_token(&root, "metalklesk").unwrap();

    assert_eq!(
      token,
      "authcookie=AUTHCOOKIE&persistent-cookie=PERSISTENTCOOKIE&portal=vpn.example.com&user=alice&domain=%28empty_domain%29&preferred-ip=10.0.0.10&preferred-ipv6=2001%3Adb8%3A%3A10&computer=metalklesk"
    );
  }

  #[test]
  fn gateway_token_omits_empty_optional_fields() {
    let res = r#"
<jnlp>
  <application-desc>
    <argument></argument>
    <argument>AUTHCOOKIE</argument>
    <argument></argument>
    <argument>vpn.example.com</argument>
    <argument>alice</argument>
    <argument>LDAP-auth</argument>
    <argument>vsys1</argument>
    <argument>-1</argument>
    <argument></argument>
    <argument></argument>
    <argument></argument>
    <argument></argument>
    <argument>tunnel</argument>
    <argument>-1</argument>
    <argument>4100</argument>
    <argument></argument>
    <argument>unused-user-cookie</argument>
    <argument>unused-prelogon-cookie</argument>
    <argument>(null)</argument>
  </application-desc>
</jnlp>
"#;

    let root = Element::parse(res.as_bytes()).unwrap();
    let token = build_gateway_token(&root, "metalklesk").unwrap();

    assert_eq!(
      token,
      "authcookie=AUTHCOOKIE&portal=vpn.example.com&user=alice&computer=metalklesk"
    );
  }
}
