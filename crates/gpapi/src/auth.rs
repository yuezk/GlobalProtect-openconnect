use std::borrow::{Borrow, Cow};
use std::sync::LazyLock;

use anyhow::bail;
use log::{info, warn};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{error::AuthDataParseError, utils::base64::decode_to_string};

pub type AuthDataParseResult = anyhow::Result<SamlAuthData, AuthDataParseError>;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SamlAuthData {
  #[serde(alias = "un")]
  username: String,
  prelogin_cookie: Option<String>,
  portal_userauthcookie: Option<String>,
  token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SamlAuthResult {
  Success(SamlAuthData),
  Failure(String),
}

impl SamlAuthResult {
  /// Returns `true` if the result is a success.
  pub fn is_success(&self) -> bool {
    matches!(self, SamlAuthResult::Success(_))
  }

  /// Returns `true` if the result is a failure.
  pub fn is_failure(&self) -> bool {
    matches!(self, SamlAuthResult::Failure(_))
  }

  /// Converts from `SamlAuthResult` to `Result<&SamlAuthData, &String>`.
  pub fn as_result(&self) -> Result<&SamlAuthData, &String> {
    match self {
      SamlAuthResult::Success(data) => Ok(data),
      SamlAuthResult::Failure(err) => Err(err),
    }
  }

  /// Converts from `SamlAuthResult` to `Result<SamlAuthData, String>`, consuming self.
  pub fn into_result(self) -> Result<SamlAuthData, String> {
    match self {
      SamlAuthResult::Success(data) => Ok(data),
      SamlAuthResult::Failure(err) => Err(err),
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
    })
  }

  pub fn from_html(html: &str) -> AuthDataParseResult {
    match parse_xml_tag(html, "saml-auth-status") {
      Some(status) if status == "1" => {
        let username = parse_xml_tag(html, "saml-username");
        let prelogin_cookie = parse_xml_tag(html, "prelogin-cookie");
        let portal_userauthcookie = parse_xml_tag(html, "portal-userauthcookie");

        SamlAuthData::new(username, prelogin_cookie, portal_userauthcookie).map_err(AuthDataParseError::Invalid)
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

      return Ok(auth_data);
    }

    let auth_data = decode_to_string(auth_data).map_err(|e| {
      warn!("Failed to decode SAML auth data: {}", e);
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

  pub fn token(&self) -> Option<&str> {
    self.token.as_deref()
  }
}

fn parse_xml_tag(html: &str, tag: &str) -> Option<String> {
  // Note: This creates a regex on each call for different tags.
  // Since we have a finite set of tags (saml-auth-status, saml-username, prelogin-cookie, portal-userauthcookie),
  // we could optimize further by pre-compiling all regexes, but this approach balances simplicity and performance.
  // Using RwLock since cache hits (reads) are more common than cache misses (writes).
  static REGEX_CACHE: LazyLock<std::sync::RwLock<std::collections::HashMap<String, Regex>>> =
    LazyLock::new(|| std::sync::RwLock::new(std::collections::HashMap::new()));

  // Try to read from cache first (fast path)
  {
    let cache = REGEX_CACHE.read().ok()?;
    if let Some(regex) = cache.get(tag) {
      return regex.captures(html)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().to_string());
    }
  }

  // Cache miss - acquire write lock and insert
  let regex = {
    let mut cache = REGEX_CACHE.write().ok()?;
    cache
      .entry(tag.to_string())
      .or_insert_with(|| {
        Regex::new(&format!("<{}>(.*)</{}>", regex::escape(tag), regex::escape(tag)))
          .expect("Invalid regex pattern for known-safe tag")
      })
      .clone()
  };


  regex
    .captures(html)
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
    let auth_data = "PGh0bWw+PCEtLSA8c2FtbC1hdXRoLXN0YXR1cz4xPC9zYW1sLWF1dGgtc3RhdHVzPjxwcmVsb2dpbi1jb29raWU+cHJlbG9naW4tY29va2llPC9wcmVsb2dpbi1jb29raWU+PHNhbWwtdXNlcm5hbWU+eHl6QGVtYWlsLmNvbTwvc2FtbC11c2VybmFtZT48c2FtbC1zbG8+bm88L3NhbWwtc2xvPjxzYW1sLVNlc3Npb25Ob3RPbk9yQWZ0ZXI+PC9zYW1sLVNlc3Npb25Ob3RPbk9yQWZ0ZXI+IC0tPjwvaHRtbD4=";

    let auth_data = SamlAuthData::from_gpcallback(auth_data).unwrap();

    assert_eq!(auth_data.username(), "xyz@email.com");
    assert_eq!(auth_data.prelogin_cookie(), Some("prelogin-cookie"));
  }
}
