use std::collections::HashMap;

use log::info;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{auth::SamlAuthData, utils::base64::decode_to_string};

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PasswordCredential {
  username: String,
  password: String,
}

impl PasswordCredential {
  pub fn new(username: &str, password: &str) -> Self {
    Self {
      username: username.to_string(),
      password: password.to_string(),
    }
  }

  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn password(&self) -> &str {
    &self.password
  }
}

impl From<&CachedCredential> for PasswordCredential {
  fn from(value: &CachedCredential) -> Self {
    Self::new(value.username(), value.password().unwrap_or_default())
  }
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PreloginCookieCredential {
  username: String,
  prelogin_cookie: String,
}

impl PreloginCookieCredential {
  pub fn new(username: &str, prelogin_cookie: &str) -> Self {
    Self {
      username: username.to_string(),
      prelogin_cookie: prelogin_cookie.to_string(),
    }
  }

  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn prelogin_cookie(&self) -> &str {
    &self.prelogin_cookie
  }
}

impl TryFrom<SamlAuthData> for PreloginCookieCredential {
  type Error = anyhow::Error;

  fn try_from(value: SamlAuthData) -> Result<Self, Self::Error> {
    let username = value.username().to_string();
    let prelogin_cookie = value
      .prelogin_cookie()
      .ok_or_else(|| anyhow::anyhow!("Missing prelogin cookie"))?
      .to_string();

    Ok(Self::new(&username, &prelogin_cookie))
  }
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AuthCookieCredential {
  username: String,
  user_auth_cookie: String,
  prelogon_user_auth_cookie: String,
}

impl AuthCookieCredential {
  pub fn new(username: &str, user_auth_cookie: &str, prelogon_user_auth_cookie: &str) -> Self {
    Self {
      username: username.to_string(),
      user_auth_cookie: user_auth_cookie.to_string(),
      prelogon_user_auth_cookie: prelogon_user_auth_cookie.to_string(),
    }
  }

  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn user_auth_cookie(&self) -> &str {
    &self.user_auth_cookie
  }

  pub fn prelogon_user_auth_cookie(&self) -> &str {
    &self.prelogon_user_auth_cookie
  }
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CachedCredential {
  username: String,
  password: Option<String>,
  auth_cookie: AuthCookieCredential,
}

impl CachedCredential {
  pub fn new(username: String, password: Option<String>, auth_cookie: AuthCookieCredential) -> Self {
    Self {
      username,
      password,
      auth_cookie,
    }
  }

  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn password(&self) -> Option<&str> {
    self.password.as_deref()
  }

  pub fn auth_cookie(&self) -> &AuthCookieCredential {
    &self.auth_cookie
  }

  pub fn set_auth_cookie(&mut self, auth_cookie: AuthCookieCredential) {
    self.auth_cookie = auth_cookie;
  }

  pub fn set_username(&mut self, username: String) {
    self.username = username;
  }

  pub fn set_password(&mut self, password: Option<String>) {
    self.password = password.map(|s| s.to_string());
  }
}

impl From<PasswordCredential> for CachedCredential {
  fn from(value: PasswordCredential) -> Self {
    Self::new(
      value.username().to_owned(),
      Some(value.password().to_owned()),
      AuthCookieCredential::new("", "", ""),
    )
  }
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
pub struct TokenCredential {
  #[serde(alias = "un")]
  username: String,
  token: String,
}

impl TokenCredential {
  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn token(&self) -> &str {
    &self.token
  }
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Credential {
  Password(PasswordCredential),
  PreloginCookie(PreloginCookieCredential),
  AuthCookie(AuthCookieCredential),
  TokenCredential(TokenCredential),
  CachedCredential(CachedCredential),
}

impl Credential {
  /// Create a credential from a globalprotectcallback:<base64 encoded string>,
  /// or globalprotectcallback:cas-as=1&un=user@xyz.com&token=very_long_string
  pub fn from_gpcallback(auth_data: &str) -> anyhow::Result<Self> {
    let auth_data = auth_data.trim_start_matches("globalprotectcallback:");

    if auth_data.starts_with("cas-as") {
      info!("Got token auth data: {}", auth_data);
      let token_cred: TokenCredential = serde_urlencoded::from_str(auth_data)?;
      Ok(Self::TokenCredential(token_cred))
    } else {
      info!("Parsing SAML auth data...");
      let auth_data = decode_to_string(auth_data)?;
      let auth_data = SamlAuthData::from_html(&auth_data)?;

      Self::try_from(auth_data)
    }
  }

