use std::borrow::Cow;

use gpapi::{
  auth::{AuthDataParseResult, SamlAuthData},
  error::AuthDataParseError,
};
use log::{info, warn};
use regex::Regex;

use crate::webview::auth_messenger::AuthError;

use super::auth_messenger::AuthResult;

pub trait ResponseReader {
  fn url(&self) -> Option<String>;

  fn get_header(&self, key: &str) -> Option<String>;

  fn get_body(&self, cb: Box<dyn FnOnce(anyhow::Result<Option<Cow<'_, str>>>) + 'static>);
}

fn is_acs_endpoint(auth_response: &impl ResponseReader) -> bool {
  auth_response.url().map_or(false, |url| url.ends_with("/SAML20/SP/ACS"))
}

pub fn read_auth_data<F>(auth_response: &impl ResponseReader, cb: F)
where
  F: Fn(AuthResult) + 'static,
{
  match read_from_headers(auth_response) {
    Ok(auth_data) => {
      info!("Found auth data in headers");
      cb(Ok(auth_data))
    }

    Err(header_err) => {
      info!("Failed to read auth data from headers: {}", header_err);

      let is_acs_endpoint = is_acs_endpoint(auth_response);
      read_from_body(auth_response, move |auth_result| {
        // If the endpoint is `/SAML20/SP/ACS` and no auth data found in body, it should be considered as invalid
        let auth_result = auth_result.map_err(move |e| {
          info!("Failed to read auth data from body: {}", e);
          if is_acs_endpoint || e.is_invalid() || header_err.is_invalid() {
            AuthError::Invalid
          } else {
            AuthError::NotFound
          }
        });

        cb(auth_result);
      });
    }
  }
}

fn read_from_headers(auth_response: &impl ResponseReader) -> AuthDataParseResult {
  let Some(status) = auth_response.get_header("saml-auth-status") else {
    info!("No SAML auth status found in headers");
    return Err(AuthDataParseError::NotFound);
  };

  if status != "1" {
    info!("Found invalid auth status: {}", status);
    return Err(AuthDataParseError::Invalid);
  }

  let username = auth_response.get_header("saml-username");
  let prelogin_cookie = auth_response.get_header("prelogin-cookie");
  let portal_userauthcookie = auth_response.get_header("portal-userauthcookie");

  SamlAuthData::new(username, prelogin_cookie, portal_userauthcookie).map_err(|e| {
    warn!("Found invalid auth data: {}", e);
    AuthDataParseError::Invalid
  })
}

fn read_from_body<F>(auth_response: &impl ResponseReader, cb: F)
where
  F: FnOnce(AuthDataParseResult) + 'static,
{
  auth_response.get_body(Box::new(|body| match body {
    Ok(body) => {
      if let Some(html) = body {
        cb(read_from_html(&html))
      }
    }
    Err(err) => {
      info!("Failed to read body: {}", err);
      cb(Err(AuthDataParseError::Invalid))
    }
  }));
}

fn read_from_html(html: &str) -> AuthDataParseResult {
  if html.contains("Temporarily Unavailable") {
    info!("Found 'Temporarily Unavailable' in HTML, auth failed");
    return Err(AuthDataParseError::Invalid);
  }

  SamlAuthData::from_html(html).or_else(|err| {
    if let Some(gpcallback) = extract_gpcallback(html) {
      info!("Found gpcallback from html...");
      SamlAuthData::from_gpcallback(&gpcallback)
    } else {
      Err(err)
    }
  })
}

fn extract_gpcallback(html: &str) -> Option<String> {
  let re = Regex::new(r#"globalprotectcallback:[^"]+"#).unwrap();
  re.captures(html)
    .and_then(|captures| captures.get(0))
    .map(|m| html_escape::decode_html_entities(m.as_str()).to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn extract_gpcallback_some() {
    let html = r#"
      <meta http-equiv="refresh" content="0; URL=globalprotectcallback:PGh0bWw+PCEtLSA8c">
      <meta http-equiv="refresh" content="0; URL=globalprotectcallback:PGh0bWw+PCEtLSA8c">
    "#;

    assert_eq!(
      extract_gpcallback(html).as_deref(),
      Some("globalprotectcallback:PGh0bWw+PCEtLSA8c")
    );
  }

  #[test]
  fn extract_gpcallback_cas() {
    let html = r#"
      <meta http-equiv="refresh" content="0; URL=globalprotectcallback:cas-as=1&amp;un=xyz@email.com&amp;token=very_long_string">
    "#;

    assert_eq!(
      extract_gpcallback(html).as_deref(),
      Some("globalprotectcallback:cas-as=1&un=xyz@email.com&token=very_long_string")
    );
  }

  #[test]
  fn extract_gpcallback_none() {
    let html = r#"
      <meta http-equiv="refresh" content="0; URL=PGh0bWw+PCEtLSA8c">
    "#;

    assert_eq!(extract_gpcallback(html), None);
  }
}