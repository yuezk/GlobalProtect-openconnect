use std::net::ToSocketAddrs;

use super::{RequestParams, credential as credential_params};
use crate::{credential::Credential, gateway::GatewayLoginContext, gp_params::GpParams};

/// Inputs required to build gateway login request parameters.
pub(crate) struct GatewayLoginInput<'a> {
  pub gp_params: &'a GpParams,
  pub cred: &'a Credential,
  pub gateway_host: &'a str,
  pub context: Option<&'a GatewayLoginContext>,
  pub client_ip: Option<&'a str>,
  pub extend_lifetime: bool,
}

/// Build request parameters for the gateway login endpoint.
///
/// Resolves the `server` parameter to an IPv4 address when
/// `profile.gateway_server_uses_resolved_ipv4()` is true.
/// Sets `gw` and `gateway-name` to the gateway FQDN.
pub(crate) fn build(input: &GatewayLoginInput) -> RequestParams {
  let profile = input.gp_params.os_profile();
  let mut body: Vec<(String, String)> = Vec::new();

  // Credential params (user, passwd, prelogin-cookie, portal-userauthcookie, etc.)
  body.extend(credential_params::build(input.cred, input.gp_params));

  // Common login params
  body.push(("prot".into(), "https:".into()));
  body.push(("jnlpReady".into(), "jnlpReady".into()));
  body.push(("ok".into(), "Login".into()));
  body.push(("direct".into(), "yes".into()));
  body.push(("ipv6-support".into(), "yes".into()));
  body.push(("clientVer".into(), "4100".into()));
  body.push(("clientos".into(), profile.client_os().as_str().into()));
  body.push(("computer".into(), profile.computer().into()));
  body.push((
    "inputStr".into(),
    input.gp_params.input_str().unwrap_or_default().into(),
  ));
  if let Some(otp) = input.gp_params.otp() {
    set_body_param(&mut body, "passwd", otp);
  }
  if let Some(os_version) = input.gp_params.os_version() {
    body.push(("os-version".into(), os_version.into()));
  }

  // Server — resolve to IPv4 when profile requires it
  let server = resolve_server(input);
  body.push(("server".into(), server));

  // Identity fields
  body.push(("host-id".into(), profile.host_id().into()));
  body.push(("serialno".into(), profile.serialno().into()));
  body.push(("preferred-ip".into(), String::new()));
  body.push(("preferred-ipv6".into(), String::new()));

  // Client version
  if let Some(client_version) = input.gp_params.client_version() {
    body.push(("clientgpversion".into(), client_version.to_string()));
  }

  // Context-dependent fields
  if let Some(context) = input.context {
    let gateway_host = context.host().to_string();
    body.push(("host".into(), gateway_host.clone()));
    body.push(("gw".into(), gateway_host.clone()));
    body.push(("gateway-name".into(), gateway_host));
    body.push(("internal".into(), context.kind().as_login_param().to_string()));
    body.push((
      "selection-type".into(),
      context.selection().as_login_param().to_string(),
    ));
    body.push(("client-ipv6".into(), String::new()));

    if let Some(connect_method) = context.connect_method() {
      body.push(("connect-method".into(), connect_method.to_string()));
    }

    if let Some(client_version) = input.gp_params.client_version() {
      // Ensure clientgpversion is set in the context block (override earlier value if present)
      if let Some(pos) = body.iter().position(|(k, _)| k == "clientgpversion") {
        body[pos].1 = client_version.to_string();
      } else {
        body.push(("clientgpversion".into(), client_version.to_string()));
      }
    }

    if let Some(client_ip) = input.client_ip {
      body.push(("client-ip".into(), client_ip.to_string()));
    }
  }

  // Extend lifetime
  if input.extend_lifetime {
    body.push(("extend-lifetime".into(), "true".into()));
  }

  RequestParams { body, query: vec![] }
}