  pub fn username(&self) -> &str {
    match self {
      Credential::Password(cred) => cred.username(),
      Credential::PreloginCookie(cred) => cred.username(),
      Credential::AuthCookie(cred) => cred.username(),
      Credential::TokenCredential(cred) => cred.username(),
      Credential::CachedCredential(cred) => cred.username(),
    }
  }

  pub fn to_params(&self) -> HashMap<&str, &str> {
    let mut params = HashMap::new();
    params.insert("user", self.username());

    let (passwd, prelogin_cookie, portal_userauthcookie, portal_prelogonuserauthcookie, token) = match self {
      Credential::Password(cred) => (Some(cred.password()), None, None, None, None),
      Credential::PreloginCookie(cred) => (None, Some(cred.prelogin_cookie()), None, None, None),
      Credential::AuthCookie(cred) => (
        None,
        None,
        Some(cred.user_auth_cookie()),
        Some(cred.prelogon_user_auth_cookie()),
        None,
      ),
      Credential::TokenCredential(cred) => (None, None, None, None, Some(cred.token())),
      Credential::CachedCredential(cred) => (
        cred.password(),
        None,
        Some(cred.auth_cookie.user_auth_cookie()),
        Some(cred.auth_cookie.prelogon_user_auth_cookie()),
        None,
      ),
    };

    params.insert("passwd", passwd.unwrap_or_default());
    params.insert("prelogin-cookie", prelogin_cookie.unwrap_or_default());
    params.insert("portal-userauthcookie", portal_userauthcookie.unwrap_or_default());
    params.insert(
      "portal-prelogonuserauthcookie",
      portal_prelogonuserauthcookie.unwrap_or_default(),
    );

    if let Some(token) = token {
      params.insert("token", token);
    }

    params
  }
}

impl TryFrom<SamlAuthData> for Credential {
  type Error = anyhow::Error;

  fn try_from(value: SamlAuthData) -> Result<Self, Self::Error> {
    let prelogin_cookie = PreloginCookieCredential::try_from(value)?;

    Ok(Self::PreloginCookie(prelogin_cookie))
  }
}

impl From<PasswordCredential> for Credential {
  fn from(value: PasswordCredential) -> Self {
    Self::Password(value)
  }
}

impl From<&AuthCookieCredential> for Credential {
  fn from(value: &AuthCookieCredential) -> Self {
    Self::AuthCookie(value.clone())
  }
}

impl From<&CachedCredential> for Credential {
  fn from(value: &CachedCredential) -> Self {
    Self::CachedCredential(value.clone())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn cred_from_gpcallback_cas() {
    let auth_data = "globalprotectcallback:cas-as=1&un=xyz@email.com&token=very_long_string";

    let cred = Credential::from_gpcallback(auth_data).unwrap();

    match cred {
      Credential::TokenCredential(token_cred) => {
        assert_eq!(token_cred.username(), "xyz@email.com");
        assert_eq!(token_cred.token(), "very_long_string");
      }
      _ => panic!("Expected TokenCredential"),
    }
  }

  #[test]
  fn cred_from_gpcallback_non_cas() {
    let auth_data = "PGh0bWw+PCEtLSA8c2FtbC1hdXRoLXN0YXR1cz4xPC9zYW1sLWF1dGgtc3RhdHVzPjxwcmVsb2dpbi1jb29raWU+cHJlbG9naW4tY29va2llPC9wcmVsb2dpbi1jb29raWU+PHNhbWwtdXNlcm5hbWU+eHl6QGVtYWlsLmNvbTwvc2FtbC11c2VybmFtZT48c2FtbC1zbG8+bm88L3NhbWwtc2xvPjxzYW1sLVNlc3Npb25Ob3RPbk9yQWZ0ZXI+PC9zYW1sLVNlc3Npb25Ob3RPbk9yQWZ0ZXI+IC0tPjwvaHRtbD4=";

    let cred = Credential::from_gpcallback(auth_data).unwrap();

    match cred {
      Credential::PreloginCookie(cred) => {
        assert_eq!(cred.username(), "xyz@email.com");
        assert_eq!(cred.prelogin_cookie(), "prelogin-cookie");
      }
      _ => panic!("Expected PreloginCookieCredential")
    }
  }
}
