use std::{borrow::Cow, collections::HashMap, net::UdpSocket};

use anyhow::bail;
use log::{debug, info, warn};
use reqwest::Client;
use urlencoding::{decode, encode};
use xmltree::Element;

use crate::{
  credential::Credential,
  error::PortalError,
  gateway::GatewayLoginContext,
  gp_params::GpParams,
  utils::{normalize_server, parse_gp_response, remove_url_scheme, xml::ElementExt},
};

pub enum GatewayLogin {
  Cookie(String),
  Mfa(String, String),
}

pub async fn gateway_login(gateway: &str, cred: &Credential, gp_params: &GpParams) -> anyhow::Result<GatewayLogin> {
  gateway_login_with_options(gateway, cred, gp_params, None, false).await
}

pub async fn gateway_login_with_context(
  gateway: &str,
  cred: &Credential,
  gp_params: &GpParams,
  context: &GatewayLoginContext,
) -> anyhow::Result<GatewayLogin> {
  gateway_login_with_options(gateway, cred, gp_params, Some(context), false).await
}

pub async fn gateway_login_with_extend_lifetime(
  gateway: &str,
  cred: &Credential,
  gp_params: &GpParams,
) -> anyhow::Result<GatewayLogin> {
  gateway_login_with_options(gateway, cred, gp_params, None, true).await
}

