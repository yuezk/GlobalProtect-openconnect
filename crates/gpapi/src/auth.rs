use anyhow::bail;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SamlAuthData {
  username: String,
  prelogin_cookie: Option<String>,
  portal_userauthcookie: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
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
}

impl SamlAuthData {
  pub fn new(username: String, prelogin_cookie: Option<String>, portal_userauthcookie: Option<String>) -> Self {
    Self {
      username,
      prelogin_cookie,
      portal_userauthcookie,
    }
  }

  pub fn parse_html(html: &str) -> anyhow::Result<SamlAuthData> {
    match parse_xml_tag(html, "saml-auth-status") {
      Some(saml_status) if saml_status == "1" => {
        let username = parse_xml_tag(html, "saml-username");
        let prelogin_cookie = parse_xml_tag(html, "prelogin-cookie");
        let portal_userauthcookie = parse_xml_tag(html, "portal-userauthcookie");

        if SamlAuthData::check(&username, &prelogin_cookie, &portal_userauthcookie) {
          return Ok(SamlAuthData::new(
            username.unwrap(),
            prelogin_cookie,
            portal_userauthcookie,
          ));
        }

        bail!("Found invalid auth data in HTML");
      }
      Some(status) => {
        bail!("Found invalid SAML status {} in HTML", status);
      }
      None => {
        bail!("No auth data found in HTML");
      }
    }
  }

  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn prelogin_cookie(&self) -> Option<&str> {
    self.prelogin_cookie.as_deref()
  }

  pub fn check(
    username: &Option<String>,
    prelogin_cookie: &Option<String>,
    portal_userauthcookie: &Option<String>,
  ) -> bool {
    let username_valid = username.as_ref().is_some_and(|username| !username.is_empty());
    let prelogin_cookie_valid = prelogin_cookie.as_ref().is_some_and(|val| val.len() > 5);
    let portal_userauthcookie_valid = portal_userauthcookie.as_ref().is_some_and(|val| val.len() > 5);

    username_valid && (prelogin_cookie_valid || portal_userauthcookie_valid)
  }
}

pub fn parse_xml_tag(html: &str, tag: &str) -> Option<String> {
  let re = Regex::new(&format!("<{}>(.*)</{}>", tag, tag)).unwrap();
  re.captures(html)
    .and_then(|captures| captures.get(1))
    .map(|m| m.as_str().to_string())
}
