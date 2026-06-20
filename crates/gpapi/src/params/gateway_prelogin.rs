use super::RequestParams;
use crate::os_profile::{OsProfile, PreloginBrowserMode, PreloginParamLocation};

/// Build request parameters for the gateway prelogin endpoint.
///
/// Same as portal prelogin but uses the gateway-specific embedded browser value.
pub(crate) fn build(profile: &OsProfile, browser_mode: PreloginBrowserMode) -> RequestParams {
  let default_browser = profile.gateway_default_browser(browser_mode);

  let params: Vec<(String, String)> = vec![
    ("tmp".into(), "tmp".into()),
    ("clientVer".into(), "4100".into()),
    ("clientos".into(), profile.client_os().as_str().into()),
    ("os-version".into(), profile.os_version().into()),
    ("host-id".into(), profile.host_id().into()),
    ("ipv6-support".into(), "yes".into()),
    ("default-browser".into(), default_browser.into()),
    ("cas-support".into(), "yes".into()),
    ("data".into(), "eyJjYXNfZW1iZWRkZWRfYnJvd3NlciI6InllcyJ9".into()),
  ];

  let mut query: Vec<(String, String)> = vec![];
  if profile.kerberos_support_in_query() {
    query.push(("kerberos-support".into(), "yes".into()));
  }

  match profile.prelogin_param_location() {
    PreloginParamLocation::Body => RequestParams { body: params, query },
    PreloginParamLocation::Query => RequestParams {
      body: vec![],
      query: params.into_iter().chain(query).collect(),
    },
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::os_profile::ClientOs;

  #[test]
  fn linux_places_params_in_body() {
    let profile = OsProfile::builder(ClientOs::Linux).build();
    let result = build(&profile, PreloginBrowserMode::Embedded);

    assert!(!result.body.is_empty());
    assert!(result.query.is_empty());
  }

  #[test]
  fn mac_places_params_in_body_with_kerberos_query() {
    let profile = OsProfile::builder(ClientOs::Mac).build();
    let result = build(&profile, PreloginBrowserMode::Embedded);

    assert!(!result.body.is_empty());
    assert_eq!(result.query.len(), 1);
    assert_eq!(result.query[0], ("kerberos-support".into(), "yes".into()));
  }

  #[test]
  fn windows_places_all_params_in_query() {
    let profile = OsProfile::builder(ClientOs::Windows).build();
    let result = build(&profile, PreloginBrowserMode::Embedded);

    assert!(result.body.is_empty());
    // 9 base params + 1 kerberos-support
    assert_eq!(result.query.len(), 10);
  }

  #[test]
  fn uses_embedded_gateway_default_browser_value() {
    let profile = OsProfile::builder(ClientOs::Linux).build();
    let result = build(&profile, PreloginBrowserMode::Embedded);

    let browser_param = result
      .body
      .iter()
      .find(|(k, _)| k == "default-browser")
      .expect("default-browser param should exist");
    assert_eq!(browser_param.1, "0");
  }

  #[test]
  fn uses_target_value_when_external_browser_is_requested() {
    let profile = OsProfile::builder(ClientOs::Linux).build();
    let result = build(&profile, PreloginBrowserMode::External);

    let browser_param = result
      .body
      .iter()
      .find(|(k, _)| k == "default-browser")
      .expect("default-browser param should exist");
    assert_eq!(browser_param.1, "4");
  }

  #[test]
  fn includes_all_required_params() {
    let profile = OsProfile::builder(ClientOs::Linux).build();
    let result = build(&profile, PreloginBrowserMode::Embedded);

    let keys: Vec<&str> = result.body.iter().map(|(k, _)| k.as_str()).collect();
    assert!(keys.contains(&"tmp"));
    assert!(keys.contains(&"clientVer"));
    assert!(keys.contains(&"clientos"));
    assert!(keys.contains(&"os-version"));
    assert!(keys.contains(&"host-id"));
    assert!(keys.contains(&"ipv6-support"));
    assert!(keys.contains(&"default-browser"));
    assert!(keys.contains(&"cas-support"));
    assert!(keys.contains(&"data"));
  }

  #[test]
  fn data_param_is_cas_embedded_browser_base64() {
    let profile = OsProfile::builder(ClientOs::Linux).build();
    let result = build(&profile, PreloginBrowserMode::Embedded);

    let data_param = result
      .body
      .iter()
      .find(|(k, _)| k == "data")
      .expect("data param should exist");
    assert_eq!(data_param.1, "eyJjYXNfZW1iZWRkZWRfYnJvd3NlciI6InllcyJ9");
  }
}