async fn gateway_login_with_options(
  gateway: &str,
  cred: &Credential,
  gp_params: &GpParams,
  context: Option<&GatewayLoginContext>,
  extend_lifetime: bool,
) -> anyhow::Result<GatewayLogin> {
  let url = normalize_server(gateway)?;
  let gateway = remove_url_scheme(&url);

  let login_url = format!("{}/ssl-vpn/login.esp", url);
  let client = Client::try_from(gp_params)?;

  let client_ip = context.and_then(|context| {
    context
      .client_ip()
      .map(|ip| ip.to_string())
      .or_else(|| detect_local_ipv4(context.host()))
  });
  let params = build_gateway_login_params(
    &gateway,
    cred,
    gp_params,
    context,
    client_ip.as_deref(),
    extend_lifetime,
  );

  info!("Perform gateway login, user_agent: {}", gp_params.user_agent());
  log_gateway_login_context(context, client_ip.as_deref());

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
    info!("Empty gateway login response headers: {:?}", res);
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

fn build_gateway_login_params(
  gateway: &str,
  cred: &Credential,
  gp_params: &GpParams,
  context: Option<&GatewayLoginContext>,
  client_ip: Option<&str>,
  extend_lifetime: bool,
) -> HashMap<String, String> {
  let mut params = cred
    .to_params()
    .into_iter()
    .map(|(key, value)| (key.to_string(), value.to_string()))
    .collect::<HashMap<_, _>>();

  params.extend(
    gp_params
      .to_params()
      .into_iter()
      .map(|(key, value)| (key.to_string(), value.to_string())),
  );
  params.insert("server".to_string(), gateway.to_string());

  if let Some(context) = context {
    params.insert("host".to_string(), context.host().to_string());
    params.insert("gw".to_string(), context.name().to_string());
    params.insert("gateway-name".to_string(), context.name().to_string());
    params.insert("internal".to_string(), context.kind().as_login_param().to_string());
    params.insert(
      "selection-type".to_string(),
      context.selection().as_login_param().to_string(),
    );
    params.insert("client-ipv6".to_string(), String::new());

    if let Some(connect_method) = context.connect_method() {
      params.insert("connect-method".to_string(), connect_method.to_string());
    }

    if let Some(client_version) = context.client_version().or_else(|| gp_params.client_version()) {
      params.insert("clientgpversion".to_string(), client_version.to_string());
    }

    if let Some(client_ip) = client_ip {
      params.insert("client-ip".to_string(), client_ip.to_string());
    }
  }

  if extend_lifetime {
    params.insert("extend-lifetime".to_string(), "true".to_string());
  }

  params
}

fn detect_local_ipv4(host: &str) -> Option<String> {
  let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
  socket.connect((host, 443)).ok()?;
  let ip = socket.local_addr().ok()?.ip();

  if ip.is_ipv4() { Some(ip.to_string()) } else { None }
}

fn log_gateway_login_context(context: Option<&GatewayLoginContext>, client_ip: Option<&str>) {
  let Some(context) = context else {
    return;
  };

  info!(
    "Gateway login context: host_present={}, gateway_name_present={}, connect_method_present={}, selection_type={}, internal={}, clientgpversion_present={}, client_ip_present={}",
    !context.host().is_empty(),
    !context.name().is_empty(),
    context.connect_method().is_some(),
    context.selection().as_login_param(),
    context.kind().as_login_param(),
    context.client_version().is_some(),
    client_ip.is_some()
  );
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
  use crate::gateway::{Gateway, GatewayKind, GatewaySelection};

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
  fn normal_gateway_login_params_do_not_extend_lifetime() {
    let cred = Credential::Password(crate::credential::PasswordCredential::new("alice", "secret"));
    let gp_params = GpParams::builder().build();

    let params = build_gateway_login_params("vpn.example.com", &cred, &gp_params, None, None, false);

    assert!(!params.contains_key("extend-lifetime"));
    assert!(!params.contains_key("host"));
    assert!(!params.contains_key("gw"));
    assert!(!params.contains_key("gateway-name"));
    assert!(!params.contains_key("connect-method"));
    assert!(!params.contains_key("selection-type"));
    assert!(!params.contains_key("internal"));
    assert!(!params.contains_key("client-ip"));
    assert!(!params.contains_key("client-ipv6"));
  }

  #[test]
  fn extension_gateway_login_params_extend_lifetime() {
    let cred = Credential::Password(crate::credential::PasswordCredential::new("alice", "secret"));
    let gp_params = GpParams::builder().build();

    let params = build_gateway_login_params("vpn.example.com", &cred, &gp_params, None, None, true);

    assert_eq!(params.get("extend-lifetime").map(String::as_str), Some("true"));
  }

  #[test]
  fn gateway_login_params_include_official_context_fields() {
    let cred = Credential::Password(crate::credential::PasswordCredential::new("alice", "secret"));
    let gp_params = GpParams::builder().build();
    let gateway = Gateway {
      name: "US_East".to_string(),
      address: "us1.vpn.example.com".to_string(),
      kind: GatewayKind::External,
      priority: 1,
      priority_rules: vec![],
    };
    let context = GatewayLoginContext::new(&gateway, GatewaySelection::Auto)
      .with_connect_method(Some("on-demand"))
      .with_client_version(Some("6.3.3-915"));

    let params = build_gateway_login_params(
      "us1.vpn.example.com",
      &cred,
      &gp_params,
      Some(&context),
      Some("192.168.0.102"),
      false,
    );

    assert_eq!(params.get("host").map(String::as_str), Some("us1.vpn.example.com"));
    assert_eq!(params.get("gw").map(String::as_str), Some("US_East"));
    assert_eq!(params.get("gateway-name").map(String::as_str), Some("US_East"));
    assert_eq!(params.get("connect-method").map(String::as_str), Some("on-demand"));
    assert_eq!(params.get("selection-type").map(String::as_str), Some("auto"));
    assert_eq!(params.get("internal").map(String::as_str), Some("no"));
    assert_eq!(params.get("clientgpversion").map(String::as_str), Some("6.3.3-915"));
    assert_eq!(params.get("client-ip").map(String::as_str), Some("192.168.0.102"));
    assert_eq!(params.get("client-ipv6").map(String::as_str), Some(""));
  }

  #[test]
  fn manual_gateway_selection_maps_to_manual_param() {
    let cred = Credential::Password(crate::credential::PasswordCredential::new("alice", "secret"));
    let gp_params = GpParams::builder().build();
    let gateway = Gateway::new("Gateway".to_string(), "vpn.example.com".to_string());
    let context = GatewayLoginContext::new(&gateway, GatewaySelection::Manual);

    let params = build_gateway_login_params("vpn.example.com", &cred, &gp_params, Some(&context), None, false);

    assert_eq!(params.get("selection-type").map(String::as_str), Some("manual"));
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
