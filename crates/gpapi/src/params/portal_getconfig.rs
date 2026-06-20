use super::{RequestParams, credential as credential_params};
use crate::{credential::Credential, gp_params::GpParams, os_profile::ClientOs, portal::csc::swg_nonce};

/// Build request parameters for the portal getconfig endpoint.
///
/// This builder produces the form body for `getconfig.esp`. It deliberately
/// excludes `server`, `prot`, `direct`, and `jnlpReady` which are not sent
/// by the official client for this endpoint.
///
/// For SAML (prelogin) credentials the password is set to `SAMLPASS` on Linux
/// and left empty on macOS/Windows, matching official client behavior.
pub(crate) fn build(cred: &Credential, gp_params: &GpParams, server: &str) -> RequestParams {
  let profile = gp_params.os_profile();
  let nonce = if profile.client_os() == ClientOs::Linux {
    "0".to_string()
  } else {
    swg_nonce()
  };

  let mut body = credential_params::build(cred, gp_params);
  body.push(("inputStr".into(), gp_params.input_str().unwrap_or_default().into()));

  // Fixed params
  body.push(("ok".into(), "Login".into()));
  body.push(("clientVer".into(), "4100".into()));

  // OS params
  body.push(("clientos".into(), profile.client_os().as_str().into()));
  body.push(("clientgpversion".into(), profile.client_version().into()));
  body.push(("computer".into(), profile.computer().into()));
  body.push(("os-version".into(), profile.os_version().into()));
  body.push(("host-id".into(), profile.host_id().into()));
  body.push(("ipv6-support".into(), "yes".into()));
  body.push(("serialno".into(), profile.serialno().into()));

  // CSC fields
  body.push(("csc-digest".into(), String::new()));
  body.push(("config-digest".into(), String::new()));
  body.push((
    "csc-support".into(),
    if gp_params.effective_csc_support() { "yes" } else { "no" }.into(),
  ));

  // Host and SWG
  body.push(("host".into(), server.into()));
  body.push(("swg-auth-token".into(), "0".into()));
  body.push(("swg-nonce".into(), nonce));

  RequestParams { body, query: vec![] }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    credential::{PasswordCredential, PreloginCredential},
    gp_params::CscMode,
    os_profile::{ClientOs, HostIdentity, OsProfile},
  };

  fn test_profile(os: ClientOs) -> OsProfile {
    OsProfile::builder(os)
      .computer_name_override("test-computer")
      .client_version("6.3.3-100")
      .host_identity(HostIdentity::new(
        "test-computer".into(),
        "test-host-id".into(),
        "test-serial".into(),
        "aa:bb:cc:dd:ee:ff".into(),
      ))
      .build()
  }

  fn password_cred() -> Credential {
    Credential::from(PasswordCredential::new("alice", "secret"))
  }

  fn prelogin_cred() -> Credential {
    Credential::Prelogin(PreloginCredential::new(
      "alice",
      Some("my-prelogin-cookie"),
      Some("my-userauthcookie"),
      None,
    ))
  }

  #[test]
  fn excludes_forbidden_params() {
    let profile = test_profile(ClientOs::Linux);
    let cred = password_cred();
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let keys: Vec<&str> = result.body.iter().map(|(k, _)| k.as_str()).collect();
    assert!(!keys.contains(&"server"));
    assert!(!keys.contains(&"prot"));
    assert!(!keys.contains(&"direct"));
    assert!(!keys.contains(&"jnlpReady"));
  }

  #[test]
  fn includes_required_params() {
    let profile = test_profile(ClientOs::Linux);
    let cred = password_cred();
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let keys: Vec<&str> = result.body.iter().map(|(k, _)| k.as_str()).collect();
    assert!(keys.contains(&"clientgpversion"));
    assert!(keys.contains(&"host-id"));
    assert!(keys.contains(&"serialno"));
    assert!(keys.contains(&"token"));
  }

  #[test]
  fn includes_identity_from_profile() {
    let profile = test_profile(ClientOs::Mac);
    let cred = password_cred();
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let find = |key: &str| result.body.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());

    assert_eq!(find("host-id"), Some(profile.host_id()));
    assert_eq!(find("serialno"), Some(profile.serialno()));
    assert_eq!(find("clientgpversion"), Some("6.3.3-100"));
    assert_eq!(find("computer"), Some("test-computer"));
    assert_eq!(find("os-version"), Some(profile.os_version()));
    assert_eq!(find("clientos"), Some("Mac"));
  }

  #[test]
  fn linux_saml_sets_passwd_to_samlpass() {
    let profile = test_profile(ClientOs::Linux);
    let cred = prelogin_cred();
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let passwd = result.body.iter().find(|(k, _)| k == "passwd").map(|(_, v)| v.as_str());
    assert_eq!(passwd, Some("SAMLPASS"));
  }

  #[test]
  fn mac_saml_sets_passwd_to_empty() {
    let profile = test_profile(ClientOs::Mac);
    let cred = prelogin_cred();
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let passwd = result.body.iter().find(|(k, _)| k == "passwd").map(|(_, v)| v.as_str());
    assert_eq!(passwd, Some(""));
  }

  #[test]
  fn saml_without_portal_auth_cookies_uses_empty_sentinel() {
    let profile = test_profile(ClientOs::Mac);
    let cred = Credential::Prelogin(PreloginCredential::new("alice", Some("my-prelogin-cookie"), None, None));
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let find = |key: &str| result.body.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());

    assert_eq!(find("portal-userauthcookie"), Some("empty"));
    assert_eq!(find("portal-prelogonuserauthcookie"), Some("empty"));
  }

  #[test]
  fn saml_blank_portal_auth_cookie_uses_empty_sentinel() {
    let profile = test_profile(ClientOs::Mac);
    let cred = Credential::Prelogin(PreloginCredential::new(
      "alice",
      Some("my-prelogin-cookie"),
      Some(""),
      None,
    ));
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let portal_cookie = result
      .body
      .iter()
      .find(|(k, _)| k == "portal-userauthcookie")
      .map(|(_, v)| v.as_str());

    assert_eq!(portal_cookie, Some("empty"));
  }

  #[test]
  fn windows_saml_sets_passwd_to_empty() {
    let profile = test_profile(ClientOs::Windows);
    let cred = prelogin_cred();
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let passwd = result.body.iter().find(|(k, _)| k == "passwd").map(|(_, v)| v.as_str());
    assert_eq!(passwd, Some(""));
  }

  #[test]
  fn password_cred_sends_actual_password() {
    let profile = test_profile(ClientOs::Linux);
    let cred = password_cred();
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let passwd = result.body.iter().find(|(k, _)| k == "passwd").map(|(_, v)| v.as_str());
    assert_eq!(passwd, Some("secret"));
  }

  #[test]
  fn includes_csc_support_fields() {
    let profile = test_profile(ClientOs::Mac);
    let cred = password_cred();
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let find = |key: &str| result.body.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());

    assert_eq!(find("csc-support"), Some("yes"));
    assert_eq!(find("csc-digest"), Some(""));
    assert_eq!(find("config-digest"), Some(""));
    assert_eq!(find("swg-auth-token"), Some("0"));
    assert!(find("swg-nonce").is_some());
  }

  #[test]
  fn linux_uses_zero_swg_nonce() {
    let profile = test_profile(ClientOs::Linux);
    let cred = prelogin_cred();
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let nonce = result
      .body
      .iter()
      .find(|(k, _)| k == "swg-nonce")
      .map(|(_, v)| v.as_str());
    assert_eq!(nonce, Some("0"));
  }

  #[test]
  fn linux_defaults_csc_support_to_no() {
    let profile = test_profile(ClientOs::Linux);
    let cred = password_cred();
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let csc = result
      .body
      .iter()
      .find(|(k, _)| k == "csc-support")
      .map(|(_, v)| v.as_str());
    assert_eq!(csc, Some("no"));
  }

  #[test]
  fn csc_mode_override_works() {
    let cred = password_cred();
    let gp_params = GpParams::builder(test_profile(ClientOs::Linux))
      .csc_mode(CscMode::Yes)
      .build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let csc = result
      .body
      .iter()
      .find(|(k, _)| k == "csc-support")
      .map(|(_, v)| v.as_str());
    assert_eq!(csc, Some("yes"));
  }

  #[test]
  fn host_param_is_server_value() {
    let profile = test_profile(ClientOs::Linux);
    let cred = password_cred();
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let host = result.body.iter().find(|(k, _)| k == "host").map(|(_, v)| v.as_str());
    assert_eq!(host, Some("vpn.example.com"));
  }

  #[test]
  fn token_is_empty_for_password_cred() {
    let profile = test_profile(ClientOs::Linux);
    let cred = password_cred();
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    let token = result.body.iter().find(|(k, _)| k == "token").map(|(_, v)| v.as_str());
    assert_eq!(token, Some(""));
  }

  #[test]
  fn all_params_in_body_not_query() {
    let profile = test_profile(ClientOs::Linux);
    let cred = password_cred();
    let gp_params = GpParams::builder(profile.clone()).build();
    let result = build(&cred, &gp_params, "vpn.example.com");

    assert!(!result.body.is_empty());
    assert!(result.query.is_empty());
  }
}