fn set_body_param(body: &mut Vec<(String, String)>, key: &str, value: impl Into<String>) {
  let value = value.into();
  if let Some(pos) = body.iter().position(|(k, _)| k == key) {
    body[pos].1 = value;
  } else {
    body.push((key.to_string(), value));
  }
}

/// Resolve the gateway server to an IPv4 address if the profile requires it.
///
/// Falls back to the original gateway host if resolution fails.
fn resolve_server(input: &GatewayLoginInput) -> String {
  if !input.gp_params.os_profile().gateway_server_uses_resolved_ipv4() {
    return input.gateway_host.to_string();
  }

  if let Some(context) = input.context {
    resolve_to_ipv4(context.host()).unwrap_or_else(|| input.gateway_host.to_string())
  } else {
    input.gateway_host.to_string()
  }
}

/// Attempt DNS resolution of a hostname to its first IPv4 address.
fn resolve_to_ipv4(host: &str) -> Option<String> {
  (host, 443)
    .to_socket_addrs()
    .ok()?
    .find(|addr| addr.ip().is_ipv4())
    .map(|addr| addr.ip().to_string())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    credential::{AuthCookieCredential, PasswordCredential},
    gateway::{Gateway, GatewayKind, GatewaySelection},
    os_profile::{ClientOs, HostIdentity, OsProfile},
  };

  fn test_identity() -> HostIdentity {
    HostIdentity::new(
      "test-computer".to_string(),
      "host-id-123".to_string(),
      "serial-456".to_string(),
      "aa-bb-cc-dd-ee-ff".to_string(),
    )
  }

  fn test_profile(os: ClientOs) -> OsProfile {
    OsProfile::builder(os).host_identity(test_identity()).build()
  }

  fn make_gp_params() -> GpParams {
    GpParams::builder(test_profile(ClientOs::Linux)).build()
  }

  fn make_credential() -> Credential {
    Credential::Password(PasswordCredential::new("alice", "secret"))
  }

  fn passwd_values(body: &[(String, String)]) -> Vec<&str> {
    body
      .iter()
      .filter(|(k, _)| k == "passwd")
      .map(|(_, v)| v.as_str())
      .collect()
  }

  #[test]
  fn otp_retry_replaces_passwd_instead_of_appending_duplicate() {
    let mut gp_params = make_gp_params();
    gp_params.set_input_str("challenge-token");
    gp_params.set_otp("123456");

    let cred = make_credential();
    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "vpn.example.com",
      context: None,
      client_ip: None,
      extend_lifetime: false,
    };

    let result = build(&input);

    assert_eq!(passwd_values(&result.body), vec!["123456"]);

    let encoded = serde_urlencoded::to_string(&result.body).expect("form body should encode");
    assert_eq!(encoded.matches("passwd=").count(), 1);
    assert!(encoded.contains("passwd=123456"));
    assert!(!encoded.contains("passwd=secret"));
  }

  #[test]
  fn build_without_context_includes_identity_fields() {
    let profile = test_profile(ClientOs::Linux);
    let gp_params = make_gp_params();
    let cred = make_credential();

    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "vpn.example.com",
      context: None,
      client_ip: None,
      extend_lifetime: false,
    };

    let result = build(&input);

    let find = |key: &str| result.body.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());

    assert_eq!(find("host-id"), Some(profile.host_id()));
    assert_eq!(find("serialno"), Some(profile.serialno()));
    assert_eq!(find("preferred-ip"), Some(""));
    assert_eq!(find("preferred-ipv6"), Some(""));
    assert_eq!(find("token"), Some(""));
    assert_eq!(find("server"), Some("vpn.example.com"));
  }

  #[test]
  fn build_without_context_excludes_context_fields() {
    let gp_params = make_gp_params();
    let cred = make_credential();

    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "vpn.example.com",
      context: None,
      client_ip: None,
      extend_lifetime: false,
    };

    let result = build(&input);

    let has_key = |key: &str| result.body.iter().any(|(k, _)| k == key);

    assert!(!has_key("host"));
    assert!(!has_key("gw"));
    assert!(!has_key("gateway-name"));
    assert!(!has_key("internal"));
    assert!(!has_key("selection-type"));
    assert!(!has_key("connect-method"));
    assert!(!has_key("client-ip"));
    assert!(!has_key("client-ipv6"));
  }

  #[test]
  fn build_with_context_includes_gateway_fqdn() {
    let gp_params = make_gp_params();
    let cred = make_credential();
    let gateway = Gateway {
      name: "US_East".to_string(),
      address: "us1.vpn.example.com".to_string(),
      kind: GatewayKind::External,
      priority: 1,
      priority_rules: vec![],
    };
    let context = GatewayLoginContext::new(&gateway, GatewaySelection::Auto);

    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "us1.vpn.example.com",
      context: Some(&context),
      client_ip: Some("192.168.0.10"),
      extend_lifetime: false,
    };

    let result = build(&input);

    let find = |key: &str| result.body.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());

    assert_eq!(find("gw"), Some("us1.vpn.example.com"));
    assert_eq!(find("gateway-name"), Some("us1.vpn.example.com"));
    assert_eq!(find("host"), Some("us1.vpn.example.com"));
    assert_eq!(find("internal"), Some("no"));
    assert_eq!(find("selection-type"), Some("auto"));
    assert_eq!(find("client-ip"), Some("192.168.0.10"));
    assert_eq!(find("client-ipv6"), Some(""));
  }

  #[test]
  fn mac_profile_uses_fqdn_for_gateway_display() {
    let gp_params = GpParams::builder(test_profile(ClientOs::Mac)).build();
    let cred = make_credential();
    let gateway = Gateway {
      name: "US_East".to_string(),
      address: "us1.vpn.example.com".to_string(),
      kind: GatewayKind::External,
      priority: 1,
      priority_rules: vec![],
    };
    let context = GatewayLoginContext::new(&gateway, GatewaySelection::Auto);

    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "us1.vpn.example.com",
      context: Some(&context),
      client_ip: None,
      extend_lifetime: false,
    };

    let result = build(&input);

    let find = |key: &str| result.body.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());

    assert_eq!(find("gw"), Some("us1.vpn.example.com"));
    assert_eq!(find("gateway-name"), Some("us1.vpn.example.com"));
  }

  #[test]
  fn linux_prelogin_credential_uses_profile_saml_password() {
    let gp_params = GpParams::builder(test_profile(ClientOs::Linux)).build();
    let cred = Credential::Prelogin(crate::credential::PreloginCredential::new(
      "alice",
      Some("prelogin-cookie"),
      None,
      None,
    ));

    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "vpn.example.com",
      context: None,
      client_ip: None,
      extend_lifetime: false,
    };

    let result = build(&input);

    let passwd = result.body.iter().find(|(k, _)| k == "passwd").map(|(_, v)| v.as_str());
    assert_eq!(passwd, Some("SAMLPASS"));
  }

  #[test]
  fn portal_auth_cookie_login_leaves_gateway_prelogin_cookie_empty() {
    let gp_params = GpParams::builder(test_profile(ClientOs::Linux)).build();
    let cred = Credential::AuthCookie(AuthCookieCredential::new("alice", "empty", "empty"));

    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "vpn.example.com",
      context: None,
      client_ip: None,
      extend_lifetime: false,
    };

    let result = build(&input);
    let find = |key: &str| result.body.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());

    assert_eq!(find("passwd"), Some("SAMLPASS"));
    assert_eq!(find("prelogin-cookie"), Some(""));
    assert_eq!(find("portal-userauthcookie"), Some("empty"));
    assert_eq!(find("portal-prelogonuserauthcookie"), Some("empty"));
  }

  #[test]
  fn portal_auth_cookie_login_reuses_portal_password_when_available() {
    let gp_params = GpParams::builder(test_profile(ClientOs::Linux)).build();
    let cred = Credential::AuthCookie(
      AuthCookieCredential::new("alice", "portal-user-cookie", "portal-prelogon-cookie").with_password("secret"),
    );

    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "vpn.example.com",
      context: None,
      client_ip: None,
      extend_lifetime: false,
    };

    let result = build(&input);
    let find = |key: &str| result.body.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());

    assert_eq!(find("passwd"), Some("secret"));
    assert_eq!(find("prelogin-cookie"), Some(""));
    assert_eq!(find("portal-userauthcookie"), Some("portal-user-cookie"));
    assert_eq!(find("portal-prelogonuserauthcookie"), Some("portal-prelogon-cookie"));
  }

  #[test]
  fn extend_lifetime_adds_param() {
    let gp_params = make_gp_params();
    let cred = make_credential();

    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "vpn.example.com",
      context: None,
      client_ip: None,
      extend_lifetime: true,
    };

    let result = build(&input);

    let find = |key: &str| result.body.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    assert_eq!(find("extend-lifetime"), Some("true"));
  }

  #[test]
  fn no_extend_lifetime_omits_param() {
    let gp_params = make_gp_params();
    let cred = make_credential();

    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "vpn.example.com",
      context: None,
      client_ip: None,
      extend_lifetime: false,
    };

    let result = build(&input);

    let has_key = |key: &str| result.body.iter().any(|(k, _)| k == key);
    assert!(!has_key("extend-lifetime"));
  }

  #[test]
  fn context_connect_method_included_when_present() {
    let gp_params = make_gp_params();
    let cred = make_credential();
    let gateway = Gateway::new("GW".to_string(), "vpn.example.com".to_string());
    let context = GatewayLoginContext::new(&gateway, GatewaySelection::Manual).with_connect_method(Some("on-demand"));

    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "vpn.example.com",
      context: Some(&context),
      client_ip: None,
      extend_lifetime: false,
    };

    let result = build(&input);

    let find = |key: &str| result.body.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    assert_eq!(find("connect-method"), Some("on-demand"));
    assert_eq!(find("selection-type"), Some("manual"));
  }

  #[test]
  fn all_params_in_body_not_query() {
    let gp_params = make_gp_params();
    let cred = make_credential();

    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "vpn.example.com",
      context: None,
      client_ip: None,
      extend_lifetime: false,
    };

    let result = build(&input);

    assert!(result.query.is_empty());
    assert!(!result.body.is_empty());
  }

  #[test]
  fn server_resolves_to_ipv4_with_context_using_localhost() {
    let gp_params = make_gp_params();
    let cred = make_credential();
    let gateway = Gateway::new("Local".to_string(), "localhost".to_string());
    let context = GatewayLoginContext::new(&gateway, GatewaySelection::Auto);

    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "localhost",
      context: Some(&context),
      client_ip: None,
      extend_lifetime: false,
    };

    let result = build(&input);

    let server = result
      .body
      .iter()
      .find(|(k, _)| k == "server")
      .map(|(_, v)| v.as_str())
      .expect("server param should exist");
    // localhost resolves to 127.0.0.1 (IPv4)
    assert_eq!(server, "127.0.0.1");
  }

  #[test]
  fn server_falls_back_to_host_when_resolution_fails() {
    let gp_params = make_gp_params();
    let cred = make_credential();
    let gateway = Gateway::new("GW".to_string(), "nonexistent.invalid.test".to_string());
    let context = GatewayLoginContext::new(&gateway, GatewaySelection::Auto);

    let input = GatewayLoginInput {
      gp_params: &gp_params,
      cred: &cred,
      gateway_host: "nonexistent.invalid.test",
      context: Some(&context),
      client_ip: None,
      extend_lifetime: false,
    };

    let result = build(&input);

    let server = result
      .body
      .iter()
      .find(|(k, _)| k == "server")
      .map(|(_, v)| v.as_str())
      .expect("server param should exist");
    // Falls back to the original gateway host when DNS resolution fails
    assert_eq!(server, "nonexistent.invalid.test");
  }
}
