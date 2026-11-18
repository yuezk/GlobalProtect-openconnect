use gp_xml::redact_xml;
use reqwest::StatusCode;

pub(crate) fn log_response_xml(path: &str, status: StatusCode, body: &[u8]) {
  if !log::log_enabled!(log::Level::Debug) {
    return;
  }
  // Redirect paths can themselves contain authentication tokens.
  let endpoint = match path {
    "/global-protect/prelogin.esp"
    | "/ssl-vpn/prelogin.esp"
    | "/global-protect/getconfig.esp"
    | "/global-protect/getconfig_csc.esp"
    | "/ssl-vpn/login.esp"
    | "/ssl-vpn/getconfig.esp"
    | "/ssl-vpn/agentmessage.esp"
    | "/ssl-vpn/hipreportcheck.esp"
    | "/ssl-vpn/hipreport.esp"
    | "/ssl-vpn/logout.esp" => path,
    _ => "[response endpoint]",
  };
  // Individual XML lines remain readable in macOS unified logs with limited record sizes.
  for line in redact_xml(body).lines() {
    log::debug!("{endpoint} response XML (HTTP {}): {line}", status.as_u16());
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use xmltree::Element;

  #[test]
  fn preserves_policy_checks_and_redacts_authentication_material() {
    let input = include_str!("../../tests/fixtures/portal-response.xml");
    let xml = redact_xml(input.as_bytes());
    let root = Element::parse(xml.as_bytes()).unwrap();
    assert!(xml.contains("custom-checks") && xml.contains("HipID") && xml.contains("NV Domain"));
    assert!(xml.contains("Corp.Example.test"));
    assert!(xml.contains("com.1password.1password") && xml.contains("Version"));
    for secret in [
      "fixture-user-cookie",
      "fixture-prelogon-cookie",
      "fixture-csc-cookie",
      "fixture-username",
      "fixture-saml",
      "fixture-attribute-token",
      "fixture-property-password",
      "fixture-cookie-name",
      "fixture-query-token",
      "fixture-url-password",
      "fixture-comment",
      "fixture-pi",
    ] {
      assert!(!xml.contains(secret), "secret retained: {secret}");
    }
    assert_eq!(root.name, "policy");
  }
}
