use std::borrow::{Borrow, Cow};

use anyhow::bail;
use log::{info, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{error::AuthDataParseError, utils::base64::decode_to_string};

pub type AuthDataParseResult = anyhow::Result<SamlAuthData, AuthDataParseError>;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SamlAuthData {
  #[serde(alias = "un")]
  username: String,
  prelogin_cookie: Option<String>,
  portal_userauthcookie: Option<String>,
  token: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  host_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum SamlAuthResult {
  Success(SamlAuthData),
  Failure(String),
}

impl SamlAuthResult {
  pub fn is_success(&self) -> bool {
    match self {
      SamlAuthResult::Success(_) => true,
      SamlAuthResult::Failure(_) => false,
    }
  }

  pub fn host_id(&self) -> Option<&str> {
    match self {
      SamlAuthResult::Success(auth_data) => auth_data.host_id(),
      SamlAuthResult::Failure(_) => None,
    }
  }
}

impl SamlAuthData {
  pub fn new(
    username: Option<String>,
    prelogin_cookie: Option<String>,
    portal_userauthcookie: Option<String>,
  ) -> anyhow::Result<Self> {
    let username = username.unwrap_or_default();
    if username.is_empty() {
      bail!("Invalid username: <empty>");
    }

    let prelogin_cookie = prelogin_cookie.unwrap_or_default();
    let portal_userauthcookie = portal_userauthcookie.unwrap_or_default();

    if prelogin_cookie.len() <= 5 && portal_userauthcookie.len() <= 5 {
      bail!(
        "Invalid prelogin-cookie: {}, portal-userauthcookie: {}",
        prelogin_cookie,
        portal_userauthcookie
      );
    }

    Ok(Self {
      username,
      prelogin_cookie: Some(prelogin_cookie),
      portal_userauthcookie: Some(portal_userauthcookie),
      token: None,
      host_id: None,
    })
  }

  pub fn with_host_id(mut self, host_id: impl Into<String>) -> Self {
    let host_id = host_id.into();
    self.host_id = (!host_id.trim().is_empty()).then_some(host_id);
    self
  }

  pub fn from_html(html: &str) -> AuthDataParseResult {
    match parse_xml_tag(html, "saml-auth-status") {
      Some(status) if status == "1" => {
        let username = parse_xml_tag(html, "saml-username");
        let prelogin_cookie = parse_xml_tag(html, "prelogin-cookie");
        let portal_userauthcookie = parse_xml_tag(html, "portal-userauthcookie");

        let auth_data =
          SamlAuthData::new(username, prelogin_cookie, portal_userauthcookie).map_err(AuthDataParseError::Invalid)?;
        auth_data.log_summary("html");

        Ok(auth_data)
      }
      Some(status) => Err(AuthDataParseError::Invalid(anyhow::anyhow!(
        "SAML auth status: {}",
        status
      ))),
      None => Err(AuthDataParseError::NotFound),
    }
  }

  pub fn from_gpcallback(data: &str) -> anyhow::Result<SamlAuthData, AuthDataParseError> {
    let auth_data = data.trim_start_matches("globalprotectcallback:");
    // Further remove the leading "/" if it exists, because some versions of GP may include it
    let auth_data = auth_data.trim_start_matches('/');

    if auth_data.starts_with("cas-as") {
      info!("Got CAS auth data from globalprotectcallback");

      // Decode the auth data and use the original value if decoding fails
      let auth_data = urlencoding::decode(auth_data).unwrap_or_else(|err| {
        warn!("Failed to decode token auth data: {}", err);
        Cow::Borrowed(auth_data)
      });

      let auth_data: SamlAuthData = serde_urlencoded::from_str(auth_data.borrow()).map_err(|e| {
        warn!("Failed to parse token auth data: {}", e);
        warn!("Auth data: {}", auth_data);
        AuthDataParseError::Invalid(anyhow::anyhow!(e))
      })?;

      auth_data.log_summary("gpcallback-cas");
      return Ok(auth_data);
    }

    let auth_data = decode_to_string(auth_data).map_err(|e| {
      warn!("Failed to decode SAML auth data: {}, data: {}", e, data);
      AuthDataParseError::Invalid(anyhow::anyhow!(e))
    })?;
    let auth_data = Self::from_html(&auth_data)?;

    Ok(auth_data)
  }

  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn prelogin_cookie(&self) -> Option<&str> {
    self.prelogin_cookie.as_deref()
  }

  pub fn portal_userauthcookie(&self) -> Option<&str> {
    self.portal_userauthcookie.as_deref()
  }

  pub fn token(&self) -> Option<&str> {
    self.token.as_deref()
  }

  pub fn host_id(&self) -> Option<&str> {
    self.host_id.as_deref()
  }

  fn log_summary(&self, source: &str) {
    info!(
      "Parsed SAML auth data: source={}, username_present={}, prelogin_cookie_len={}, portal_userauthcookie_len={}, token_len={}",
      source,
      !self.username.is_empty(),
      optional_len(self.prelogin_cookie.as_deref()),
      optional_len(self.portal_userauthcookie.as_deref()),
      optional_len(self.token.as_deref())
    );
  }
}

fn optional_len(value: Option<&str>) -> usize {
  value.unwrap_or_default().len()
}

fn parse_xml_tag(html: &str, tag: &str) -> Option<String> {
  let re = Regex::new(&format!("<{}>(.*)</{}>", tag, tag)).unwrap();
  re.captures(html)
    .and_then(|captures| captures.get(1))
    .map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn auth_data_from_gpcallback_cas() {
    let auth_data = "globalprotectcallback:cas-as=1&un=xyz@email.com&token=very_long_string";

    let auth_data = SamlAuthData::from_gpcallback(auth_data).unwrap();

    assert_eq!(auth_data.username(), "xyz@email.com");
    assert_eq!(auth_data.token(), Some("very_long_string"));
  }

  #[test]
  fn auth_data_from_gpcallback_cas_urlencoded() {
    let auth_data = "globalprotectcallback:cas-as%3D1%26un%3Dxyz%40email.com%26token%3Dvery_long_string";

    let auth_data = SamlAuthData::from_gpcallback(auth_data).unwrap();

    assert_eq!(auth_data.username(), "xyz@email.com");
    assert_eq!(auth_data.token(), Some("very_long_string"));
  }

  #[test]
  fn auth_data_from_gpcallback_non_cas() {
    let auth_data = "PGh0bWw+PCEtLSA8c2FtbC1hdXRoLXN0YXR1cz4xPC9zYW1sLWF1dGgtc3RhdHVzPjxwcmVsb2dpbi1jb29raWU+cHJlbG9naW4tY29va2llPC9wcmVsb2dpbi1jb29raWU+PHBvcnRhbC11c2VyYXV0aGNvb2tpZT5wb3J0YWwtdXNlcmF1dGhjb29raWU8L3BvcnRhbC11c2VyYXV0aGNvb2tpZT48c2FtbC11c2VybmFtZT54eXpAZW1haWwuY29tPC9zYW1sLXVzZXJuYW1lPjxzYW1sLXNsbz5ubzwvc2FtbC1zbG8+IC0tPjwvaHRtbD4=";

    let auth_data = SamlAuthData::from_gpcallback(auth_data).unwrap();

    assert_eq!(auth_data.username(), "xyz@email.com");
    assert_eq!(auth_data.prelogin_cookie(), Some("prelogin-cookie"));
    assert_eq!(auth_data.portal_userauthcookie(), Some("portal-userauthcookie"));
  }

  #[test]
  fn auth_result_success_can_carry_host_id() {
    let auth_data = SamlAuthData::new(
      Some("alice@example.com".to_string()),
      Some("prelogin-cookie".to_string()),
      None,
    )
    .unwrap()
    .with_host_id("host-seed");
    let result = SamlAuthResult::Success(auth_data);

    assert_eq!(result.host_id(), Some("host-seed"));

    let value = serde_json::to_value(result).unwrap();
    assert_eq!(value["success"]["hostId"], "host-seed");
  }

  #[test]
  fn auth_result_success_omits_empty_host_id() {
    let auth_data = SamlAuthData::new(
      Some("alice@example.com".to_string()),
      Some("prelogin-cookie".to_string()),
      None,
    )
    .unwrap()
    .with_host_id(" ");
    let result = SamlAuthResult::Success(auth_data);

    assert_eq!(result.host_id(), None);

    let value = serde_json::to_value(result).unwrap();
    assert!(value["success"].get("hostId").is_none());
  }
}
